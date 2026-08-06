//! Reproducible corpora, the exact original-space oracle, machine provenance,
//! and the recorded shape of a measurement.
//!
//! Nothing here participates in serving. This crate exists so that a benchmark
//! number can be reconstructed later from the descriptor, seed, identities, and
//! toolchain it was recorded with.

#![deny(unsafe_code)]

pub mod corpus;
pub mod exact;
pub mod harness;
pub mod machine;
pub mod results;

pub use corpus::{CorpusDescriptor, CorpusError, CorpusSplits};
pub use exact::{ExactOracle, Neighbor, recall_at};
pub use harness::{CodecFormatRun, HarnessError, M1_CODEC_ID, codec_id_hex};
pub use machine::{CacheState, MachineProfile, SourceRevision};
pub use results::{
    CertificateSoakResult, CodecFormatMeasurement, PercentileSummary, SCHEMA_VERSION,
    SchemaViolations, validate_against_schema,
};
