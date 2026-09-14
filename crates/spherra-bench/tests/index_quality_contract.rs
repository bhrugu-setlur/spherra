use serde_json::Value;
use std::{path::Path, process::Command};

fn command(args: &[&str], output: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_spherra-bench"))
        .args(args)
        .args(["--seed", "20260804", "--output"])
        .arg(output)
        .output()
        .unwrap()
}
fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
fn check(v: &Value) {
    assert_eq!(v["queries_checked"], 4);
    assert_eq!(v["hits_checked"], 40);
    assert_eq!(v["equality_differences"], 0);
    assert_eq!(v["enclosure_failures"], 0);
    assert_eq!(v["gate_passed"], true);
    assert_eq!(v["gate_eligible"], false);
    assert_eq!(v["query_results"].as_array().unwrap().len(), 4);
    let schema: Value = serde_json::from_str(include_str!(
        "../../../docs/benchmarks/local-index-quality.schema.json"
    ))
    .unwrap();
    spherra_testkit::results::validate_against_schema(&schema, v).unwrap();
    let expected = [
        "schema_version",
        "kind",
        "timestamp",
        "git_commit",
        "dirty_worktree",
        "machine",
        "command",
        "source",
        "durability_mode",
        "corpus_name",
        "corpus_hash",
        "query_hash",
        "seed",
        "vector_count",
        "query_count",
        "training_rows",
        "k",
        "candidate_budget",
        "generation",
        "segment_count",
        "model",
        "index_current_blake3",
        "oracle_reference",
        "historical_reference",
        "queries_checked",
        "hits_checked",
        "equality_differences",
        "enclosure_failures",
        "recall_at_10",
        "query_results",
        "gate_eligible",
        "gate_passed",
        "elapsed_seconds",
        "build_source_commit",
        "build_dirty_worktree",
        "reused_index",
    ];
    assert_eq!(v.as_object().unwrap().len(), expected.len());
    for field in expected {
        assert!(v.get(field).is_some(), "{field}");
        let mut missing = v.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            spherra_testkit::results::validate_against_schema(&schema, &missing).is_err(),
            "{field}"
        );
    }
    let matches: u64 = v["query_results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| q["exact_matches"].as_u64().unwrap())
        .sum();
    assert_eq!(v["recall_at_10"].as_f64().unwrap(), matches as f64 / 40.0);
    for query in v["query_results"].as_array().unwrap() {
        for hit in query["hits"].as_array().unwrap() {
            assert_eq!(hit["row"], hit["expected_row"]);
            assert_eq!(hit["score"], hit["expected_score"]);
            assert!(hit["expected_length"].as_f64().unwrap() > 0.0);
            assert!(hit["lower"].as_f64().unwrap() <= hit["truth"].as_f64().unwrap());
            assert!(hit["upper"].as_f64().unwrap() >= hit["truth"].as_f64().unwrap());
        }
    }
}

#[test]
fn public_index_quality_records_every_hit_and_rejects_wrong_history() {
    let dir = tempfile::tempdir().unwrap();
    let index = dir.path().join("index");
    let output = dir.path().join("quality.json");
    let r = command(
        &[
            "index",
            "--corpus",
            "generated-correlated-768x400",
            "--queries",
            "4",
            "--candidate-budget",
            "200",
            "--index-dir",
            index.to_str().unwrap(),
        ],
        &output,
    );
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    check(&read(&output));
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let historical = root.join("docs/benchmarks/results/2026-09-13-local-index-reference-generated-correlated-768x20000.json");
    let wrong = dir.path().join("wrong");
    let r = command(
        &[
            "index",
            "--corpus",
            "generated-correlated-768x400",
            "--queries",
            "4",
            "--candidate-budget",
            "200",
            "--index-dir",
            wrong.to_str().unwrap(),
            "--historical-reference",
            historical.to_str().unwrap(),
        ],
        &output,
    );
    assert!(!r.status.success());
    assert!(!wrong.exists());
    assert!(String::from_utf8_lossy(&r.stderr).contains("historical reference"));
}

#[test]
fn chunked_quality_requires_the_exact_pinned_oracle_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let reference = dir.path().join("oracle.json");
    let r = command(
        &["oracle-reference", "--rows", "400", "--queries", "4"],
        &reference,
    );
    assert!(r.status.success());
    let index = dir.path().join("index");
    let output = dir.path().join("quality.json");
    let r = command(
        &[
            "index",
            "--rows",
            "400",
            "--queries",
            "4",
            "--training-rows",
            "344",
            "--candidate-budget",
            "200",
            "--index-dir",
            index.to_str().unwrap(),
            "--oracle-reference",
            reference.to_str().unwrap(),
        ],
        &output,
    );
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    check(&read(&output));
    let mut bad = read(&reference);
    bad["artifact_blake3"] = serde_json::json!("00".repeat(32));
    let bad_path = dir.path().join("bad-oracle.json");
    std::fs::write(&bad_path, serde_json::to_vec(&bad).unwrap()).unwrap();
    let wrong = dir.path().join("wrong");
    let r = command(
        &[
            "index",
            "--rows",
            "400",
            "--queries",
            "4",
            "--training-rows",
            "344",
            "--index-dir",
            wrong.to_str().unwrap(),
            "--oracle-reference",
            bad_path.to_str().unwrap(),
        ],
        &output,
    );
    assert!(!r.status.success());
    assert!(!wrong.exists());
    assert!(String::from_utf8_lossy(&r.stderr).contains("oracle"));
}
