use serde_json::Value;
use std::{path::Path, process::Command};
fn run(args: &[&str], output: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_spherra-bench"))
        .args(args)
        .args(["--seed", "20260804", "--output"])
        .arg(output)
        .output()
        .unwrap()
}
#[test]
fn loss_diagnostic_accounts_for_every_true_neighbor_and_checks_all_candidates() {
    let dir = tempfile::tempdir().unwrap();
    let oracle = dir.path().join("oracle.json");
    assert!(
        run(
            &["oracle-reference", "--rows", "400", "--queries", "4"],
            &oracle
        )
        .status
        .success()
    );
    let index = dir.path().join("index");
    let output = dir.path().join("loss.json");
    let result = run(
        &[
            "index-diagnose",
            "--rows",
            "400",
            "--queries",
            "4",
            "--training-rows",
            "344",
            "--budgets",
            "10,20,400",
            "--oracle-reference",
            oracle.to_str().unwrap(),
            "--index-dir",
            index.to_str().unwrap(),
        ],
        &output,
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let v: Value = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    assert_eq!(v["checks_passed"], true);
    let schema: Value = serde_json::from_str(include_str!(
        "../../../docs/benchmarks/local-index-loss.schema.json"
    ))
    .unwrap();
    spherra_testkit::results::validate_against_schema(&schema, &v).unwrap();
    for field in [
        "trace",
        "oracle_reference",
        "build_source_commit",
        "query_hash",
        "checks_passed",
        "summaries",
    ] {
        let mut missing = v.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(spherra_testkit::results::validate_against_schema(&schema, &missing).is_err());
    }
    let trace = std::fs::read(v["trace"]["path"].as_str().unwrap()).unwrap();
    assert_eq!(
        blake3::hash(&trace).to_hex().as_str(),
        v["trace"]["blake3"].as_str().unwrap()
    );
    assert_eq!(trace.len() as u64, v["trace"]["bytes"].as_u64().unwrap());
    let lines: Vec<Value> = String::from_utf8(trace)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 4);
    assert!(
        lines
            .iter()
            .all(|l| l["candidates"].as_array().unwrap().len() == 400)
    );
    assert_eq!(v["query_results"].as_array().unwrap().len(), 4);
    for q in v["query_results"].as_array().unwrap() {
        let mut previous = 0;
        for b in q["budgets"].as_array().unwrap() {
            let covered = b["candidate_matches"].as_u64().unwrap();
            let delivered = b["delivered_matches"].as_u64().unwrap();
            assert!(covered >= previous && covered >= delivered);
            previous = covered;
            assert_eq!(covered + b["selection_losses"].as_u64().unwrap(), 10);
            assert_eq!(delivered + b["ranking_losses"].as_u64().unwrap(), covered);
            assert_eq!(b["exact_rerank_matches"], covered);
            assert_eq!(
                b["candidate_count"].as_u64().unwrap(),
                b["budget"].as_u64().unwrap()
            );
            assert_eq!(b["neighbors"].as_array().unwrap().len(), 10);
            for h in b["hits"].as_array().unwrap() {
                assert_eq!(h["reference_score"], h["public_score"]);
                assert!(h["reconstruction_length"].as_f64().unwrap() > 0.0);
                assert!(h["lower"].as_f64().unwrap() <= h["truth"].as_f64().unwrap());
                assert!(h["truth"].as_f64().unwrap() <= h["upper"].as_f64().unwrap());
            }
        }
        assert_eq!(previous, 10);
    }
}
#[test]
fn bad_diagnostic_budget_fails_before_index_creation() {
    let dir = tempfile::tempdir().unwrap();
    let index = dir.path().join("index");
    let r = run(
        &[
            "index-diagnose",
            "--budgets",
            "9,200",
            "--index-dir",
            index.to_str().unwrap(),
        ],
        &dir.path().join("out.json"),
    );
    assert!(!r.status.success());
    assert!(!index.exists());
    assert!(String::from_utf8_lossy(&r.stderr).contains("budget"));
}

#[test]
fn diagnostic_rejects_a_tampered_oracle_before_building() {
    let dir = tempfile::tempdir().unwrap();
    let oracle = dir.path().join("oracle.json");
    assert!(
        run(
            &["oracle-reference", "--rows", "400", "--queries", "4"],
            &oracle
        )
        .status
        .success()
    );
    let mut value: Value = serde_json::from_slice(&std::fs::read(&oracle).unwrap()).unwrap();
    value["artifact_blake3"] = serde_json::json!("00".repeat(32));
    std::fs::write(&oracle, serde_json::to_vec(&value).unwrap()).unwrap();
    let index = dir.path().join("index");
    let r = run(
        &[
            "index-diagnose",
            "--rows",
            "400",
            "--queries",
            "4",
            "--oracle-reference",
            oracle.to_str().unwrap(),
            "--index-dir",
            index.to_str().unwrap(),
        ],
        &dir.path().join("out.json"),
    );
    assert!(!r.status.success());
    assert!(!index.exists());
    assert!(String::from_utf8_lossy(&r.stderr).contains("BLAKE3"));
}
