//! The explicit v1 section directory.
//!
//! Each entry describes one durable byte range: what it holds, how it is
//! aligned, how many logical rows it covers, how it is blocked for CRC32C, and
//! which CRC-table section carries those checksums.

use spherra_domain::DIMENSION;

use crate::error::FormatError;
use crate::header::{FileKind, IDENTITY_LEN};

pub const DIRECTORY_ENTRY_LEN: usize = 72;

/// The most sections a v1 segment may declare. Both file kinds need far fewer;
/// the cap keeps a hostile `section_count` from forcing a large directory read.
pub const MAX_SECTIONS: u16 = 64;

/// Alignment the writer gives every section, and the largest one a reader
/// accepts.
pub const SECTION_ALIGNMENT: u32 = 64;
pub const MAX_SECTION_ALIGNMENT: u32 = 4096;

/// Bytes covered by one CRC32C entry. The final block of a section is
/// checksummed over its logical bytes only.
pub const BLOCK_SIZE: u32 = 4096;

/// `crc_table_index` value used by CRC-table sections, which are covered by the
/// whole-file BLAKE3 identity rather than by another CRC table.
pub const NO_CRC_TABLE: u16 = u16::MAX;

/// Logical bytes of one direct-int4 primary code: 768 nibbles.
pub const DIRECT_CODE_BYTE_LEN: usize = DIMENSION / 2;
/// Logical bytes of one PQ96x8 residual code.
pub const PQ96_CODE_BYTE_LEN: usize = 96;
/// FP32 centers in one direct-int4 quantizer table: 16 per coordinate.
pub const QUANTIZER_TABLE_VALUE_LEN: usize = DIMENSION * 16;
/// FP32 values in one PQ96x8 codebook: 96 subquantizers of 256 eight-dimensional centroids.
pub const PQ_CODEBOOK_VALUE_LEN: usize = 96 * 256 * 8;
/// Durable bytes of one stored error certificate: five FP64 fields.
pub const ERROR_CERTIFICATE_BYTE_LEN: usize = 40;
/// Durable bytes of one row's chunk id, document id, and put sequence.
pub const ROW_ENTRY_BYTE_LEN: usize = 40;

/// Rows per TILED_SOA_32 tile, and the physical bytes one full tile occupies.
pub const TILE_ROWS: u32 = 32;
const TILE_BYTES: u64 = DIMENSION as u64 * (TILE_ROWS as u64 / 2);

/// Physical byte length of `row_count` primary codes in TILED_SOA_32.
///
/// The final tile is always materialized in full; its unused lanes are zero
/// padding that the logical row count excludes.
pub fn tiled_soa32_len(row_count: u32) -> Result<u64, FormatError> {
    u64::from(row_count.div_ceil(TILE_ROWS))
        .checked_mul(TILE_BYTES)
        .ok_or(FormatError::TooManyRows)
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SectionKind {
    IdsSequences,
    RadiusFlags,
    PrimaryDirectInt4,
    Int4QuantizerTable,
    PrimaryCertificate,
    RefinedCertificate,
    Pq96Residual,
    PqCodebook,
    CrcTable,
}

impl SectionKind {
    pub(crate) const PRIMARY_SECTIONS: [Self; 6] = [
        Self::IdsSequences,
        Self::RadiusFlags,
        Self::PrimaryDirectInt4,
        Self::Int4QuantizerTable,
        Self::PrimaryCertificate,
        Self::RefinedCertificate,
    ];
    pub(crate) const RESIDUAL_SECTIONS: [Self; 2] = [Self::Pq96Residual, Self::PqCodebook];

    const fn to_u16(self) -> u16 {
        match self {
            Self::IdsSequences => 1,
            Self::RadiusFlags => 2,
            Self::PrimaryDirectInt4 => 3,
            Self::Int4QuantizerTable => 4,
            Self::PrimaryCertificate => 5,
            Self::RefinedCertificate => 6,
            Self::Pq96Residual => 7,
            Self::PqCodebook => 8,
            Self::CrcTable => 9,
        }
    }

    fn from_u16(index: u16, value: u16) -> Result<Self, FormatError> {
        match value {
            1 => Ok(Self::IdsSequences),
            2 => Ok(Self::RadiusFlags),
            3 => Ok(Self::PrimaryDirectInt4),
            4 => Ok(Self::Int4QuantizerTable),
            5 => Ok(Self::PrimaryCertificate),
            6 => Ok(Self::RefinedCertificate),
            7 => Ok(Self::Pq96Residual),
            8 => Ok(Self::PqCodebook),
            9 => Ok(Self::CrcTable),
            kind => Err(FormatError::UnknownSectionKind { index, kind }),
        }
    }

    /// Whether this kind is permitted in a file of the given kind.
    pub(crate) fn is_allowed_in(self, file_kind: FileKind) -> bool {
        match self {
            Self::CrcTable => true,
            _ => match file_kind {
                FileKind::Primary => Self::PRIMARY_SECTIONS.contains(&self),
                FileKind::Residual => Self::RESIDUAL_SECTIONS.contains(&self),
            },
        }
    }

    /// Rows this kind stores one entry per, or `None` when it is a whole-file
    /// table, certificate, or CRC table.
    pub(crate) const fn bytes_per_row(self) -> Option<u64> {
        match self {
            Self::IdsSequences => Some(ROW_ENTRY_BYTE_LEN as u64),
            Self::RadiusFlags => Some(4),
            Self::Pq96Residual => Some(PQ96_CODE_BYTE_LEN as u64),
            _ => None,
        }
    }

    /// Byte length this kind must have, given the segment's row count.
    pub(crate) fn expected_len(self, row_count: u32) -> Result<Option<u64>, FormatError> {
        if let Some(bytes_per_row) = self.bytes_per_row() {
            return u64::from(row_count)
                .checked_mul(bytes_per_row)
                .ok_or(FormatError::TooManyRows)
                .map(Some);
        }
        Ok(match self {
            Self::PrimaryDirectInt4 => Some(tiled_soa32_len(row_count)?),
            Self::Int4QuantizerTable => Some(QUANTIZER_TABLE_VALUE_LEN as u64 * 4),
            Self::PqCodebook => Some(PQ_CODEBOOK_VALUE_LEN as u64 * 4),
            Self::PrimaryCertificate | Self::RefinedCertificate => {
                Some(ERROR_CERTIFICATE_BYTE_LEN as u64)
            }
            _ => None,
        })
    }

    /// Whether this kind's directory entry must declare the segment row count.
    pub(crate) fn is_row_addressed(self) -> bool {
        self.bytes_per_row().is_some() || self == Self::PrimaryDirectInt4
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SectionEntry {
    pub kind: SectionKind,
    pub flags: u16,
    pub alignment: u32,
    pub offset: u64,
    pub length: u64,
    pub logical_row_count: u32,
    pub block_size: u32,
    pub crc_table_index: u16,
    pub identity: [u8; IDENTITY_LEN],
}

impl SectionEntry {
    pub fn encode(&self) -> [u8; DIRECTORY_ENTRY_LEN] {
        let mut bytes = [0; DIRECTORY_ENTRY_LEN];
        bytes[0..2].copy_from_slice(&self.kind.to_u16().to_le_bytes());
        bytes[2..4].copy_from_slice(&self.flags.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.alignment.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.offset.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.length.to_le_bytes());
        bytes[24..28].copy_from_slice(&self.logical_row_count.to_le_bytes());
        bytes[28..32].copy_from_slice(&self.block_size.to_le_bytes());
        bytes[32..34].copy_from_slice(&self.crc_table_index.to_le_bytes());
        // bytes[34..40] stay zero: two reserved u16/u32 fields.
        bytes[40..72].copy_from_slice(&self.identity);
        bytes
    }

    /// Decodes one directory entry, rejecting unknown kinds, unknown flags, and
    /// any reserved field a future version might have claimed.
    pub fn decode(index: u16, bytes: &[u8; DIRECTORY_ENTRY_LEN]) -> Result<Self, FormatError> {
        let flags = u16::from_le_bytes([bytes[2], bytes[3]]);
        if flags != 0 {
            return Err(FormatError::UnknownSectionFlags { index, flags });
        }
        if bytes[34..40].iter().any(|reserved| *reserved != 0) {
            return Err(FormatError::ReservedFieldNotZero { index });
        }

        let mut identity = [0; IDENTITY_LEN];
        identity.copy_from_slice(&bytes[40..72]);
        Ok(Self {
            kind: SectionKind::from_u16(index, u16::from_le_bytes([bytes[0], bytes[1]]))?,
            flags,
            alignment: u32_at(bytes, 4),
            offset: u64_at(bytes, 8),
            length: u64_at(bytes, 16),
            logical_row_count: u32_at(bytes, 24),
            block_size: u32_at(bytes, 28),
            crc_table_index: u16::from_le_bytes([bytes[32], bytes[33]]),
            identity,
        })
    }

    /// The exclusive end of this section, or an overflow error.
    pub fn end(&self) -> Result<u64, FormatError> {
        self.offset
            .checked_add(self.length)
            .ok_or(FormatError::LengthOverflow)
    }

    /// Number of CRC32C blocks covering this section's bytes.
    pub fn block_count(&self, index: u16) -> Result<u64, FormatError> {
        if self.block_size == 0 {
            return Err(FormatError::InvalidBlockSize { index });
        }
        Ok(self.length.div_ceil(u64::from(self.block_size)))
    }
}

fn u32_at(bytes: &[u8; DIRECTORY_ENTRY_LEN], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn u64_at(bytes: &[u8; DIRECTORY_ENTRY_LEN], offset: usize) -> u64 {
    let mut value = [0; 8];
    value.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(value)
}

#[cfg(test)]
mod tests {
    use super::{TILE_ROWS, tiled_soa32_len};

    #[test]
    fn a_partial_tail_tile_is_still_materialized_in_full() {
        let full_tile = tiled_soa32_len(TILE_ROWS).expect("one full tile");

        assert_eq!(tiled_soa32_len(1).expect("one row"), full_tile);
        assert_eq!(
            tiled_soa32_len(TILE_ROWS + 1).expect("two tiles"),
            full_tile * 2
        );
    }
}
