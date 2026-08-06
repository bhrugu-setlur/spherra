//! Explicit durable segment bytes and corruption-safe checked readers.
//!
//! A v1 segment is one sealed, read-only file: a fixed-width little-endian
//! header, a checked section directory, section payloads, and one CRC32C table
//! per independently read data section, all covered by a whole-file BLAKE3
//! identity. Primary and residual files are separate `file_kind`s with separate
//! capability readers; only a validated pairing exposes residual rows.

#![deny(unsafe_code)]

mod error;
mod header;
mod identity;
mod reader;
mod section;
mod writer;

pub use error::FormatError;
pub use header::{
    FileKind, HEADER_LEN, LayoutId, MAJOR_VERSION, MINOR_VERSION, SegmentExpectations,
    SegmentHeader, SegmentIdentity,
};
pub use reader::{PairedSegmentReaders, PrimaryFileReader, ResidualFileReader, SegmentSource};
pub use section::{
    BLOCK_SIZE, DIRECT_CODE_BYTE_LEN, DIRECTORY_ENTRY_LEN, ERROR_CERTIFICATE_BYTE_LEN,
    PQ_CODEBOOK_VALUE_LEN, PQ96_CODE_BYTE_LEN, QUANTIZER_TABLE_VALUE_LEN, ROW_ENTRY_BYTE_LEN,
    SectionKind, TILE_ROWS, tiled_soa32_len,
};
pub use writer::{
    PrimarySegment, ResidualSegment, RowEntry, StagedFile, StoredErrorCertificate,
    encode_primary_segment, encode_residual_segment, stage_primary_segment, stage_residual_segment,
};
