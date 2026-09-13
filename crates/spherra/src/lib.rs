//! Local embedded vector index foundation.
#![deny(unsafe_code)]

mod container;
mod fs;
mod lock;
mod manifest;
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

mod builder;
mod drift;
mod error;
mod storage;
pub use builder::{CommitReport, CreateOptions, IndexBuilder, MAX_TRAINING_ROWS, RowId, Vector};
pub use drift::{DriftReport, DriftStatistics};
pub use error::Error;
#[cfg(test)]
mod builder_qualification;
mod certified;
#[cfg(test)]
mod drift_tests;
mod open;
#[cfg(test)]
mod open_tests;
mod search;
pub use certified::{Certificate, SegmentCertificates};
pub use open::Index;
pub use search::{Hit, SearchOptions, SearchResult};
#[cfg(test)]
mod search_qualification;
#[cfg(test)]
mod worker_probe;
