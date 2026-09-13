use std::io;

use crate::header::{FileKind, LayoutId};
use crate::section::SectionKind;

/// Every way a durable segment file can be refused.
///
/// The reader never panics on hostile bytes: each structural fault below is a
/// value, and the private checked-open path returns before any data accessor
/// exists.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FormatError {
    #[error("segment io failed: {0}")]
    Io(#[from] io::Error),

    #[error("file is shorter than the v1 header: {actual} bytes")]
    ShortFile { actual: u64 },

    #[error("wrong segment magic: {actual:02x?}")]
    WrongMagic { actual: [u8; 8] },

    #[error("unsupported major format version {actual}")]
    UnsupportedMajorVersion { actual: u16 },

    #[error("unsupported minor format version {actual}")]
    UnsupportedMinorVersion { actual: u16 },

    #[error("unknown file kind {actual}")]
    UnknownFileKind { actual: u16 },

    #[error("expected a {expected:?} file but found a {actual:?} file")]
    UnexpectedFileKind {
        expected: FileKind,
        actual: FileKind,
    },

    #[error("declared header length {actual} is not the v1 header length")]
    WrongHeaderLength { actual: u32 },

    #[error("declared dimension {actual} is not the supported dimension")]
    WrongDimension { actual: u16 },

    #[error("a segment must declare at least one row")]
    EmptySegment,

    #[error("segment codec identity does not match the expected codec")]
    CodecMismatch,

    #[error("segment scorer version {actual} does not match expected {expected}")]
    ScorerVersionMismatch { expected: u32, actual: u32 },

    #[error("segment transform identity does not match the expected transform")]
    TransformMismatch,

    #[error("segment quantizer identity does not match the expected quantizer")]
    QuantizerMismatch,

    #[error("segment PQ codebook identity does not match the expected codebook")]
    CodebookMismatch,

    #[error("unknown layout {actual}")]
    UnknownLayout { actual: u16 },

    #[error("segment layout {actual:?} does not match expected {expected:?}")]
    LayoutMismatch {
        expected: LayoutId,
        actual: LayoutId,
    },

    #[error("a declared length or offset overflows the addressable file")]
    LengthOverflow,

    #[error("declared payload length {declared} disagrees with the file length {actual}")]
    PayloadLengthMismatch { declared: u64, actual: u64 },

    #[error("section count {actual} is outside the supported range")]
    SectionCountOutOfRange { actual: u16 },

    #[error("the section directory does not lie between the header and the end of the file")]
    DirectoryOutOfFile,

    #[error("section {index} ends outside the file")]
    SectionOutsideFile { index: u16 },

    #[error("section {index} overlaps the previous section or the directory")]
    OverlappingSections { index: u16 },

    #[error("section {index} starts before the previous section")]
    NonMonotonicDirectory { index: u16 },

    #[error("section {index} declares unsupported alignment {alignment}")]
    InvalidAlignment { index: u16, alignment: u32 },

    #[error("section {index} offset {offset} violates its declared alignment {alignment}")]
    MisalignedSection {
        index: u16,
        offset: u64,
        alignment: u32,
    },

    #[error("section {index} reserves a non-zero value in a reserved field")]
    ReservedFieldNotZero { index: u16 },

    #[error("section {index} declares unknown flags {flags:#06x}")]
    UnknownSectionFlags { index: u16, flags: u16 },

    #[error("unknown section kind {kind} at directory index {index}")]
    UnknownSectionKind { index: u16, kind: u16 },

    #[error("section kind {kind:?} appears more than once")]
    DuplicateSection { kind: SectionKind },

    #[error("required section kind {kind:?} is missing")]
    MissingSection { kind: SectionKind },

    #[error("section kind {kind:?} at index {index} is forbidden in this file kind")]
    ForbiddenSection { index: u16, kind: SectionKind },

    #[error("section {index} declares {actual} logical rows but the header declares {expected}")]
    SectionRowCountMismatch {
        index: u16,
        expected: u32,
        actual: u32,
    },

    #[error("section {index} of kind {kind:?} has length {actual}, expected {expected}")]
    SectionLengthMismatch {
        index: u16,
        kind: SectionKind,
        expected: u64,
        actual: u64,
    },

    #[error("section {index} declares an unusable block size")]
    InvalidBlockSize { index: u16 },

    #[error("section {index} references a CRC table that is not a CRC-table section")]
    InvalidCrcTableReference { index: u16 },

    #[error("data section {index} has no paired CRC table")]
    MissingCrcTable { index: u16 },

    #[error("CRC table {index} is referenced by more than one data section, or by none")]
    CrcTableAliased { index: u16 },

    #[error("block {block} of section {index} fails its CRC32C")]
    BlockChecksumMismatch { index: u16, block: u64 },

    #[error("section {index} does not match its recorded BLAKE3 identity")]
    SectionIdentityMismatch { index: u16 },

    #[error("the file does not match its recorded whole-file BLAKE3 identity")]
    FileIdentityMismatch,

    #[error("row {row} is outside the segment's {row_count} rows")]
    RowOutOfRange { row: u32, row_count: u32 },

    #[error("tile {tile} is outside the segment's {tile_count} tiles")]
    TileOutOfRange { tile: u32, tile_count: u32 },

    #[error("a stored {value} is not a usable finite non-negative value")]
    InvalidStoredValue { value: &'static str },

    #[error("paired files belong to different collections")]
    CollectionMismatch,

    #[error("paired files belong to different segments")]
    SegmentMismatch,

    #[error("paired files declare different row counts: {primary} and {residual}")]
    RowCountMismatch { primary: u32, residual: u32 },

    #[error("paired files declare different representation or scorer identities")]
    PairedIdentityMismatch,

    #[error("a segment cannot declare more rows than the format addresses")]
    TooManyRows,
}

impl PartialEq for FormatError {
    /// Compares the classification and its structural fields. Two `Io` faults
    /// are never equal: an `io::Error` carries no comparable identity.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Io(_), _) | (_, Self::Io(_)) => false,
            _ => format!("{self:?}") == format!("{other:?}"),
        }
    }
}
