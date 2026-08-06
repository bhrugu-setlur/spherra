//! The explicit canonical v1 segment header.
//!
//! Every field is encoded and decoded one at a time as fixed-width
//! little-endian bytes. No Rust struct is ever serialized by memory layout, so
//! padding, field order, and host endianness cannot leak into durable bytes.

use spherra_domain::DIMENSION;

use crate::error::FormatError;

pub const MAGIC: [u8; 8] = *b"SPHERRA1";
pub const MAJOR_VERSION: u16 = 1;
pub const MINOR_VERSION: u16 = 0;

/// Byte length of the v1 header, equal to the sum of the field widths below.
pub const HEADER_LEN: usize = 240;

const OFFSET_MAGIC: usize = 0;
const OFFSET_MAJOR: usize = 8;
const OFFSET_MINOR: usize = 10;
const OFFSET_FILE_KIND: usize = 12;
const OFFSET_HEADER_LEN: usize = 14;
const OFFSET_COLLECTION_ID: usize = 18;
const OFFSET_SEGMENT_ID: usize = 34;
const OFFSET_DIMENSION: usize = 50;
const OFFSET_ROW_COUNT: usize = 52;
const OFFSET_CODEC_ID: usize = 56;
const OFFSET_SCORER_VERSION: usize = 88;
const OFFSET_TRANSFORM_ID: usize = 92;
const OFFSET_QUANTIZER_ID: usize = 124;
const OFFSET_PQ_CODEBOOK_ID: usize = 156;
const OFFSET_LAYOUT_ID: usize = 188;
const OFFSET_SECTION_COUNT: usize = 190;
const OFFSET_DIRECTORY_OFFSET: usize = 192;
const OFFSET_PAYLOAD_LEN: usize = 200;

/// Offset of the whole-file identity, which is zeroed while that identity is
/// computed and verified.
pub const OFFSET_WHOLE_FILE_BLAKE3: usize = 208;
pub const IDENTITY_LEN: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileKind {
    Primary,
    Residual,
}

impl FileKind {
    const PRIMARY: u16 = 1;
    const RESIDUAL: u16 = 2;

    const fn to_u16(self) -> u16 {
        match self {
            Self::Primary => Self::PRIMARY,
            Self::Residual => Self::RESIDUAL,
        }
    }

    fn from_u16(value: u16) -> Result<Self, FormatError> {
        match value {
            Self::PRIMARY => Ok(Self::Primary),
            Self::RESIDUAL => Ok(Self::Residual),
            actual => Err(FormatError::UnknownFileKind { actual }),
        }
    }
}

/// Durable identifier for the physical arrangement of primary codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutId {
    TiledSoa32,
}

impl LayoutId {
    const TILED_SOA_32: u16 = 1;

    const fn to_u16(self) -> u16 {
        match self {
            Self::TiledSoa32 => Self::TILED_SOA_32,
        }
    }

    fn from_u16(value: u16) -> Result<Self, FormatError> {
        match value {
            Self::TILED_SOA_32 => Ok(Self::TiledSoa32),
            actual => Err(FormatError::UnknownLayout { actual }),
        }
    }
}

/// The durable identity a segment claims for itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentIdentity {
    pub collection_id: [u8; 16],
    pub segment_id: [u8; 16],
    pub codec_id: [u8; IDENTITY_LEN],
    pub scorer_version: u32,
    pub transform_id: [u8; IDENTITY_LEN],
    pub quantizer_id: [u8; IDENTITY_LEN],
    pub pq_codebook_id: [u8; IDENTITY_LEN],
    pub layout: LayoutId,
}

/// The representation the caller is prepared to read.
///
/// `spherra-format` cannot derive a codec, transform, quantizer, or codebook
/// identity by itself — the codec owns those. The caller supplies the ones it
/// holds, and the checked reader refuses a file that claims anything else.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentExpectations {
    pub codec_id: [u8; IDENTITY_LEN],
    pub scorer_version: u32,
    pub transform_id: [u8; IDENTITY_LEN],
    pub quantizer_id: [u8; IDENTITY_LEN],
    pub pq_codebook_id: [u8; IDENTITY_LEN],
    pub layout: LayoutId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentHeader {
    pub file_kind: FileKind,
    pub identity: SegmentIdentity,
    pub row_count: u32,
    pub section_count: u16,
    pub section_directory_offset: u64,
    pub payload_len: u64,
    pub whole_file_blake3: [u8; IDENTITY_LEN],
}

impl SegmentHeader {
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut bytes = [0; HEADER_LEN];
        put(&mut bytes, OFFSET_MAGIC, &MAGIC);
        put(&mut bytes, OFFSET_MAJOR, &MAJOR_VERSION.to_le_bytes());
        put(&mut bytes, OFFSET_MINOR, &MINOR_VERSION.to_le_bytes());
        put(
            &mut bytes,
            OFFSET_FILE_KIND,
            &self.file_kind.to_u16().to_le_bytes(),
        );
        put(
            &mut bytes,
            OFFSET_HEADER_LEN,
            &(HEADER_LEN as u32).to_le_bytes(),
        );
        put(
            &mut bytes,
            OFFSET_COLLECTION_ID,
            &self.identity.collection_id,
        );
        put(&mut bytes, OFFSET_SEGMENT_ID, &self.identity.segment_id);
        put(
            &mut bytes,
            OFFSET_DIMENSION,
            &(DIMENSION as u16).to_le_bytes(),
        );
        put(&mut bytes, OFFSET_ROW_COUNT, &self.row_count.to_le_bytes());
        put(&mut bytes, OFFSET_CODEC_ID, &self.identity.codec_id);
        put(
            &mut bytes,
            OFFSET_SCORER_VERSION,
            &self.identity.scorer_version.to_le_bytes(),
        );
        put(&mut bytes, OFFSET_TRANSFORM_ID, &self.identity.transform_id);
        put(&mut bytes, OFFSET_QUANTIZER_ID, &self.identity.quantizer_id);
        put(
            &mut bytes,
            OFFSET_PQ_CODEBOOK_ID,
            &self.identity.pq_codebook_id,
        );
        put(
            &mut bytes,
            OFFSET_LAYOUT_ID,
            &self.identity.layout.to_u16().to_le_bytes(),
        );
        put(
            &mut bytes,
            OFFSET_SECTION_COUNT,
            &self.section_count.to_le_bytes(),
        );
        put(
            &mut bytes,
            OFFSET_DIRECTORY_OFFSET,
            &self.section_directory_offset.to_le_bytes(),
        );
        put(
            &mut bytes,
            OFFSET_PAYLOAD_LEN,
            &self.payload_len.to_le_bytes(),
        );
        put(
            &mut bytes,
            OFFSET_WHOLE_FILE_BLAKE3,
            &self.whole_file_blake3,
        );
        bytes
    }

    /// Decodes and validates every self-describing header field.
    ///
    /// Identity expectations, directory geometry, and checksums are validated
    /// by the reader; this function establishes only that the bytes describe a
    /// v1 segment of a known kind, dimension, and layout.
    pub fn decode(bytes: &[u8; HEADER_LEN]) -> Result<Self, FormatError> {
        let mut magic = [0; 8];
        magic.copy_from_slice(&bytes[OFFSET_MAGIC..OFFSET_MAGIC + 8]);
        if magic != MAGIC {
            return Err(FormatError::WrongMagic { actual: magic });
        }

        let major = u16_at(bytes, OFFSET_MAJOR);
        if major != MAJOR_VERSION {
            return Err(FormatError::UnsupportedMajorVersion { actual: major });
        }
        let minor = u16_at(bytes, OFFSET_MINOR);
        if minor > MINOR_VERSION {
            return Err(FormatError::UnsupportedMinorVersion { actual: minor });
        }

        let file_kind = FileKind::from_u16(u16_at(bytes, OFFSET_FILE_KIND))?;

        let header_len = u32_at(bytes, OFFSET_HEADER_LEN);
        if header_len as usize != HEADER_LEN {
            return Err(FormatError::WrongHeaderLength { actual: header_len });
        }

        let dimension = u16_at(bytes, OFFSET_DIMENSION);
        if dimension as usize != DIMENSION {
            return Err(FormatError::WrongDimension { actual: dimension });
        }

        let row_count = u32_at(bytes, OFFSET_ROW_COUNT);
        if row_count == 0 {
            return Err(FormatError::EmptySegment);
        }

        Ok(Self {
            file_kind,
            identity: SegmentIdentity {
                collection_id: array16(bytes, OFFSET_COLLECTION_ID),
                segment_id: array16(bytes, OFFSET_SEGMENT_ID),
                codec_id: array32(bytes, OFFSET_CODEC_ID),
                scorer_version: u32_at(bytes, OFFSET_SCORER_VERSION),
                transform_id: array32(bytes, OFFSET_TRANSFORM_ID),
                quantizer_id: array32(bytes, OFFSET_QUANTIZER_ID),
                pq_codebook_id: array32(bytes, OFFSET_PQ_CODEBOOK_ID),
                layout: LayoutId::from_u16(u16_at(bytes, OFFSET_LAYOUT_ID))?,
            },
            row_count,
            section_count: u16_at(bytes, OFFSET_SECTION_COUNT),
            section_directory_offset: u64_at(bytes, OFFSET_DIRECTORY_OFFSET),
            payload_len: u64_at(bytes, OFFSET_PAYLOAD_LEN),
            whole_file_blake3: array32(bytes, OFFSET_WHOLE_FILE_BLAKE3),
        })
    }

    /// Refuses a file whose representation is not the one the caller holds.
    pub fn check_expectations(&self, expected: &SegmentExpectations) -> Result<(), FormatError> {
        if self.identity.codec_id != expected.codec_id {
            return Err(FormatError::CodecMismatch);
        }
        if self.identity.scorer_version != expected.scorer_version {
            return Err(FormatError::ScorerVersionMismatch {
                expected: expected.scorer_version,
                actual: self.identity.scorer_version,
            });
        }
        if self.identity.transform_id != expected.transform_id {
            return Err(FormatError::TransformMismatch);
        }
        if self.identity.quantizer_id != expected.quantizer_id {
            return Err(FormatError::QuantizerMismatch);
        }
        if self.identity.pq_codebook_id != expected.pq_codebook_id {
            return Err(FormatError::CodebookMismatch);
        }
        if self.identity.layout != expected.layout {
            return Err(FormatError::LayoutMismatch {
                expected: expected.layout,
                actual: self.identity.layout,
            });
        }
        Ok(())
    }
}

fn put(bytes: &mut [u8; HEADER_LEN], offset: usize, value: &[u8]) {
    bytes[offset..offset + value.len()].copy_from_slice(value);
}

// Every call site below uses a constant offset whose field ends inside the
// fixed-size header array, so these accessors cannot index out of bounds.
fn u16_at(bytes: &[u8; HEADER_LEN], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn u32_at(bytes: &[u8; HEADER_LEN], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn u64_at(bytes: &[u8; HEADER_LEN], offset: usize) -> u64 {
    let mut value = [0; 8];
    value.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(value)
}

fn array16(bytes: &[u8; HEADER_LEN], offset: usize) -> [u8; 16] {
    let mut value = [0; 16];
    value.copy_from_slice(&bytes[offset..offset + 16]);
    value
}

fn array32(bytes: &[u8; HEADER_LEN], offset: usize) -> [u8; IDENTITY_LEN] {
    let mut value = [0; IDENTITY_LEN];
    value.copy_from_slice(&bytes[offset..offset + IDENTITY_LEN]);
    value
}

#[cfg(test)]
mod tests {
    use super::{HEADER_LEN, OFFSET_PAYLOAD_LEN, OFFSET_WHOLE_FILE_BLAKE3};

    #[test]
    fn the_header_length_is_exactly_the_sum_of_its_declared_fields() {
        assert_eq!(OFFSET_PAYLOAD_LEN + 8, OFFSET_WHOLE_FILE_BLAKE3);
        assert_eq!(OFFSET_WHOLE_FILE_BLAKE3 + 32, HEADER_LEN);
    }
}
