use serde_json::Value;
use std::{path::Path, process::Command};
fn run(kind: &str, dir: &Path, extra: &[&str]) -> Value {
    let output = dir.join(format!("{kind}.json"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_spherra-bench"));
    command
        .args([kind, "--seed", "20260804", "--output"])
        .arg(&output)
        .args(extra);
    if kind == "latency" {
        command.arg("--index-dir").arg(dir.join("index"));
    }
    let result = command.output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap()
}
fn schema_check(kind: &str, value: &Value, extra: &[&str]) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let schema: Value = serde_json::from_slice(
        &std::fs::read(root.join(format!("docs/benchmarks/local-{kind}.schema.json"))).unwrap(),
    )
    .unwrap();
    let mut expected = vec![
        "durability_mode",
        "schema_version",
        "kind",
        "timestamp",
        "git_commit",
        "dirty_worktree",
        "machine",
        "command",
        "source",
    ];
    expected.extend_from_slice(extra);
    let mut actual: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected);
    spherra_testkit::results::validate_against_schema(&schema, value).unwrap();
    for field in expected {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            spherra_testkit::results::validate_against_schema(&schema, &missing).is_err(),
            "missing {field}"
        );
    }
}
#[test]
fn reference_command_pins_deterministic_bytes_and_complete_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let v = run(
        "oracle-reference",
        dir.path(),
        &["--rows", "400", "--queries", "4"],
    );
    schema_check(
        "oracle-reference",
        &v,
        &[
            "corpus_hash",
            "query_hash",
            "query_count",
            "k",
            "artifact_path",
            "artifact_bytes",
            "artifact_blake3",
            "elapsed_seconds",
            "rows_scored",
            "workers",
        ],
    );
    let bytes = std::fs::read(v["artifact_path"].as_str().unwrap()).unwrap();
    assert_eq!(&bytes[..8], b"SPHROR01");
    assert_eq!(u64::from_le_bytes(bytes[8..16].try_into().unwrap()), 400);
    assert_eq!(bytes.len(), 6440);
    assert_eq!(
        blake3::hash(&bytes).to_hex().to_string(),
        v["artifact_blake3"]
    );
    let again = run(
        "oracle-reference",
        dir.path(),
        &["--rows", "400", "--queries", "4"],
    );
    assert_eq!(v["artifact_blake3"], again["artifact_blake3"]);
    assert_eq!(v["corpus_hash"], again["corpus_hash"]);
}
#[test]
#[cfg(target_os = "macos")]
fn latency_and_memory_children_report_measured_fields_without_claiming_smoke_gates() {
    let dir = tempfile::tempdir().unwrap();
    let v = run(
        "latency",
        dir.path(),
        &[
            "--rows",
            "400",
            "--queries",
            "4",
            "--warmup",
            "2",
            "--training-rows",
            "344",
        ],
    );
    schema_check(
        "latency",
        &v,
        &[
            "corpus_hash",
            "query_hash",
            "training_rows",
            "query_count",
            "warmup_queries",
            "k",
            "candidate_budget",
            "workers",
            "kernel",
            "cache_state",
            "ac_power",
            "build_seconds",
            "build_rows_per_second",
            "build_source_commit",
            "build_dirty_worktree",
            "reused_index",
            "open_seconds",
            "descriptors_held",
            "generation",
            "segment_count",
            "latencies_ms",
            "p50_ms",
            "p99_ms",
            "peak_rss_bytes",
            "gate_eligible",
            "gate_passed",
            "time_log",
            "model",
            "index_current_blake3",
            "drift_warnings",
        ],
    );
    assert_eq!(v["latencies_ms"].as_array().unwrap().len(), 4);
    assert_eq!(
        v["descriptors_held"],
        v["segment_count"].as_u64().unwrap() + 1
    );
    assert_eq!(v["gate_eligible"], false);
    assert!(v["peak_rss_bytes"].as_u64().unwrap() > 0);
    let reused = run(
        "latency",
        dir.path(),
        &[
            "--rows",
            "400",
            "--queries",
            "4",
            "--warmup",
            "2",
            "--training-rows",
            "344",
            "--reuse",
            "true",
        ],
    );
    assert_eq!(reused["reused_index"], true);
    assert_eq!(reused["index_current_blake3"], v["index_current_blake3"]);
    let v = run(
        "build-memory",
        dir.path(),
        &["--rows", "33", "--training-rows", "344"],
    );
    schema_check(
        "build-memory",
        &v,
        &[
            "pre_input_rss_bytes",
            "post_input_rss_bytes",
            "input_bytes",
            "input_logical_bytes",
            "training_rows",
            "pushed_rows",
            "peak_rss_bytes",
            "builder_owned_bytes",
            "limit_bytes",
            "gate_eligible",
            "gate_passed",
            "time_log",
        ],
    );
    assert_eq!(v["gate_eligible"], false);
    assert_eq!(v["input_logical_bytes"], (33 + 344) * 768 * 4);
    assert_eq!(
        v["builder_owned_bytes"].as_u64().unwrap(),
        v["peak_rss_bytes"].as_u64().unwrap()
            - v["pre_input_rss_bytes"].as_u64().unwrap()
            - v["input_bytes"].as_u64().unwrap()
    );
}
