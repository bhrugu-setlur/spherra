use serde_json::{Value, json};
use std::{path::Path, process::Command};
fn file(dir: &Path, name: &str, count: usize, offset: usize) -> Value {
    let path = dir.join(format!("{name}.f32"));
    let records = dir.join(format!("{name}.jsonl"));
    let mut bytes = Vec::new();
    let mut text = String::new();
    for r in 0..count {
        let mut v = [0.0_f32; 768];
        v[(r + offset) % 768] = 1.0;
        for x in v {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
        text.push_str(&format!(
            "{{\"id\":\"{name}-{r}\",\"text\":\"{name} text {r}\"}}\n"
        ));
    }
    std::fs::write(&path, &bytes).unwrap();
    std::fs::write(&records, &text).unwrap();
    json!({"path":path,"row_count":count,"byte_len":bytes.len(),"blake3":blake3::hash(&bytes).to_hex().to_string(),"records_path":records,"records_blake3":blake3::hash(text.as_bytes()).to_hex().to_string(),"truncated_inputs":0,"embedding_seconds":0.0})
}
fn command(args: &[&str], output: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_spherra-bench"))
        .args(args)
        .args(["--output"])
        .arg(output)
        .output()
        .unwrap()
}
#[test]
fn real_query_splits_require_matching_oracles_and_reuse_training_identity() {
    let dir = tempfile::tempdir().unwrap();
    let dataset = dir.path().join("dataset.json");
    let qrels = dir.path().join("qrels.jsonl");
    std::fs::write(&qrels, "").unwrap();
    let d = json!({"schema_version":1,"name":"test-real-query-fixture","dimension":768,"provenance":{"fixture":true,"dataset_revision":"fixture","model_revision":"fixture","preprocessing":"fixture","sampling":"fixture"},"indexed":file(dir.path(),"indexed",40,0),"calibration":file(dir.path(),"calibration",344,100),"tuning":file(dir.path(),"tuning",4,0),"test":file(dir.path(),"test",4,5),"qrels":{"path":qrels,"blake3":blake3::hash(b"").to_hex().to_string(),"records":0}});
    std::fs::write(&dataset, serde_json::to_vec(&d).unwrap()).unwrap();
    let oracle = dir.path().join("oracle.json");
    let out = dir.path().join("loss.json");
    let index = dir.path().join("index");
    let r = command(
        &[
            "dataset-oracle",
            "--dataset",
            dataset.to_str().unwrap(),
            "--queries",
            "4",
            "--training-rows",
            "344",
            "--query-split",
            "tuning",
        ],
        &oracle,
    );
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let args = [
        "index-diagnose",
        "--dataset",
        dataset.to_str().unwrap(),
        "--query-split",
        "tuning",
        "--queries",
        "4",
        "--seed",
        "20260804",
        "--training-rows",
        "344",
        "--budgets",
        "10,40",
        "--oracle-reference",
        oracle.to_str().unwrap(),
        "--index-dir",
        index.to_str().unwrap(),
    ];
    let r = command(&args, &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let v: Value = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
    assert_eq!(v["source"]["query_split"], "tuning");
    assert_eq!(v["checks_passed"], true);
    assert_eq!(v["summaries"][1]["candidate_recall_at_10"], 1.0);
    let wrong = dir.path().join("wrong-index");
    let mut args2 = args.to_vec();
    args2[4] = "test";
    let last = args2.len() - 1;
    args2[last] = wrong.to_str().unwrap();
    let r = command(&args2, &dir.path().join("wrong.json"));
    assert!(!r.status.success());
    assert!(!wrong.exists());
    assert!(String::from_utf8_lossy(&r.stderr).contains("oracle"));
    let mut reuse = args.to_vec();
    reuse.extend(["--reuse", "true"]);
    let r = command(&reuse, &dir.path().join("reused.json"));
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let mut broken = d.clone();
    broken["indexed"]["blake3"] = json!("00".repeat(32));
    std::fs::write(&dataset, serde_json::to_vec(&broken).unwrap()).unwrap();
    let r = command(&reuse, &dir.path().join("broken.json"));
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("BLAKE3"));
}
