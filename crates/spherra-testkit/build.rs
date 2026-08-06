//! Captures the toolchain facts a measurement must record.
//!
//! Both values come from Cargo's own environment rather than from a shell
//! command that could embed a user path: `RUSTC` is the compiler Cargo already
//! chose, and `PROFILE` is the profile it is building.

use std::process::Command;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=RUSTC");
    println!("cargo::rerun-if-env-changed=PROFILE");

    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let version = Command::new(rustc)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo::rustc-env=SPHERRA_RUSTC_VERSION={version}");

    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned());
    println!("cargo::rustc-env=SPHERRA_CARGO_PROFILE={profile}");
}
