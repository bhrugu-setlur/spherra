#![no_main]

//! Hostile segment bytes must be refused as values, never as panics.
//!
//! Pure random bytes never pass the magic check, so each input is also spliced
//! over a valid minimal primary and residual file. That drives the fuzzer into
//! the section directory, block-CRC, section-identity, and pairing paths that
//! only a nearly-valid file reaches.

use libfuzzer_sys::fuzz_target;
use spherra_domain::{ChunkId, DocumentId, PutSeq};
use spherra_format::{
    DIRECT_CODE_BYTE_LEN, LayoutId, PQ_CODEBOOK_VALUE_LEN, PQ96_CODE_BYTE_LEN,
    PairedSegmentReaders, PrimaryFileReader, PrimarySegment, QUANTIZER_TABLE_VALUE_LEN,
    ResidualFileReader, ResidualSegment, RowEntry, SegmentExpectations, SegmentIdentity,
    StoredErrorCertificate, encode_primary_segment, encode_residual_segment,
};

const ROW_COUNT: u32 = 3;

fn identity() -> SegmentIdentity {
    SegmentIdentity {
        collection_id: [0x11; 16],
        segment_id: [0x22; 16],
        codec_id: [0x33; 32],
        scorer_version: 1,
        transform_id: [0x44; 32],
        quantizer_id: [0x55; 32],
        pq_codebook_id: [0x66; 32],
        layout: LayoutId::TiledSoa32,
    }
}

fn expectations() -> SegmentExpectations {
    SegmentExpectations {
        codec_id: [0x33; 32],
        scorer_version: 1,
        transform_id: [0x44; 32],
        quantizer_id: [0x55; 32],
        pq_codebook_id: [0x66; 32],
        layout: LayoutId::TiledSoa32,
    }
}

fn certificate() -> StoredErrorCertificate {
    StoredErrorCertificate {
        max_reconstruction_l2_error: 0.125,
        eta_transform_dot: 0.001_953_125,
        query_norm_upper: 1.0,
        eta_serving_score: 0.000_061_035_156_25,
        epsilon: 0.127,
    }
}

/// The base files are built once: re-encoding a 786 KiB codebook per iteration
/// would cap throughput long before the structural paths are well covered.
fn base_primary() -> &'static Vec<u8> {
    static BASE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    BASE.get_or_init(build_base_primary)
}

fn base_residual() -> &'static Vec<u8> {
    static BASE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    BASE.get_or_init(build_base_residual)
}

fn build_base_primary() -> Vec<u8> {
    encode_primary_segment(&PrimarySegment {
        identity: identity(),
        rows: (0..ROW_COUNT)
            .map(|row| RowEntry {
                chunk_id: ChunkId::from_u128(u128::from(row) + 1),
                document_id: DocumentId::from_u128(0x0A),
                put_seq: PutSeq::new(1, u64::from(row)).expect("a small put sequence"),
            })
            .collect(),
        radius_flags: (0..ROW_COUNT).map(|row| [row as u8; 4]).collect(),
        primary_codes: (0..ROW_COUNT)
            .map(|row| [row as u8 % 0x11; DIRECT_CODE_BYTE_LEN])
            .collect(),
        quantizer_table: (0..QUANTIZER_TABLE_VALUE_LEN)
            .map(|index| index as f32 * 0.001 - 1.0)
            .collect(),
        primary_certificate: certificate(),
        refined_certificate: certificate(),
    })
    .expect("the fuzz base primary segment encodes")
}

fn build_base_residual() -> Vec<u8> {
    encode_residual_segment(&ResidualSegment {
        identity: identity(),
        row_count: ROW_COUNT,
        residual_codes: (0..ROW_COUNT)
            .map(|row| [row as u8; PQ96_CODE_BYTE_LEN])
            .collect(),
        pq_codebook: (0..PQ_CODEBOOK_VALUE_LEN)
            .map(|index| (index % 256) as f32 * 0.000_25 - 0.032)
            .collect(),
    })
    .expect("the fuzz base residual segment encodes")
}

/// Overwrites `base` at an input-chosen offset with the remaining input bytes.
fn splice(base: &[u8], data: &[u8]) -> Vec<u8> {
    let mut spliced = base.to_vec();
    let Some((offset_bytes, patch)) = data.split_at_checked(4) else {
        return spliced;
    };

    let offset = u32::from_le_bytes([
        offset_bytes[0],
        offset_bytes[1],
        offset_bytes[2],
        offset_bytes[3],
    ]) as usize
        % spliced.len();
    let len = patch.len().min(spliced.len() - offset);
    spliced[offset..offset + len].copy_from_slice(&patch[..len]);
    spliced
}

/// Exercises every public accessor a successfully opened reader offers.
fn drain(bytes: Vec<u8>) {
    let expected = expectations();

    if let Ok(primary) = PrimaryFileReader::open_bytes(bytes.clone(), &expected) {
        let row_count = primary.row_count();
        std::hint::black_box(primary.quantizer_table().is_ok());
        std::hint::black_box(primary.primary_certificate().is_ok());
        std::hint::black_box(primary.refined_certificate().is_ok());
        for row in [0, row_count / 2, row_count.saturating_sub(1), row_count] {
            std::hint::black_box(primary.row(row).is_ok());
            std::hint::black_box(primary.radius_flags(row).is_ok());
            std::hint::black_box(primary.primary_code(row).is_ok());
        }
    }

    if let Ok(residual) = ResidualFileReader::open_bytes(bytes.clone(), &expected) {
        std::hint::black_box(residual.pq_codebook().is_ok());

        if let Ok(primary) = PrimaryFileReader::open_bytes(base_primary().clone(), &expected)
            && let Ok(paired) = PairedSegmentReaders::open(primary, residual)
        {
            for row in 0..=ROW_COUNT {
                std::hint::black_box(paired.residual_code(row).is_ok());
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    drain(data.to_vec());
    drain(splice(base_primary(), data));
    drain(splice(base_residual(), data));
});
