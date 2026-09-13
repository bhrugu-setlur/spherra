use rustix::fs::{FlockOperation, flock};
use std::{
    fs::{File, OpenOptions},
    io,
    path::Path,
};
/// The permanent LOCK inode is never removed. Closing this handle releases the
/// advisory lock; all library readers and builders participate in this protocol.
pub(crate) struct IndexLock {
    _file: File,
}
impl IndexLock {
    pub fn acquire(dir: &Path, exclusive: bool) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join("LOCK"))?;
        let op = if exclusive {
            FlockOperation::NonBlockingLockExclusive
        } else {
            FlockOperation::NonBlockingLockShared
        };
        flock(&file, op).map_err(io::Error::from)?;
        Ok(Self { _file: file })
    }
}
