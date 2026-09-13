//! Local embedded vector index foundation.
#![deny(unsafe_code)]

// Foundation internals become reachable through the public builder/open API in
// Tasks 7–8. Keep this temporary allowance local to those internal modules.
#[allow(dead_code)]
mod container;
#[allow(dead_code)]
mod fs;
#[allow(dead_code)]
mod lock;
#[allow(dead_code)]
mod manifest;
#[allow(dead_code)]
mod model;

/// Decoder entry points available only to the isolated fuzz workspace.
#[cfg(feature = "fuzzing")]
pub mod fuzzing {
    fn exercise(bytes: &[u8], decode: impl Fn(&[u8])) {
        decode(bytes);
        // Also repair only the outer checksum so mutations reach the bounded
        // payload decoders instead of always stopping at the integrity check.
        if bytes.len() >= 50 && bytes.len() <= crate::container::MAX_CONTAINER_PAYLOAD + 50 {
            let mut repaired = bytes.to_vec();
            let end = repaired.len() - 32;
            let hash = *blake3::hash(&repaired[..end]).as_bytes();
            repaired[end..].copy_from_slice(&hash);
            decode(&repaired);
        }
    }
    pub fn current(bytes: &[u8]) {
        exercise(bytes, |b| {
            let _ = crate::container::Current::decode(b);
        });
    }
    pub fn model(bytes: &[u8]) {
        exercise(bytes, |b| {
            let _ = crate::model::ModelFile::decode(b);
        });
    }
    pub fn manifest(bytes: &[u8]) {
        exercise(bytes, |b| {
            if let Ok(m) = crate::manifest::Manifest::decode(b) {
                let _ = m.validate(m.generation);
            }
        });
    }
}

#[cfg(test)]
mod container_tests;
#[cfg(test)]
mod fs_tests;
#[cfg(test)]
mod lock_tests;
