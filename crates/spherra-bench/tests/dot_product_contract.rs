use serde_json::Value;
use std::{path::Path, process::Command};
fn write(path: &Path, rows: &[[f32; 768]]) -> String {
    let bytes: Vec<_> = rows
        .iter()
        .flatten()
        .flat_map(|x| x.to_le_bytes())
        .collect();
    std::fs::write(path, &bytes).unwrap();
    blake3::hash(&bytes).to_hex().to_string()
}
#[test]
fn dot_product_report_is_complete_reproducible_and_rejects_bad_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let corpus = spherra_testkit::CorpusDescriptor::resolve("generated-correlated-768x400")
        .unwrap()
        .load(20260804, 4)
        .unwrap();
    let indexed = dir.path().join("indexed.f32");
    let training = dir.path().join("training.f32");
    let queries = dir.path().join("queries.f32");
    let ih = write(&indexed, &corpus.indexed()[..10]);
    let th = write(&training, corpus.calibration());
    let qh = write(&queries, corpus.queries());
    let output = dir.path().join("report.json");
    let index = dir.path().join("index");
    let run = |hash: &str| {
        Command::new(env!("CARGO_BIN_EXE_spherra-bench"))
            .arg("dot-product")
            .arg("--indexed")
            .arg(&indexed)
            .args(["--rows", "10", "--indexed-blake3", hash])
            .arg("--training")
            .arg(&training)
            .args([
                "--training-rows",
                &corpus.calibration().len().to_string(),
                "--training-blake3",
                &th,
            ])
            .arg("--queries")
            .arg(&queries)
            .args(["--query-count", "4", "--queries-blake3", &qh])
            .arg("--index-dir")
            .arg(&index)
            .arg("--output")
            .arg(&output)
            .output()
            .unwrap()
    };
    assert!(!run(&"0".repeat(64)).status.success());
    assert!(!output.exists() && !index.exists());
    let r = run(&ih);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let bytes = std::fs::read(&output).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let required = [
        "schema_version",
        "kind",
        "timestamp",
        "git_commit",
        "dirty_worktree",
        "machine",
        "command",
        "indexed_blake3",
        "training_blake3",
        "queries_blake3",
        "index_current_blake3",
        "rows",
        "training_rows",
        "query_count",
        "seed",
        "k",
        "candidate_budget",
        "enclosure_failures",
        "build_seconds",
        "dot_recall_at_k",
        "cosine_recall_against_dot_at_k",
        "queries",
    ];
    let schema: Value = serde_json::from_str(include_str!(
        "../../../docs/benchmarks/dot-product.schema.json"
    ))
    .unwrap();
    for field in required {
        assert!(value.get(field).is_some(), "{field}");
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x == field)
        );
    }
    // Optional fields without a sweep are the correction experiments.
    let experiments = [
        "renormalized_dot_recall_at_k",
        "stored_exact_dot_recall_at_k",
        "stored_fp16_dot_recall_at_k",
        "stored_u8_dot_recall_at_k",
        "minimum_alignment",
    ];
    for field in experiments {
        assert!(value[field].is_number(), "{field}");
    }
    assert_eq!(
        value.as_object().unwrap().len(),
        required.len() + experiments.len()
    );
    assert_eq!(value["enclosure_failures"], 0);
    assert_eq!(value["dot_recall_at_k"], 1.0);
    assert_eq!(value["queries"].as_array().unwrap().len(), 4);
    for query in value["queries"].as_array().unwrap() {
        for hit in query["dot_hits"].as_array().unwrap() {
            assert!(hit["lower"].as_f64().unwrap() <= hit["truth"].as_f64().unwrap());
            assert!(hit["truth"].as_f64().unwrap() <= hit["upper"].as_f64().unwrap());
        }
    }
    assert!(!run(&ih).status.success());
    assert_eq!(std::fs::read(&output).unwrap(), bytes);

    let sweep = |budgets: &str, name: &str| {
        Command::new(env!("CARGO_BIN_EXE_spherra-bench"))
            .arg("dot-product")
            .arg("--indexed")
            .arg(&indexed)
            .args(["--rows", "10", "--indexed-blake3", &ih])
            .arg("--training")
            .arg(&training)
            .args([
                "--training-rows",
                &corpus.calibration().len().to_string(),
                "--training-blake3",
                &th,
            ])
            .arg("--queries")
            .arg(&queries)
            .args(["--query-count", "4", "--queries-blake3", &qh])
            .args(["--k", "2", "--candidate-budget", "2"])
            .args(["--sweep-budgets", budgets])
            .arg("--index-dir")
            .arg(dir.path().join(format!("index-{name}")))
            .arg("--output")
            .arg(dir.path().join(format!("{name}.json")))
            .output()
            .unwrap()
    };
    assert!(!sweep("1,10", "short").status.success());
    assert!(!sweep("2,x", "garbled").status.success());
    let r = sweep("2,20", "sweep");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let value: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("sweep.json")).unwrap()).unwrap();
    let rows = value["budget_sweep"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["candidate_budget"], 2);
    // Budgets are capped at the row count, matching the public search result.
    assert_eq!(rows[1]["candidate_budget"], 10);
    assert_eq!(rows[1]["dot_recall_at_k"], 1.0);
    assert_eq!(value["dot_recall_at_k"], rows[0]["dot_recall_at_k"]);
}
#[test]
fn dot_latency_dispatch_records_its_metric_and_rejects_bad_workloads() {
    for name in ["dot-product-latency", "dot-product-latency-child"] {
        let r = Command::new(env!("CARGO_BIN_EXE_spherra-bench"))
            .args([name, "--unknown", "true"])
            .output()
            .unwrap();
        assert!(!r.status.success());
        assert!(!String::from_utf8_lossy(&r.stderr).contains("unknown subcommand"));
    }
    let schema: Value = serde_json::from_str(include_str!(
        "../../../docs/benchmarks/local-latency.schema.json"
    ))
    .unwrap();
    assert!(
        schema["properties"]["kind"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "dot-product-latency")
    );
}
