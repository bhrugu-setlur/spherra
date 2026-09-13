use super::*;
use crate::{
    Index, SearchOptions,
    fs::{Fault, FaultyFs},
};
use std::process::Command;
fn training() -> Vec<Vector> {
    vec![[1.0; 768]; 257]
}
fn options() -> CreateOptions {
    CreateOptions {
        seed: 17,
        validation_rows: Some(1),
    }
}
fn attempt(dir: &Path, append: bool, fs: Arc<dyn FileSystem>) -> Result<CommitReport, Error> {
    let mut b = if append {
        IndexBuilder::append_with_fs(dir, fs)?
    } else {
        IndexBuilder::create_with_fs(dir, &training(), options(), fs)?
    };
    b.push(&[1.0; 768])?;
    b.commit()
}
fn setup(dir: &Path, append: bool) {
    if append {
        attempt(dir, false, Arc::new(RealFs)).unwrap();
    }
}
fn visible(dir: &Path) -> Option<(u64, u64)> {
    match Index::open(dir) {
        Ok(index) => {
            let result = index
                .search(
                    &[1.0; 768],
                    SearchOptions {
                        k: 10,
                        candidate_budget: None,
                    },
                )
                .unwrap();
            assert_eq!(result.hits().len() as u64, index.len());
            Some((index.generation(), index.len()))
        }
        Err(Error::NotFound) => None,
        Err(e) => panic!("mixed or corrupt generation after failure: {e}"),
    }
}
fn trace(append: bool) -> (Vec<&'static str>, usize) {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("index");
    setup(&dir, append);
    let fs = Arc::new(FaultyFs::new(None, Fault::Error));
    attempt(&dir, append, fs.clone()).unwrap();
    let ops = fs.operations();
    let publication = ops.iter().rposition(|op| *op == "rename").unwrap() + 1;
    assert_eq!(ops[publication], "sync_dir");
    (ops, publication)
}
#[test]
#[ignore = "Task 9 release qualification: every create/append filesystem call and short write"]
fn every_filesystem_failure_matches_publication_outcomes() {
    for append in [false, true] {
        let (ops, publication) = trace(append);
        let old = append.then_some((1, 1));
        let new = if append { (2, 2) } else { (1, 1) };
        for at in 1..=ops.len() {
            let temp = tempfile::tempdir().unwrap();
            let dir = temp.path().join("index");
            setup(&dir, append);
            let before = std::fs::read(dir.join("CURRENT")).ok();
            let fs = Arc::new(FaultyFs::new(Some(at), Fault::Error));
            let result = attempt(&dir, append, fs);
            if at <= publication {
                assert!(
                    matches!(result, Err(Error::Io(_))),
                    "append={append} call={at} op={} result={result:?}",
                    ops[at - 1]
                );
                assert_eq!(std::fs::read(dir.join("CURRENT")).ok(), before);
                assert_eq!(visible(&dir), old);
            } else if at == publication + 1 {
                assert!(
                    matches!(result,Err(Error::CommitOutcomeUnknown{generation}) if generation==new.0)
                );
                assert_eq!(visible(&dir), Some(new));
            } else {
                let report = result.unwrap();
                assert!(!report.cleanup_complete(), "call={at}");
                assert_eq!(visible(&dir), Some(new));
            }
            // Every abandoned create can be retried; every published generation
            // can be appended. This also exercises cleanup of incomplete files.
            let present = visible(&dir).is_some();
            let retry = attempt(&dir, present, Arc::new(RealFs)).unwrap();
            assert!(retry.cleanup_complete());
            let (_, m, _) = storage::load(&RealFs, &dir).unwrap();
            let names: Vec<_> = std::fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            assert!(!names.iter().any(|n| n.ends_with(".tmp")));
            // LOCK, CURRENT, one model, all committed manifests and segment pairs.
            assert_eq!(
                names.len(),
                3 + m.generation as usize + 2 * m.segments.len()
            );
            assert_eq!(
                m.total_rows,
                old.map_or(0, |v| v.1) + u64::from(at > publication) + 1
            );
        }
        for (i, op) in ops.iter().enumerate().filter(|(_, op)| **op == "write") {
            let temp = tempfile::tempdir().unwrap();
            let dir = temp.path().join("index");
            setup(&dir, append);
            let report = attempt(
                &dir,
                append,
                Arc::new(FaultyFs::new(Some(i + 1), Fault::ShortWrite)),
            )
            .unwrap();
            assert!(report.cleanup_complete(), "{op}");
            assert_eq!(visible(&dir), Some(new));
        }
        eprintln!(
            "append={append}: {} numbered failures and {} partial-write points match D§9",
            ops.len(),
            ops.iter().filter(|op| **op == "write").count()
        );
    }
}
#[test]
fn directory_states_locks_and_cleanup_retry() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        IndexBuilder::append(dir.path()),
        Err(Error::NotFound)
    ));
    assert!(matches!(Index::open(dir.path()), Err(Error::NotFound)));
    std::fs::write(dir.path().join("notes.txt"), "keep me").unwrap();
    std::fs::write(dir.path().join("CURRENT.tmp"), "abandoned").unwrap();
    attempt(dir.path(), false, Arc::new(RealFs)).unwrap();
    assert!(!dir.path().join("CURRENT.tmp").exists());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
        "keep me"
    );
    assert!(matches!(
        IndexBuilder::create(dir.path(), &training(), options()),
        Err(Error::AlreadyExists)
    ));
    let index = Index::open(dir.path()).unwrap();
    assert!(matches!(
        IndexBuilder::append(dir.path()),
        Err(Error::IndexBusy)
    ));
    drop(index);
    let builder = IndexBuilder::append(dir.path()).unwrap();
    assert!(matches!(Index::open(dir.path()), Err(Error::IndexBusy)));
    drop(builder);
    let (ops, publication) = trace(true);
    let fs = Arc::new(FaultyFs::new(Some(publication + 2), Fault::Error));
    let mut builder = IndexBuilder::append_with_fs(dir.path(), fs).unwrap();
    builder.push(&[1.0; 768]).unwrap();
    // Place an otherwise unreachable library-owned artifact after startup
    // cleanup so the post-publication cleanup failure leaves observable work.
    let leftover = dir.path().join(format!("model-{}.bin", "0".repeat(64)));
    std::fs::write(&leftover, "leftover").unwrap();
    let report = builder.commit().unwrap();
    assert!(!report.cleanup_complete(), "trace {ops:?}");
    assert!(leftover.exists());
    let builder = IndexBuilder::append(dir.path()).unwrap();
    assert!(!leftover.exists());
    drop(builder);
}
#[test]
#[ignore = "Task 9 release qualification: stage a full segment to poison push"]
fn poisoned_builder_preserves_the_original_failure_and_publishes_nothing() {
    let baseline = tempfile::tempdir().unwrap();
    let fs = Arc::new(FaultyFs::new(None, Fault::Error));
    let builder =
        IndexBuilder::create_with_fs(baseline.path(), &training(), options(), fs.clone()).unwrap();
    let before_stage = fs.calls();
    drop(builder);
    for (offset, fault) in [(1, Fault::Error), (3, Fault::CorruptRead)] {
        let dir = tempfile::tempdir().unwrap();
        let fs = Arc::new(FaultyFs::new(Some(before_stage + offset), fault));
        let mut builder =
            IndexBuilder::create_with_fs(dir.path(), &training(), options(), fs.clone()).unwrap();
        for _ in 0..65535 {
            builder.push(&[1.0; 768]).unwrap();
        }
        let original = builder.push(&[1.0; 768]).unwrap_err();
        assert!(if offset == 1 {
            matches!(original, Error::Io(_))
        } else {
            matches!(original, Error::Corrupt)
        });
        let calls = fs.calls();
        let repeated = builder.push(&[1.0; 768]).unwrap_err();
        assert_eq!(
            std::mem::discriminant(&original),
            std::mem::discriminant(&repeated)
        );
        if let (Error::Io(a), Error::Io(b)) = (&original, &repeated) {
            assert!(Arc::ptr_eq(a, b));
        }
        let repeated = builder.commit().unwrap_err();
        assert_eq!(
            std::mem::discriminant(&original),
            std::mem::discriminant(&repeated)
        );
        if let (Error::Io(a), Error::Io(b)) = (&original, &repeated) {
            assert!(Arc::ptr_eq(a, b));
        }
        assert_eq!(fs.calls(), calls);
        assert_eq!(visible(dir.path()), None);
        attempt(dir.path(), false, Arc::new(RealFs)).unwrap();
    }
}

#[test]
fn segment_and_row_limits_publish_nothing() {
    let dir = tempfile::tempdir().unwrap();
    attempt(dir.path(), false, Arc::new(RealFs)).unwrap();
    let before = std::fs::read(dir.path().join("CURRENT")).unwrap();
    let mut builder = IndexBuilder::append(dir.path()).unwrap();
    let entry = builder.manifest.segments[0].clone();
    // A bounded in-memory fixture represents the valid boundary without
    // duplicating 4,096 identical model-table files on disk.
    builder.manifest.segments = (0..4096)
        .map(|i| SegmentEntry {
            id: (i as u128).to_le_bytes(),
            first_row: i,
            ..entry.clone()
        })
        .collect();
    builder.first_row = 4096;
    assert!(matches!(
        builder.push(&[1.0; 768]),
        Err(Error::SegmentLimit)
    ));
    drop(builder);
    let mut builder = IndexBuilder::append(dir.path()).unwrap();
    builder.first_row = MAX_ROWS;
    assert!(matches!(builder.push(&[1.0; 768]), Err(Error::RowLimit)));
    drop(builder);
    assert_eq!(std::fs::read(dir.path().join("CURRENT")).unwrap(), before);
}
#[test]
fn crash_child() {
    let Ok(path) = std::env::var("SPHERRA_CRASH_INDEX") else {
        return;
    };
    let at = std::env::var("SPHERRA_CRASH_CALL")
        .unwrap()
        .parse()
        .unwrap();
    let append = std::env::var("SPHERRA_CRASH_APPEND").unwrap() == "true";
    let fault = if std::env::var_os("SPHERRA_CRASH_READY").is_some() {
        Fault::Pause
    } else {
        Fault::Abort
    };
    let _ = attempt(
        Path::new(&path),
        append,
        Arc::new(FaultyFs::new(Some(at), fault)),
    );
    panic!("child did not stop at injected call");
}
#[test]
#[ignore = "Task 9 release qualification: deterministic process kills and aborts"]
fn process_kills_and_aborts_leave_only_whole_generations() {
    use std::os::unix::process::ExitStatusExt;
    for append in [false, true] {
        let (ops, publication) = trace(append);
        let first_write = ops.iter().position(|op| *op == "write").unwrap() + 1;
        for at in [first_write, publication, publication + 1] {
            let temp = tempfile::tempdir().unwrap();
            let dir = temp.path().join("index");
            setup(&dir, append);
            let ready = temp.path().join("ready");
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "builder::recovery_tests::crash_child",
                    "--nocapture",
                ])
                .env("SPHERRA_CRASH_INDEX", &dir)
                .env("SPHERRA_CRASH_CALL", at.to_string())
                .env("SPHERRA_CRASH_APPEND", append.to_string())
                .env("SPHERRA_CRASH_READY", &ready)
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while !ready.exists() {
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "child exited before pause"
                );
                if std::time::Instant::now() > deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("child did not reach filesystem call")
                };
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            child.kill().unwrap();
            assert_eq!(child.wait().unwrap().signal(), Some(9));
            let expected = if at > publication {
                Some(if append { (2, 2) } else { (1, 1) })
            } else {
                append.then_some((1, 1))
            };
            assert_eq!(visible(&dir), expected);
            attempt(&dir, expected.is_some(), Arc::new(RealFs)).unwrap();
        }
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("index");
        setup(&dir, append);
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "builder::recovery_tests::crash_child",
                "--nocapture",
            ])
            .env("SPHERRA_CRASH_INDEX", &dir)
            .env("SPHERRA_CRASH_CALL", (publication + 1).to_string())
            .env("SPHERRA_CRASH_APPEND", append.to_string())
            .status()
            .unwrap();
        assert_eq!(status.signal(), Some(6));
        assert_eq!(visible(&dir), Some(if append { (2, 2) } else { (1, 1) }));
    }
}
