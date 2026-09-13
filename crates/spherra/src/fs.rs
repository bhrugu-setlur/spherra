use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
pub(crate) trait FileSystem: Send + Sync {
    fn create_dir(&self, path: &Path) -> io::Result<()>;
    fn create(&self, path: &Path) -> io::Result<File>;
    fn write(&self, file: &mut File, bytes: &[u8]) -> io::Result<usize>;
    fn sync(&self, file: &File) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn sync_dir(&self, path: &Path) -> io::Result<()>;
    fn read(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>>;
    fn remove(&self, path: &Path) -> io::Result<()>;
    fn list(&self, path: &Path) -> io::Result<Vec<PathBuf>>;
}
pub(crate) struct RealFs;
impl FileSystem for RealFs {
    fn create_dir(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }
    fn create(&self, path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
    }
    fn write(&self, file: &mut File, bytes: &[u8]) -> io::Result<usize> {
        file.write(bytes)
    }
    fn sync(&self, file: &File) -> io::Result<()> {
        file.sync_all()
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }
    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        File::open(path)?.sync_all()
    }
    fn read(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        let file = File::open(path)?;
        if file.metadata()?.len() > limit as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file exceeds size limit",
            ));
        }
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file grew beyond size limit",
            ));
        }
        Ok(bytes)
    }
    fn remove(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
    fn list(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        fs::read_dir(path)?
            .map(|entry| entry.map(|e| e.path()))
            .collect()
    }
}
pub(crate) fn write_all(fs: &dyn FileSystem, file: &mut File, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        match fs.write(file, bytes) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) enum Fault {
    Error,
    ShortWrite,
    CorruptRead,
    Abort,
    Pause,
}
#[cfg(test)]
pub(crate) struct FaultyFs {
    at: Option<usize>,
    action: Fault,
    count: std::sync::atomic::AtomicUsize,
    log: std::sync::Mutex<Vec<&'static str>>,
}
#[cfg(test)]
impl FaultyFs {
    pub fn new(at: Option<usize>, action: Fault) -> Self {
        Self {
            at,
            action,
            count: std::sync::atomic::AtomicUsize::new(0),
            log: std::sync::Mutex::new(Vec::new()),
        }
    }
    pub fn calls(&self) -> usize {
        self.count.load(std::sync::atomic::Ordering::SeqCst)
    }
    pub fn operations(&self) -> Vec<&'static str> {
        self.log.lock().unwrap().clone()
    }
    fn tick(&self, name: &'static str) -> io::Result<bool> {
        self.log.lock().unwrap().push(name);
        let call = self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        if self.at == Some(call) {
            match self.action {
                Fault::Error => return Err(io::Error::other("injected filesystem failure")),
                Fault::Abort => std::process::abort(),
                Fault::Pause => {
                    // Child-process test rendezvous, outside the index directory.
                    // The parent sends SIGKILL after this numbered call is reached.
                    let ready = std::env::var_os("SPHERRA_CRASH_READY")
                        .expect("pause injection requires a parent rendezvous");
                    fs::write(ready, b"ready")?;
                    loop {
                        std::thread::park();
                    }
                }
                Fault::ShortWrite | Fault::CorruptRead => return Ok(true),
            }
        }
        Ok(false)
    }
}
#[cfg(test)]
impl FileSystem for FaultyFs {
    fn create_dir(&self, p: &Path) -> io::Result<()> {
        self.tick("create_dir")?;
        RealFs.create_dir(p)
    }
    fn create(&self, p: &Path) -> io::Result<File> {
        self.tick("create")?;
        RealFs.create(p)
    }
    fn write(&self, f: &mut File, b: &[u8]) -> io::Result<usize> {
        if self.tick("write")? && matches!(self.action, Fault::ShortWrite) {
            RealFs.write(f, &b[..(b.len() / 2).max(1)])
        } else {
            RealFs.write(f, b)
        }
    }
    fn sync(&self, f: &File) -> io::Result<()> {
        self.tick("sync")?;
        RealFs.sync(f)
    }
    fn rename(&self, a: &Path, b: &Path) -> io::Result<()> {
        self.tick("rename")?;
        RealFs.rename(a, b)
    }
    fn sync_dir(&self, p: &Path) -> io::Result<()> {
        self.tick("sync_dir")?;
        RealFs.sync_dir(p)
    }
    fn read(&self, p: &Path, l: usize) -> io::Result<Vec<u8>> {
        let corrupt = self.tick("read")? && matches!(self.action, Fault::CorruptRead);
        let mut bytes = RealFs.read(p, l)?;
        if corrupt {
            if let Some(first) = bytes.first_mut() {
                *first ^= 1;
            }
        }
        Ok(bytes)
    }
    fn remove(&self, p: &Path) -> io::Result<()> {
        self.tick("remove")?;
        RealFs.remove(p)
    }
    fn list(&self, p: &Path) -> io::Result<Vec<PathBuf>> {
        self.tick("list")?;
        RealFs.list(p)
    }
}
