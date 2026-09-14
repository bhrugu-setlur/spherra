use serde_json::Value;
use std::{path::Path, process::Command};
fn audit(
    dir: &Path,
    name: &str,
    lengths: &[f32],
    extra: &[&str],
) -> (std::process::Output, std::path::PathBuf) {
    let input = dir.join(format!("{name}.f32"));
    let output = dir.join(format!("{name}.json"));
    let mut bytes = Vec::new();
    for &length in lengths {
        for c in 0..768 {
            bytes.extend_from_slice(&(if c == 0 { length } else { 0.0_f32 }).to_le_bytes());
        }
    }
    std::fs::write(&input, &bytes).unwrap();
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let r = Command::new(env!("CARGO_BIN_EXE_spherra-bench"))
        .args(["norm-audit", "--input"])
        .arg(input)
        .args([
            "--rows",
            &lengths.len().to_string(),
            "--input-blake3",
            &hash,
            "--output",
        ])
        .arg(&output)
        .args(extra)
        .output()
        .unwrap();
    (r, output)
}
fn read(p: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}
#[test]
fn reports_exact_statistics_and_warns_only_with_explicit_scale_policy() {
    let d = tempfile::tempdir().unwrap();
    let (r, p) = audit(d.path(), "varied", &[1., 1., 1., 14.], &[]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let v = read(&p);
    assert_eq!(v["statistics"]["median"], 1.0);
    assert_eq!(v["statistics"]["p95"], 14.0);
    assert_eq!(v["statistics"]["near_unit_fraction"], 0.75);
    assert_eq!(v["warnings"], serde_json::json!([]));
    let (r, p) = audit(
        d.path(),
        "before-rounding",
        &[1.0001],
        &["--expect-unit", "true", "--unit-tolerance", "0.000001"],
    );
    assert!(r.status.success());
    assert!(read(&p)["statistics"]["minimum"].as_f64().unwrap() > 1.0);
    assert_eq!(
        read(&p)["warnings"],
        serde_json::json!(["unit_length_policy_exceeded"])
    );

    let (r, p) = audit(
        d.path(),
        "mixed",
        &[1., 1., 1., 14.],
        &["--expect-unit", "true"],
    );
    assert!(r.status.success());
    assert_eq!(
        read(&p)["warnings"],
        serde_json::json!(["unit_length_policy_exceeded"])
    );
    let (r, p) = audit(
        d.path(),
        "strict",
        &[1., 14.],
        &["--expect-unit", "true", "--fail-on-warning", "true"],
    );
    assert!(!r.status.success());
    assert!(p.exists());
}
#[test]
fn detects_rescaling_against_a_reference_and_binds_its_identity() {
    let d = tempfile::tempdir().unwrap();
    let (r, p) = audit(d.path(), "baseline", &[1., 1., 1., 1.], &[]);
    assert!(r.status.success());
    let (r, out) = audit(
        d.path(),
        "shift",
        &[14., 14., 14., 14.],
        &["--baseline", p.to_str().unwrap()],
    );
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let v = read(&out);
    assert_eq!(v["baseline"]["median_ratio"], 14.0);
    assert_eq!(
        v["baseline"]["report_blake3"],
        blake3::hash(&std::fs::read(p).unwrap())
            .to_hex()
            .to_string()
    );
    assert!(
        v["warnings"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("median_scale_shift"))
    );
}
#[test]
fn preserves_tiny_norms_before_fp16_rounding_and_reports_rejected_rows() {
    let d = tempfile::tempdir().unwrap();
    let (r, p) = audit(
        d.path(),
        "bad",
        &[0., 1e-13, 1e-10, 1e-7, 70000., f32::NAN, f32::INFINITY],
        &[],
    );
    assert!(!r.status.success());
    let v = read(&p);
    assert_eq!(v["counts"]["non_finite_rows"], 2);
    assert_eq!(v["counts"]["norm_below_minimum"], 2);
    assert_eq!(v["counts"]["norm_above_fp16_maximum"], 1);
    assert_eq!(v["counts"]["positive_fp16_underflow"], 2);
    assert_eq!(v["counts"]["accepted_rows"], 2);
    assert_eq!(v["statistics"]["minimum"], 0.0);
    assert!(v["statistics"]["median"].as_f64().unwrap() > 0.0);
    let (r, p) = audit(d.path(), "allbad", &[f32::NAN], &[]);
    assert!(!r.status.success());
    assert!(read(&p)["statistics"].is_null());
}
#[test]
fn rejects_wrong_hash_truncation_bad_policy_and_overwrite_without_output() {
    let d = tempfile::tempdir().unwrap();
    let input = d.path().join("input");
    std::fs::write(&input, [0; 3072]).unwrap();
    let output = d.path().join("out");
    let run = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_spherra-bench"))
            .args(["norm-audit", "--input"])
            .arg(&input)
            .args([
                "--rows",
                "1",
                "--input-blake3",
                &"00".repeat(32),
                "--output",
            ])
            .arg(&output)
            .args(extra)
            .output()
            .unwrap()
    };
    assert!(!run(&[]).status.success());
    assert!(!output.exists());
    assert!(!run(&["--unit-tolerance", "NaN"]).status.success());
    assert!(!output.exists());
    std::fs::write(&input, [0; 3071]).unwrap();
    assert!(!run(&[]).status.success());
    assert!(!output.exists());
    std::fs::write(&output, b"keep").unwrap();
    assert!(!run(&[]).status.success());
    assert_eq!(std::fs::read(output).unwrap(), b"keep");
}
