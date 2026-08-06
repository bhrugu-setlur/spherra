//! Deterministic minimal segments plus the byte offsets the v1 header promises.
//!
//! The offsets below are written independently of `spherra-format` on purpose:
//! a durable-format test that reuses the implementation's own field offsets
//! cannot detect a layout change.

// Two test binaries share this module; each uses a subset of the offsets.
#![allow(dead_code)]

use spherra_domain::{ChunkId, DocumentId, PutSeq};
use spherra_format::{
    DIRECT_CODE_BYTE_LEN, LayoutId, PQ_CODEBOOK_VALUE_LEN, PQ96_CODE_BYTE_LEN, PrimarySegment,
    QUANTIZER_TABLE_VALUE_LEN, ResidualSegment, RowEntry, SegmentExpectations, SegmentIdentity,
    StoredErrorCertificate, encode_primary_segment, encode_residual_segment,
};

pub const HEADER_LEN: usize = 240;
pub const DIRECTORY_ENTRY_LEN: usize = 72;

pub const OFFSET_MAGIC: usize = 0;
pub const OFFSET_MAJOR: usize = 8;
pub const OFFSET_MINOR: usize = 10;
pub const OFFSET_FILE_KIND: usize = 12;
pub const OFFSET_HEADER_LEN: usize = 14;
pub const OFFSET_COLLECTION_ID: usize = 18;
pub const OFFSET_SEGMENT_ID: usize = 34;
pub const OFFSET_DIMENSION: usize = 50;
pub const OFFSET_ROW_COUNT: usize = 52;
pub const OFFSET_CODEC_ID: usize = 56;
pub const OFFSET_SCORER_VERSION: usize = 88;
pub const OFFSET_TRANSFORM_ID: usize = 92;
pub const OFFSET_QUANTIZER_ID: usize = 124;
pub const OFFSET_PQ_CODEBOOK_ID: usize = 156;
pub const OFFSET_LAYOUT_ID: usize = 188;
pub const OFFSET_SECTION_COUNT: usize = 190;
pub const OFFSET_DIRECTORY_OFFSET: usize = 192;
pub const OFFSET_PAYLOAD_LEN: usize = 200;
pub const OFFSET_WHOLE_FILE_BLAKE3: usize = 208;

pub const ENTRY_OFFSET_KIND: usize = 0;
pub const ENTRY_OFFSET_FLAGS: usize = 2;
pub const ENTRY_OFFSET_ALIGNMENT: usize = 4;
pub const ENTRY_OFFSET_OFFSET: usize = 8;
pub const ENTRY_OFFSET_LENGTH: usize = 16;
pub const ENTRY_OFFSET_LOGICAL_ROW_COUNT: usize = 24;
pub const ENTRY_OFFSET_BLOCK_SIZE: usize = 28;
pub const ENTRY_OFFSET_CRC_TABLE_INDEX: usize = 32;
pub const ENTRY_OFFSET_RESERVED_U16: usize = 34;
pub const ENTRY_OFFSET_RESERVED_U32: usize = 36;
pub const ENTRY_OFFSET_IDENTITY: usize = 40;

pub const KIND_IDS_SEQUENCES: u16 = 1;
pub const KIND_RADIUS_FLAGS: u16 = 2;
pub const KIND_PRIMARY_DIRECT_INT4: u16 = 3;
pub const KIND_INT4_QUANTIZER_TABLE: u16 = 4;
pub const KIND_PRIMARY_CERTIFICATE: u16 = 5;
pub const KIND_REFINED_CERTIFICATE: u16 = 6;
pub const KIND_PQ96_RESIDUAL: u16 = 7;
pub const KIND_PQ_CODEBOOK: u16 = 8;
pub const KIND_CRC_TABLE: u16 = 9;

pub const ROW_COUNT: u32 = 2;
pub const COLLECTION_ID: [u8; 16] = [0x11; 16];
pub const SEGMENT_ID: [u8; 16] = [0x22; 16];
pub const CODEC_ID: [u8; 32] = [0x33; 32];
pub const TRANSFORM_ID: [u8; 32] = [0x44; 32];
pub const QUANTIZER_ID: [u8; 32] = [0x55; 32];
pub const PQ_CODEBOOK_ID: [u8; 32] = [0x66; 32];
pub const SCORER_VERSION: u32 = 1;

pub fn identity() -> SegmentIdentity {
    SegmentIdentity {
        collection_id: COLLECTION_ID,
        segment_id: SEGMENT_ID,
        codec_id: CODEC_ID,
        scorer_version: SCORER_VERSION,
        transform_id: TRANSFORM_ID,
        quantizer_id: QUANTIZER_ID,
        pq_codebook_id: PQ_CODEBOOK_ID,
        layout: LayoutId::TiledSoa32,
    }
}

pub fn expectations() -> SegmentExpectations {
    SegmentExpectations {
        codec_id: CODEC_ID,
        scorer_version: SCORER_VERSION,
        transform_id: TRANSFORM_ID,
        quantizer_id: QUANTIZER_ID,
        pq_codebook_id: PQ_CODEBOOK_ID,
        layout: LayoutId::TiledSoa32,
    }
}

pub fn primary_segment() -> PrimarySegment {
    PrimarySegment {
        identity: identity(),
        rows: (0..ROW_COUNT).map(row_entry).collect(),
        radius_flags: (0..ROW_COUNT).map(radius_flags).collect(),
        primary_codes: (0..ROW_COUNT).map(primary_code).collect(),
        quantizer_table: quantizer_table(),
        primary_certificate: StoredErrorCertificate {
            max_reconstruction_l2_error: 0.125,
            eta_transform_dot: 0.001_953_125,
            query_norm_upper: 1.000_244_140_625,
            eta_serving_score: 0.000_061_035_156_25,
            epsilon: 0.127_015_209_197_998_05,
        },
        refined_certificate: StoredErrorCertificate {
            max_reconstruction_l2_error: 0.031_25,
            eta_transform_dot: 0.001_953_125,
            query_norm_upper: 1.000_244_140_625,
            eta_serving_score: 0.000_122_070_312_5,
            epsilon: 0.033_332_824_707_031_25,
        },
    }
}

pub fn residual_segment() -> ResidualSegment {
    ResidualSegment {
        identity: identity(),
        row_count: ROW_COUNT,
        residual_codes: (0..ROW_COUNT).map(residual_code).collect(),
        pq_codebook: pq_codebook(),
    }
}

pub fn primary_bytes() -> Vec<u8> {
    encode_primary_segment(&primary_segment()).expect("the minimal primary segment encodes")
}

pub fn residual_bytes() -> Vec<u8> {
    encode_residual_segment(&residual_segment()).expect("the minimal residual segment encodes")
}

pub fn row_entry(row: u32) -> RowEntry {
    RowEntry {
        chunk_id: ChunkId::from_u128(0x0100_0000_0000_0000_0000_0000_0000_0000 + u128::from(row)),
        document_id: DocumentId::from_u128(0x0A),
        put_seq: PutSeq::new(1, u64::from(row) + 7).expect("a small put sequence is valid"),
    }
}

pub fn radius_flags(row: u32) -> [u8; 4] {
    let row = row as u8;
    [row + 1, row + 2, row + 3, row + 4]
}

/// Nibble `c` of row `r` is `(c + 7r) mod 16`, packed low-nibble-first.
pub fn primary_code(row: u32) -> [u8; DIRECT_CODE_BYTE_LEN] {
    let mut code = [0; DIRECT_CODE_BYTE_LEN];
    for (byte, value) in code.iter_mut().enumerate() {
        let low = primary_nibble(row, byte * 2);
        let high = primary_nibble(row, byte * 2 + 1);
        *value = low | (high << 4);
    }
    code
}

pub fn primary_nibble(row: u32, coordinate: usize) -> u8 {
    ((coordinate + row as usize * 7) % 16) as u8
}

pub fn residual_code(row: u32) -> [u8; PQ96_CODE_BYTE_LEN] {
    let mut code = [0; PQ96_CODE_BYTE_LEN];
    for (subquantizer, value) in code.iter_mut().enumerate() {
        *value = ((subquantizer + row as usize * 13) % 256) as u8;
    }
    code
}

pub fn quantizer_table() -> Vec<f32> {
    (0..QUANTIZER_TABLE_VALUE_LEN)
        .map(|index| {
            let coordinate = index / 16;
            let center = index % 16;
            (center as f32 - 7.5) * 0.125 + coordinate as f32 * 0.001
        })
        .collect()
}

pub fn pq_codebook() -> Vec<f32> {
    (0..PQ_CODEBOOK_VALUE_LEN)
        .map(|index| {
            let subquantizer = index / (256 * 8);
            let code = (index / 8) % 256;
            let lane = index % 8;
            (code as f32 - 127.5) * 0.000_25
                + subquantizer as f32 * 0.000_001
                + lane as f32 * 0.000_000_1
        })
        .collect()
}

/// Offset of directory entry `index` inside an encoded segment.
pub fn entry_at(bytes: &[u8], index: usize) -> usize {
    let directory_offset = read_u64(bytes, OFFSET_DIRECTORY_OFFSET) as usize;
    directory_offset + index * DIRECTORY_ENTRY_LEN
}

/// Index of the first directory entry with the given section kind.
pub fn index_of_kind(bytes: &[u8], kind: u16) -> usize {
    let section_count = read_u16(bytes, OFFSET_SECTION_COUNT) as usize;
    (0..section_count)
        .find(|index| read_u16(bytes, entry_at(bytes, *index) + ENTRY_OFFSET_KIND) == kind)
        .unwrap_or_else(|| panic!("the fixture contains section kind {kind}"))
}

pub fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("two bytes"))
}

pub fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}

pub fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("eight bytes"))
}

pub fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

pub fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
