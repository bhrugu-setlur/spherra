//! The pure v1 segment writer.
//!
//! It produces one deterministic byte sequence from an in-memory segment,
//! records a CRC32C per independently read block, and computes the whole-file
//! BLAKE3 identity with that identity field zeroed. Durable temp/sync/rename
//! and directory-sync belong to the later LSM milestone, not here: staging
//! returns a descriptor for a file the caller then makes durable.

use std::path::{Path, PathBuf};

use spherra_domain::{ChunkId, DocumentId, PutSeq};

use crate::error::FormatError;
use crate::header::{
    FileKind, HEADER_LEN, IDENTITY_LEN, OFFSET_WHOLE_FILE_BLAKE3, SegmentHeader, SegmentIdentity,
};
use crate::section::{
    BLOCK_SIZE, DIRECT_CODE_BYTE_LEN, DIRECTORY_ENTRY_LEN, ERROR_CERTIFICATE_BYTE_LEN,
    NO_CRC_TABLE, PQ_CODEBOOK_VALUE_LEN, PQ96_CODE_BYTE_LEN, QUANTIZER_TABLE_VALUE_LEN,
    ROW_ENTRY_BYTE_LEN, SECTION_ALIGNMENT, SectionEntry, SectionKind, TILE_ROWS, tiled_soa32_len,
};

/// One row's durable primary-key truth.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RowEntry {
    pub chunk_id: ChunkId,
    pub document_id: DocumentId,
    pub put_seq: PutSeq,
}

/// The five certified error terms a block certificate stores per score kind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StoredErrorCertificate {
    pub max_reconstruction_l2_error: f64,
    pub eta_transform_dot: f64,
    pub query_norm_upper: f64,
    pub eta_serving_score: f64,
    pub epsilon: f64,
}

impl StoredErrorCertificate {
    fn encode(&self) -> [u8; ERROR_CERTIFICATE_BYTE_LEN] {
        let mut bytes = [0; ERROR_CERTIFICATE_BYTE_LEN];
        for (slot, value) in bytes.chunks_exact_mut(8).zip(self.fields()) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, FormatError> {
        let mut fields = [0.0; 5];
        for (slot, value) in fields.iter_mut().zip(bytes.chunks_exact(8)) {
            let mut raw = [0; 8];
            raw.copy_from_slice(value);
            *slot = f64::from_le_bytes(raw);
            if !slot.is_finite() || *slot < 0.0 {
                return Err(FormatError::InvalidStoredValue {
                    value: "certificate error term",
                });
            }
        }
        Ok(Self {
            max_reconstruction_l2_error: fields[0],
            eta_transform_dot: fields[1],
            query_norm_upper: fields[2],
            eta_serving_score: fields[3],
            epsilon: fields[4],
        })
    }

    const fn fields(&self) -> [f64; 5] {
        [
            self.max_reconstruction_l2_error,
            self.eta_transform_dot,
            self.query_norm_upper,
            self.eta_serving_score,
            self.epsilon,
        ]
    }
}

/// A complete in-memory primary segment.
#[derive(Clone, Debug, PartialEq)]
pub struct PrimarySegment {
    pub identity: SegmentIdentity,
    pub rows: Vec<RowEntry>,
    pub radius_flags: Vec<[u8; 4]>,
    pub primary_codes: Vec<[u8; DIRECT_CODE_BYTE_LEN]>,
    pub quantizer_table: Vec<f32>,
    pub primary_certificate: StoredErrorCertificate,
    pub refined_certificate: StoredErrorCertificate,
}

/// A complete in-memory residual segment.
#[derive(Clone, Debug, PartialEq)]
pub struct ResidualSegment {
    pub identity: SegmentIdentity,
    pub row_count: u32,
    pub residual_codes: Vec<[u8; PQ96_CODE_BYTE_LEN]>,
    pub pq_codebook: Vec<f32>,
}

/// A written but not yet durable segment file.
///
/// Making the bytes durable — fsync, rename, directory sync — is the later LSM
/// milestone's responsibility.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedFile {
    path: PathBuf,
    len: u64,
    identity: [u8; IDENTITY_LEN],
}

impl StagedFile {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn len(&self) -> u64 {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub const fn identity(&self) -> [u8; IDENTITY_LEN] {
        self.identity
    }
}

pub fn encode_primary_segment(segment: &PrimarySegment) -> Result<Vec<u8>, FormatError> {
    let row_count = check_row_count(segment.rows.len())?;
    if segment.radius_flags.len() != segment.rows.len()
        || segment.primary_codes.len() != segment.rows.len()
    {
        return Err(FormatError::InvalidStoredValue {
            value: "primary segment with unequal per-row column lengths",
        });
    }
    if segment.quantizer_table.len() != QUANTIZER_TABLE_VALUE_LEN {
        return Err(FormatError::InvalidStoredValue {
            value: "quantizer table length",
        });
    }

    let mut ids = Vec::with_capacity(segment.rows.len() * ROW_ENTRY_BYTE_LEN);
    for row in &segment.rows {
        ids.extend_from_slice(&row.chunk_id.as_u128().to_le_bytes());
        ids.extend_from_slice(&row.document_id.as_u128().to_le_bytes());
        ids.extend_from_slice(&row.put_seq.raw().to_le_bytes());
    }

    let radius = segment.radius_flags.concat();
    let codes = encode_tiled_soa32(&segment.primary_codes, row_count)?;
    let quantizer = encode_f32_slice(&segment.quantizer_table)?;

    encode_segment(
        FileKind::Primary,
        segment.identity,
        row_count,
        vec![
            (SectionKind::IdsSequences, ids),
            (SectionKind::RadiusFlags, radius),
            (SectionKind::PrimaryDirectInt4, codes),
            (SectionKind::Int4QuantizerTable, quantizer),
            (
                SectionKind::PrimaryCertificate,
                segment.primary_certificate.encode().to_vec(),
            ),
            (
                SectionKind::RefinedCertificate,
                segment.refined_certificate.encode().to_vec(),
            ),
        ],
    )
}

pub fn encode_residual_segment(segment: &ResidualSegment) -> Result<Vec<u8>, FormatError> {
    let row_count = check_row_count(segment.residual_codes.len())?;
    if row_count != segment.row_count {
        return Err(FormatError::RowCountMismatch {
            primary: segment.row_count,
            residual: row_count,
        });
    }
    if segment.pq_codebook.len() != PQ_CODEBOOK_VALUE_LEN {
        return Err(FormatError::InvalidStoredValue {
            value: "PQ codebook length",
        });
    }

    encode_segment(
        FileKind::Residual,
        segment.identity,
        row_count,
        vec![
            (SectionKind::Pq96Residual, segment.residual_codes.concat()),
            (
                SectionKind::PqCodebook,
                encode_f32_slice(&segment.pq_codebook)?,
            ),
        ],
    )
}

pub fn stage_primary_segment(
    directory: &Path,
    segment: &PrimarySegment,
) -> Result<StagedFile, FormatError> {
    stage(directory, encode_primary_segment(segment)?)
}

pub fn stage_residual_segment(
    directory: &Path,
    segment: &ResidualSegment,
) -> Result<StagedFile, FormatError> {
    stage(directory, encode_residual_segment(segment)?)
}

fn stage(directory: &Path, bytes: Vec<u8>) -> Result<StagedFile, FormatError> {
    use std::io::Write;

    let identity = read_identity(&bytes);
    let mut file = tempfile::Builder::new()
        .prefix("spherra-segment-")
        .suffix(".staged")
        .tempfile_in(directory)?;
    file.write_all(&bytes)?;
    file.flush()?;
    let (_, path) = file.keep().map_err(|error| error.error)?;

    Ok(StagedFile {
        path,
        len: bytes.len() as u64,
        identity,
    })
}

/// Lays out the header, section payloads, CRC tables, and directory, then seals
/// the result with its whole-file BLAKE3 identity.
fn encode_segment(
    file_kind: FileKind,
    identity: SegmentIdentity,
    row_count: u32,
    sections: Vec<(SectionKind, Vec<u8>)>,
) -> Result<Vec<u8>, FormatError> {
    let section_count = sections.len() * 2;
    let directory_len = section_count * DIRECTORY_ENTRY_LEN;
    let mut bytes = vec![0; HEADER_LEN + directory_len];

    // Data sections first, then their CRC tables, so a scan reads contiguous
    // payload bytes without stepping over checksum blocks.
    let crc_tables: Vec<Vec<u8>> = sections
        .iter()
        .map(|(_, payload)| crc_table(payload))
        .collect();

    let mut entries = Vec::with_capacity(section_count);
    for (index, (kind, payload)) in sections.iter().enumerate() {
        let offset = align_to(&mut bytes);
        bytes.extend_from_slice(payload);
        entries.push(SectionEntry {
            kind: *kind,
            flags: 0,
            alignment: SECTION_ALIGNMENT,
            offset,
            length: payload.len() as u64,
            logical_row_count: if kind.is_row_addressed() {
                row_count
            } else {
                0
            },
            block_size: BLOCK_SIZE,
            crc_table_index: (sections.len() + index) as u16,
            identity: *blake3::hash(payload).as_bytes(),
        });
    }
    for table in &crc_tables {
        let offset = align_to(&mut bytes);
        bytes.extend_from_slice(table);
        entries.push(SectionEntry {
            kind: SectionKind::CrcTable,
            flags: 0,
            alignment: SECTION_ALIGNMENT,
            offset,
            length: table.len() as u64,
            logical_row_count: 0,
            block_size: 0,
            crc_table_index: NO_CRC_TABLE,
            identity: *blake3::hash(table).as_bytes(),
        });
    }

    for (index, entry) in entries.iter().enumerate() {
        let start = HEADER_LEN + index * DIRECTORY_ENTRY_LEN;
        bytes[start..start + DIRECTORY_ENTRY_LEN].copy_from_slice(&entry.encode());
    }

    let header = SegmentHeader {
        file_kind,
        identity,
        row_count,
        section_count: section_count as u16,
        section_directory_offset: HEADER_LEN as u64,
        payload_len: (bytes.len() - HEADER_LEN) as u64,
        whole_file_blake3: [0; IDENTITY_LEN],
    };
    bytes[..HEADER_LEN].copy_from_slice(&header.encode());

    let file_identity = crate::identity::whole_file_blake3(&bytes);
    bytes[OFFSET_WHOLE_FILE_BLAKE3..OFFSET_WHOLE_FILE_BLAKE3 + IDENTITY_LEN]
        .copy_from_slice(&file_identity);
    Ok(bytes)
}

/// Pads to the section alignment and returns the offset the next section starts at.
fn align_to(bytes: &mut Vec<u8>) -> u64 {
    let alignment = SECTION_ALIGNMENT as usize;
    let padding = (alignment - bytes.len() % alignment) % alignment;
    bytes.resize(bytes.len() + padding, 0);
    bytes.len() as u64
}

fn crc_table(payload: &[u8]) -> Vec<u8> {
    payload
        .chunks(BLOCK_SIZE as usize)
        .flat_map(|block| crc32c::crc32c(block).to_le_bytes())
        .collect()
}

/// Packs logical direct-int4 codes into TILED_SOA_32.
///
/// Coordinate `c` of row `r` occupies the `r % 32` lane of coordinate `c`
/// within tile `r / 32`; even lanes take the low nibble. The tail tile is
/// materialized in full and its unused lanes stay zero.
fn encode_tiled_soa32(
    codes: &[[u8; DIRECT_CODE_BYTE_LEN]],
    row_count: u32,
) -> Result<Vec<u8>, FormatError> {
    let length =
        usize::try_from(tiled_soa32_len(row_count)?).map_err(|_| FormatError::TooManyRows)?;
    let mut bytes = vec![0; length];
    let tile_bytes = length / row_count.div_ceil(TILE_ROWS) as usize;

    for (row, code) in codes.iter().enumerate() {
        let tile = row / TILE_ROWS as usize;
        let lane = row % TILE_ROWS as usize;
        for (coordinate, nibble) in code.iter().enumerate() {
            // One source byte carries the even coordinate in its low nibble and
            // the odd coordinate in its high nibble.
            for (parity, value) in [nibble & 0x0f, nibble >> 4].into_iter().enumerate() {
                let target = tile * tile_bytes
                    + (coordinate * 2 + parity) * (TILE_ROWS as usize / 2)
                    + lane / 2;
                bytes[target] |= value << if lane.is_multiple_of(2) { 0 } else { 4 };
            }
        }
    }
    Ok(bytes)
}

fn encode_f32_slice(values: &[f32]) -> Result<Vec<u8>, FormatError> {
    if values.iter().any(|value| !value.is_finite()) {
        return Err(FormatError::InvalidStoredValue {
            value: "non-finite FP32 table entry",
        });
    }
    Ok(values
        .iter()
        .flat_map(|value| canonical_zero(*value).to_le_bytes())
        .collect())
}

/// Collapses `-0.0` so an identical table never produces two byte sequences.
fn canonical_zero(value: f32) -> f32 {
    if value == 0.0 { 0.0 } else { value }
}

fn check_row_count(rows: usize) -> Result<u32, FormatError> {
    let row_count = u32::try_from(rows).map_err(|_| FormatError::TooManyRows)?;
    if row_count == 0 {
        return Err(FormatError::EmptySegment);
    }
    tiled_soa32_len(row_count)?;
    Ok(row_count)
}

fn read_identity(bytes: &[u8]) -> [u8; IDENTITY_LEN] {
    let mut identity = [0; IDENTITY_LEN];
    identity
        .copy_from_slice(&bytes[OFFSET_WHOLE_FILE_BLAKE3..OFFSET_WHOLE_FILE_BLAKE3 + IDENTITY_LEN]);
    identity
}
