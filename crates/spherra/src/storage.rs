use crate::{
    Error,
    container::Current,
    fs::{FileSystem, write_all},
    manifest::Manifest,
    model::{Model, ModelFile},
};
use std::{
    collections::HashSet,
    io,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static NONCE: AtomicU64 = AtomicU64::new(0);
pub(crate) fn unique_id() -> [u8; 16] {
    let mut h = blake3::Hasher::new();
    h.update(&std::process::id().to_le_bytes());
    h.update(&NONCE.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    h.update(
        &std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
            .to_le_bytes(),
    );
    h.finalize().as_bytes()[..16].try_into().expect("16 bytes")
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub(crate) fn named(kind: &str, hash: &[u8; 32]) -> String {
    format!("{kind}-{}.bin", hex(hash))
}
pub(crate) fn current(fs: &dyn FileSystem, dir: &Path) -> Result<Current, Error> {
    let bytes = fs
        .read(&dir.join("CURRENT"), Current::BYTE_LEN)
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                Error::NotFound
            } else {
                e.into()
            }
        })?;
    Ok(Current::decode(&bytes)?)
}
pub(crate) fn manifest(
    fs: &dyn FileSystem,
    dir: &Path,
    hash: &[u8; 32],
) -> Result<Manifest, Error> {
    let bytes = fs.read(&dir.join(named("manifest", hash)), Manifest::MAX_BYTE_LEN)?;
    if blake3::hash(&bytes).as_bytes() != hash {
        return Err(Error::IdentityMismatch);
    }
    Ok(Manifest::decode(&bytes)?)
}
pub(crate) fn load(fs: &dyn FileSystem, dir: &Path) -> Result<(Current, Manifest, Model), Error> {
    let current = current(fs, dir)?;
    let manifest = manifest(fs, dir, &current.manifest_hash)?;
    if manifest.generation != current.generation {
        return Err(Error::Corrupt);
    }
    let bytes = fs.read(
        &dir.join(named("model", &manifest.model_hash)),
        ModelFile::BYTE_LEN,
    )?;
    if *blake3::hash(&bytes).as_bytes() != manifest.model_hash {
        return Err(Error::IdentityMismatch);
    }
    let model = Model::restore(ModelFile::decode(&bytes)?)?;
    manifest
        .validate(current.generation)
        .map_err(|_| Error::Corrupt)?;
    Ok((current, manifest, model))
}
/// New files are verified after reopening, before any final name becomes visible.
pub(crate) fn stage(
    fs: &dyn FileSystem,
    dir: &Path,
    name: &str,
    bytes: &[u8],
    verify: impl FnOnce(Vec<u8>) -> Result<(), Error>,
) -> Result<(), Error> {
    let tmp = dir.join(format!("{}-{}.tmp", name, hex(&unique_id())));
    let mut file = fs.create(&tmp)?;
    write_all(fs, &mut file, bytes)?;
    let read = fs.read(&tmp, bytes.len())?;
    if read != bytes {
        return Err(Error::Corrupt);
    }
    verify(read)?;
    fs.sync(&file)?;
    drop(file);
    fs.rename(&tmp, &dir.join(name))?;
    fs.sync_dir(dir)?;
    Ok(())
}
/// Retain the committed predecessor chain. A staged manifest is not a commit.
/// Unknown user files and the permanent LOCK inode are never cleanup targets.
pub(crate) fn cleanup(
    fs: &dyn FileSystem,
    dir: &Path,
    current: Option<&Current>,
) -> Result<(), Error> {
    let mut keep = HashSet::from(["LOCK".to_owned(), "CURRENT".to_owned()]);
    if let Some(current) = current {
        let mut hash = current.manifest_hash;
        let mut generation = current.generation;
        while hash != [0; 32] {
            let m = manifest(fs, dir, &hash)?;
            m.validate(generation).map_err(|_| Error::Corrupt)?;
            keep.insert(named("manifest", &hash));
            keep.insert(named("model", &m.model_hash));
            for s in &m.segments {
                keep.insert(format!("{}.primary", hex(&s.id)));
                keep.insert(format!("{}.residual", hex(&s.id)));
            }
            if generation == 1 {
                if m.previous != [0; 32] {
                    return Err(Error::Corrupt);
                }
                break;
            }
            if m.previous == [0; 32] {
                return Err(Error::Corrupt);
            }
            generation -= 1;
            hash = m.previous;
        }
    }
    for path in fs.list(dir)? {
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !keep.contains(name) && owned_name(name) {
            fs.remove(&path)?;
        }
    }
    fs.sync_dir(dir)?;
    Ok(())
}
fn owned_name(name: &str) -> bool {
    fn is_hex(s: &str, n: usize) -> bool {
        s.len() == n
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }
    if name == "CURRENT.tmp" {
        return true;
    }
    if let Some(base) = name.strip_suffix(".tmp") {
        return base
            .rsplit_once('-')
            .is_some_and(|(n, id)| is_hex(id, 32) && owned_name(n));
    }
    if let Some(base) = name.strip_suffix(".bin") {
        return base
            .strip_prefix("model-")
            .or_else(|| base.strip_prefix("manifest-"))
            .is_some_and(|h| is_hex(h, 64));
    }
    name.strip_suffix(".primary")
        .or_else(|| name.strip_suffix(".residual"))
        .is_some_and(|h| is_hex(h, 32))
}
