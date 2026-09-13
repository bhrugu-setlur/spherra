use crate::fs::{Fault, FaultyFs, FileSystem, RealFs, write_all};
#[test]
fn real_filesystem_stages_verifies_syncs_renames_and_removes() {
    let dir = tempfile::tempdir().unwrap();
    let fs = RealFs;
    let path = dir.path().join("test.tmp");
    let final_path = dir.path().join("final.bin");
    let mut file = fs.create(&path).unwrap();
    write_all(&fs, &mut file, b"complete file").unwrap();
    assert_eq!(fs.read(&path, 13).unwrap(), b"complete file");
    assert!(fs.read(&path, 12).is_err());
    fs.sync(&file).unwrap();
    fs.rename(&path, &final_path).unwrap();
    fs.sync_dir(dir.path()).unwrap();
    assert_eq!(fs.list(dir.path()).unwrap(), vec![final_path.clone()]);
    fs.remove(&final_path).unwrap();
    assert!(fs.list(dir.path()).unwrap().is_empty());
}
#[test]
fn partial_writes_are_completed_and_injected_errors_surface() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("partial");
    let fs = FaultyFs::new(Some(2), Fault::ShortWrite);
    let mut file = fs.create(&path).unwrap();
    write_all(&fs, &mut file, b"complete file").unwrap();
    assert_eq!(fs.read(&path, 13).unwrap(), b"complete file");
    assert_eq!(fs.calls(), 4); // create, two writes, read
    let fs = FaultyFs::new(Some(1), Fault::Error);
    assert!(fs.read(&path, 13).is_err());
    assert_eq!(fs.calls(), 1);
}
