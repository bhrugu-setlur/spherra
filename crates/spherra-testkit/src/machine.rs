//! Machine, toolchain, and worktree provenance for a recorded measurement.
//!
//! Every field here answers "on what, and from which source revision, was this
//! measured?". CPU and memory come from `sysinfo`; OS, architecture, and the
//! toolchain come from Rust's own compile-time facts. Nothing shells out to a
//! command whose output could carry a user path or a secret: the only external
//! process is `git`, invoked with fixed arguments that return a commit hash and
//! a porcelain status, never a filesystem path.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct MachineProfile {
    pub os: String,
    pub architecture: String,
    pub cpu: String,
    pub physical_memory_bytes: u64,
    pub rustc: String,
    pub cargo_profile: String,
}

impl MachineProfile {
    pub fn capture() -> Self {
        let system = System::new_with_specifics(
            RefreshKind::nothing()
                .with_memory(MemoryRefreshKind::nothing().with_ram())
                .with_cpu(CpuRefreshKind::nothing()),
        );
        let cpu = system
            .cpus()
            .first()
            .map(|cpu| cpu.brand().trim().to_owned())
            .filter(|brand| !brand.is_empty())
            .unwrap_or_else(|| "unknown".to_owned());

        Self {
            os: format!(
                "{} {}",
                System::name().unwrap_or_else(|| std::env::consts::OS.to_owned()),
                System::os_version().unwrap_or_else(|| "unknown".to_owned()),
            ),
            architecture: std::env::consts::ARCH.to_owned(),
            cpu,
            physical_memory_bytes: system.total_memory(),
            rustc: env!("SPHERRA_RUSTC_VERSION").to_owned(),
            cargo_profile: env!("SPHERRA_CARGO_PROFILE").to_owned(),
        }
    }
}

/// The source revision a measurement was taken from.
///
/// `commit` is `"unknown"` when git is unavailable; `dirty` is then `true`,
/// because an unverifiable worktree must never be recorded as clean.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SourceRevision {
    pub commit: String,
    pub dirty: bool,
}

impl SourceRevision {
    pub fn capture() -> Self {
        let commit = git_output(&["rev-parse", "HEAD"]);
        let status = git_output(&["status", "--porcelain"]);
        match (commit, status) {
            (Some(commit), Some(status)) => Self {
                commit,
                dirty: !status.is_empty(),
            },
            _ => Self {
                commit: "unknown".to_owned(),
                dirty: true,
            },
        }
    }
}

fn git_output(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").args(arguments).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|text| text.trim().to_owned())
}

/// How warm the page cache and allocator were when the measurement ran.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheState {
    Cold,
    Warm,
    Hot,
}

impl CacheState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Warm => "warm",
            Self::Hot => "hot",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "cold" => Some(Self::Cold),
            "warm" => Some(Self::Warm),
            "hot" => Some(Self::Hot),
            _ => None,
        }
    }
}

/// M1 performs no durable write, so it records the absence of an fsync mode
/// rather than inventing one. A later storage milestone replaces this.
pub const DURABILITY_MODE_NOT_APPLICABLE: &str = "not-applicable";

/// The current instant as an RFC 3339 UTC string.
pub fn timestamp_rfc3339_utc() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    format_rfc3339_utc(seconds)
}

/// Civil date from Unix seconds, using Howard Hinnant's `civil_from_days`.
fn format_rfc3339_utc(unix_seconds: u64) -> String {
    let days = (unix_seconds / 86_400) as i64;
    let second_of_day = unix_seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let shifted = days_since_epoch + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::{civil_from_days, format_rfc3339_utc};

    #[test]
    fn the_epoch_and_known_instants_round_trip_to_utc_calendar_dates() {
        assert_eq!(format_rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339_utc(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(format_rfc3339_utc(1_780_000_000), "2026-05-28T20:26:40Z");
        // 2024 was a leap year; day 60 of it is 29 February.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }
}
