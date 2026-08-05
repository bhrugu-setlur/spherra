#![deny(unsafe_code)]

pub mod int4;
mod pq96;
mod spec;
mod tiled_soa;
mod transform;

pub use int4::{DirectCode, DirectCodeError, QuantizerTable, RadiusFlags};
pub use pq96::{
    CodecError, Pq96Code, Pq96Codebook, Pq96TrainingDiagnostics, PreparedCandidate, PreparedQuery,
    PrimaryCodes, PrimaryScore, ResidualCodes, ResidualVector, rerank_candidates, scan_primary,
};
pub use spec::TransformSpec;
pub use tiled_soa::TiledSoa32;
pub use transform::{TransformPlan, TransformedDirection, inverse_transform, transform};
