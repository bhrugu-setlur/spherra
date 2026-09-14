use std::process::Command;
#[test]
fn invalid_original_rerank_workload_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let originals = d.path().join("originals.f32");
    let r = Command::new(env!("CARGO_BIN_EXE_spherra-bench"))
        .args([
            "original-rerank",
            "--rows",
            "0",
            "--seed",
            "20260804",
            "--originals",
        ])
        .arg(&originals)
        .args(["--output"])
        .arg(d.path().join("out.json"))
        .output()
        .unwrap();
    assert!(!r.status.success());
    assert!(!originals.exists());
    assert!(String::from_utf8_lossy(&r.stderr).contains("workload"));
}
