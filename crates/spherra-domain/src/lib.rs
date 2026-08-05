#![deny(unsafe_code)]

mod error;
mod ids;
mod record;
mod sequence;

pub use error::DomainError;
pub use ids::{ChunkId, DocumentId};
pub use record::{DEFAULT_MIN_NORM_EPSILON, DIMENSION, ReliableDirection, ValidatedVector};
pub use sequence::PutSeq;
