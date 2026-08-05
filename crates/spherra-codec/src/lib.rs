#![deny(unsafe_code)]

mod pq96;
mod spec;
mod transform;

pub use pq96::{
    CodecError, Pq96Code, Pq96Codebook, Pq96TrainingDiagnostics, PreparedCandidate, PreparedQuery,
    PrimaryCodes, PrimaryScore, ResidualCodes, ResidualVector, rerank_candidates, scan_primary,
};
pub use spec::TransformSpec;
pub use transform::{TransformPlan, TransformedDirection, inverse_transform, transform};
