use crate::lock::IndexLock;
use std::{path::Path, process::Command};

#[test]
fn lock_child() {
    let Some(path) = std::env::var_os("SPHERRA_LOCK_TEST_DIR") else {
        return;
    };
    let exclusive = std::env::var("SPHERRA_LOCK_TEST_EXCLUSIVE").unwrap() == "yes";
    let expected = std::env::var("SPHERRA_LOCK_TEST_SUCCESS").unwrap() == "yes";
    let result = IndexLock::acquire(Path::new(&path), exclusive);
    assert_eq!(result.is_ok(), expected);
    if !expected {
        assert_eq!(result.err().unwrap().kind(), std::io::ErrorKind::WouldBlock);
    }
}
#[test]
fn shared_and_exclusive_locks_exclude_other_processes() {
    let dir = tempfile::tempdir().unwrap();
    let child = |exclusive: bool, success: bool| {
        assert!(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "lock_tests::lock_child", "--nocapture"])
                .env("SPHERRA_LOCK_TEST_DIR", dir.path())
                .env(
                    "SPHERRA_LOCK_TEST_EXCLUSIVE",
                    if exclusive { "yes" } else { "no" }
                )
                .env(
                    "SPHERRA_LOCK_TEST_SUCCESS",
                    if success { "yes" } else { "no" }
                )
                .status()
                .unwrap()
                .success()
        );
    };
    let shared = IndexLock::acquire(dir.path(), false).unwrap();
    child(false, true);
    child(true, false);
    drop(shared);
    let exclusive = IndexLock::acquire(dir.path(), true).unwrap();
    child(false, false);
    child(true, false);
    drop(exclusive);
    child(true, true);
    assert!(dir.path().join("LOCK").is_file());
}
