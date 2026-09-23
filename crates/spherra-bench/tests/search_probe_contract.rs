use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn probe_preserves_results_across_concurrent_callers_and_rejects_bad_requests() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_spherra-bench"))
        .args([
            "search-probe",
            "--rows",
            "33",
            "--queries",
            "8",
            "--seed",
            "20260804",
            "--training-rows",
            "344",
            "--index-dir",
        ])
        .arg(dir.path().join("index"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut read = || {
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        serde_json::from_str::<Value>(&line).expect("probe must emit JSON")
    };
    let ready = read();
    assert_eq!(ready["kind"], "ready");
    assert_eq!(ready["rows"], 33);
    for metric in ["cosine", "dot"] {
        writeln!(input, "{{\"metric\":\"{metric}\",\"queries\":[0]}}").unwrap();
        let serial = read();
        writeln!(
            input,
            "{{\"metric\":\"{metric}\",\"queries\":[0,0,0,0,0,0,0,0]}}"
        )
        .unwrap();
        let concurrent = read();
        for result in concurrent["results"].as_array().unwrap() {
            assert_eq!(result["fingerprint"], serial["results"][0]["fingerprint"]);
            assert_eq!(result["rows_scanned"], 33);
            assert_eq!(result["rows_refined"], 33);
            assert_eq!(result["hits"], 10);
            assert!(result["elapsed_ns"].as_u64().unwrap() > 0);
        }
    }
    writeln!(input, "{{\"metric\":\"cosine\",\"queries\":[8]}}").unwrap();
    drop(input);
    assert!(!child.wait().unwrap().success());

    for (request, success) in [
        ("{\"metric\":\"cosine\",\"queries\":[1]}\n".to_owned(), true),
        (
            "{\"metric\":\"unknown\",\"queries\":[1]}\n".to_owned(),
            false,
        ),
        ("{\"metric\":\"dot\",\"queries\":[]}\n".to_owned(), false),
        (" ".repeat(4097), false),
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_spherra-bench"))
            .args([
                "search-probe",
                "--rows",
                "33",
                "--queries",
                "8",
                "--seed",
                "20260804",
                "--training-rows",
                "344",
                "--reuse",
                "true",
                "--index-dir",
            ])
            .arg(dir.path().join("index"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.success(), success);
        if success {
            let messages = String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(messages.len(), 3);
            assert_eq!(messages[2]["kind"], "end");
            assert_eq!(messages[0]["revision"], messages[2]["revision"]);
        }
    }
}
