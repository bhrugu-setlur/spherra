//! Input-length diagnostics before FP16 rounding; never opens or changes an index.
use crate::local_index::{allowed, finish_revision, number};
use crate::{BenchError, Options};
use serde_json::{Value, json};
use spherra_domain::{DEFAULT_MIN_NORM_EPSILON, ValidatedVector};
use spherra_testkit::{
    machine::{MachineProfile, SourceRevision, timestamp_rfc3339_utc},
    results::validate_against_schema,
};
use std::{
    fs,
    io::{BufReader, Read, Write},
    path::Path,
    time::Instant,
};
const SCHEMA: &str = include_str!("../../../docs/benchmarks/norm-audit.schema.json");
const MAX_ROWS: u64 = 10_000_000;
fn bad(message: &str) -> BenchError {
    BenchError::harness(message)
}
fn schema() -> Result<Value, BenchError> {
    serde_json::from_str(SCHEMA).map_err(BenchError::serialize)
}
fn reference(path: &Path) -> Result<(Value, String), BenchError> {
    if fs::metadata(path).map_err(BenchError::harness)?.len() > 1_000_000 {
        return Err(bad("oversized norm baseline"));
    }
    let bytes = fs::read(path).map_err(BenchError::harness)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(BenchError::serialize)?;
    validate_against_schema(&schema()?, &value).map_err(BenchError::harness)?;
    if value["counts"]["invalid_rows"] != 0
        || !value["statistics"]["median"]
            .as_f64()
            .is_some_and(|v| v > 0.0)
        || !value["statistics"]["p95"].as_f64().is_some_and(|v| v > 0.0)
    {
        return Err(bad("baseline requires valid rows and positive median/p95"));
    }
    Ok((value, blake3::hash(&bytes).to_hex().to_string()))
}
pub(crate) fn run(o: &Options) -> Result<(), BenchError> {
    allowed(
        o,
        &[
            "input",
            "rows",
            "input-blake3",
            "output",
            "expect-unit",
            "unit-tolerance",
            "max-outside-fraction",
            "near-zero",
            "baseline",
            "shift-factor",
            "fail-on-warning",
        ],
    )?;
    let rows: u64 = o.require_parsed("rows")?;
    let expect_unit = number(o, "expect-unit", false)?;
    let tolerance = number(o, "unit-tolerance", 1e-4_f64)?;
    let outside = number(o, "max-outside-fraction", 0.01_f64)?;
    let near_zero = number(o, "near-zero", 1e-6_f64)?;
    let factor = number(o, "shift-factor", 2.0_f64)?;
    let strict = number(o, "fail-on-warning", false)?;
    if !(1..=MAX_ROWS).contains(&rows)
        || !tolerance.is_finite()
        || !(0.0..1.0).contains(&tolerance)
        || !outside.is_finite()
        || !(0.0..=1.0).contains(&outside)
        || !near_zero.is_finite()
        || near_zero <= 0.0
        || !factor.is_finite()
        || factor <= 1.0
    {
        return Err(bad("invalid norm-audit size or policy"));
    }
    let expected = o.require("input-blake3")?;
    if expected.len() != 64
        || !expected
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(bad("input-blake3 must be 64 lowercase hexadecimal digits"));
    }
    let output = Path::new(o.require("output")?);
    if output.exists() {
        return Err(bad("norm-audit output already exists"));
    }
    let input = Path::new(o.require("input")?);
    let revision = SourceRevision::capture();
    let start = Instant::now();
    let baseline = o
        .get("baseline")
        .map(|p| reference(Path::new(p)))
        .transpose()?;
    let file = fs::File::open(input).map_err(BenchError::harness)?;
    if file.metadata().map_err(BenchError::harness)?.len() != rows * 768 * 4 {
        return Err(bad(
            "input length does not match rows of 768 little-endian FP32 values",
        ));
    }
    let mut reader = BufReader::new(file);
    let mut hash = blake3::Hasher::new();
    // One FP64 norm per row, at most80 MB. Original vectors are streamed one row
    // at a time; percentile values are exact nearest-rank, not sampled estimates.
    let mut norms = Vec::with_capacity(rows as usize);
    let (
        mut non_finite,
        mut below,
        mut above,
        mut underflow,
        mut accepted,
        mut near_unit,
        mut small,
    ) = (0_u64, 0_u64, 0_u64, 0_u64, 0_u64, 0_u64, 0_u64);
    let mut first_invalid = Vec::new();
    let mut maximum_relative_rounding_error = 0.0_f64;
    for ordinal in 0..rows {
        let mut bytes = [0_u8; 3072];
        reader.read_exact(&mut bytes).map_err(BenchError::harness)?;
        hash.update(&bytes);
        let row: [f32; 768] = std::array::from_fn(|c| {
            f32::from_le_bytes(bytes[c * 4..c * 4 + 4].try_into().unwrap())
        });
        let reason = if row.iter().any(|v| !v.is_finite()) {
            non_finite += 1;
            Some("non_finite_component")
        } else {
            // Match domain validation's FP64 fused reduction before any normalization.
            let norm = row
                .iter()
                .fold(0.0_f64, |sum, &v| f64::from(v).mul_add(f64::from(v), sum))
                .sqrt();
            norms.push(norm);
            near_unit += u64::from((norm - 1.0).abs() <= tolerance);
            small += u64::from(norm <= near_zero);
            if norm > 65504.0 {
                above += 1;
                Some("norm_above_fp16_maximum")
            } else {
                let validated = ValidatedVector::new(row.to_vec()).map_err(BenchError::harness)?;
                let stored = f64::from(validated.radius_f32());
                underflow += u64::from(norm > 0.0 && stored == 0.0);
                if norm > 0.0 {
                    maximum_relative_rounding_error =
                        maximum_relative_rounding_error.max((stored - norm).abs() / norm);
                }
                if norm < DEFAULT_MIN_NORM_EPSILON {
                    below += 1;
                    Some("norm_below_minimum")
                } else {
                    accepted += 1;
                    None
                }
            }
        };
        if let Some(reason) = reason {
            if first_invalid.len() < 8 {
                first_invalid.push(json!({"row":ordinal,"reason":reason}));
            }
        }
    }
    let mut extra = [0];
    if reader.read(&mut extra).map_err(BenchError::harness)? != 0 {
        return Err(bad("input grew during norm audit"));
    }
    let actual = hash.finalize().to_hex().to_string();
    if actual != expected {
        return Err(bad("input BLAKE3 mismatch"));
    }
    norms.sort_unstable_by(f64::total_cmp);
    let statistics = if norms.is_empty() {
        Value::Null
    } else {
        let percentile = |p: usize| norms[(p * norms.len()).div_ceil(100) - 1];
        json!({"minimum":norms[0],"median":percentile(50),"p95":percentile(95),"p99":percentile(99),"maximum":norms[norms.len()-1],"near_unit_fraction":near_unit as f64/norms.len() as f64})
    };
    let mut warnings = Vec::new();
    if expect_unit
        && !norms.is_empty()
        && (norms.len() as u64 - near_unit) as f64 > outside * norms.len() as f64
    {
        warnings.push("unit_length_policy_exceeded");
    }
    if small > 0 {
        warnings.push("near_zero_inputs");
    }
    if underflow > 0 {
        warnings.push("fp16_magnitude_underflow");
    }
    let comparison=baseline.map(|(b,hash)|{
  let ratio=|key:&str|statistics[key].as_f64().map(|v|v/b["statistics"][key].as_f64().unwrap());
  let median=ratio("median");let p95=ratio("p95");
  if median.is_some_and(|v|v>factor || v<1.0/factor){warnings.push("median_scale_shift");}
  if p95.is_some_and(|v|v>factor || v<1.0/factor){warnings.push("p95_scale_shift");}
  json!({"path":o.get("baseline"),"report_blake3":hash,"input_blake3":b["input"]["blake3"],"median_ratio":median,"p95_ratio":p95})
 });
    let invalid = rows - accepted;
    let fail = invalid > 0 || (strict && !warnings.is_empty());
    let mut result = json!({"schema_version":1,"kind":"norm-audit","git_commit":revision.commit,"dirty_worktree":revision.dirty,"timestamp":timestamp_rfc3339_utc(),"machine":MachineProfile::capture(),"command":format!("spherra-bench norm-audit {}",o.0.iter().map(|(k,v)|format!("--{k} {v}")).collect::<Vec<_>>().join(" ")),"input":{"path":input,"rows":rows,"dimension":768,"byte_len":rows*3072,"blake3":actual},"policy":{"expect_unit":expect_unit,"unit_tolerance":tolerance,"max_outside_fraction":outside,"near_zero":near_zero,"shift_factor":factor,"fail_on_warning":strict},"counts":{"finite_rows":norms.len(),"accepted_rows":accepted,"invalid_rows":invalid,"non_finite_rows":non_finite,"norm_below_minimum":below,"norm_above_fp16_maximum":above,"positive_fp16_underflow":underflow,"near_unit_rows":near_unit,"near_zero_rows":small},"statistics":statistics,"maximum_relative_fp16_rounding_error":maximum_relative_rounding_error,"first_invalid":first_invalid,"warnings":warnings,"baseline":comparison,"gate_passed":!fail,"elapsed_seconds":start.elapsed().as_secs_f64()});
    finish_revision(&mut result);
    validate_against_schema(&schema()?, &result).map_err(BenchError::harness)?;
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(BenchError::harness)?;
    }
    let mut writer = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output)
        .map_err(BenchError::harness)?;
    serde_json::to_writer_pretty(&mut writer, &result).map_err(BenchError::serialize)?;
    writer.write_all(b"\n").map_err(BenchError::harness)?;
    if fail {
        Err(bad(
            "norm audit reported invalid rows or strict-policy warnings; see output report",
        ))
    } else {
        Ok(())
    }
}
