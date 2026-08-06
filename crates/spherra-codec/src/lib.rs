#![deny(unsafe_code)]

mod certificate;
pub mod int4;
mod pq96;
mod scorer;
mod spec;
mod tiled_soa;
mod transform;

pub use certificate::{
    BlockCertificate, CertificateBlockCandidate, CertificateBlockId, CertificateError,
    CertificateRow, CertifiedBlockScore, ErrorCertificate, ExhaustiveBlock, ScoreBounds,
    build_exhaustive_certificate,
};
pub use int4::{DirectCode, DirectCodeError, QuantizerTable, RadiusFlags};
pub use pq96::{
    CodecError, Pq96Code, Pq96Codebook, Pq96TrainingDiagnostics, PreparedCandidate, PreparedQuery,
    PrimaryCodes, PrimaryScore, ResidualCodes, ResidualVector, rerank_candidates, scan_primary,
};
pub use scorer::{
    FixedPointScore, FixedPointScorer, LookupScaleMeasurement, LookupTable, PreparedScorerQuery,
    ScoreKind, ScoreProvenance, ScorerError, ScorerMetadata, dot_f64, normalize_fp64,
};
pub use spec::TransformSpec;
pub use tiled_soa::TiledSoa32;
pub use transform::{TransformPlan, TransformedDirection, inverse_transform, transform};
