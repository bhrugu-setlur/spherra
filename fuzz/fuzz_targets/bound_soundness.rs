#![no_main]

use core::array;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use spherra_codec::{
    CertificateBlockId, CertificateRow, DirectCode, ExhaustiveBlock, FixedPointScorer, Pq96Code,
    Pq96Codebook, QuantizerTable, TransformPlan, build_exhaustive_certificate, dot_f64,
    normalize_fp64,
};
use spherra_domain::DIMENSION;

fn byte_at(data: &[u8], offset: usize, fallback: u8) -> u8 {
    data.get(offset % data.len().max(1))
        .copied()
        .unwrap_or(fallback)
}

/// Produces finite normal FP32 values spanning a deliberately wide exponent
/// range. Normalization keeps the subsequent transform inputs bounded while
/// still exercising exponent-sensitive conversion and reduction paths.
fn vector(data: &[u8], salt: usize) -> [f32; DIMENSION] {
    array::from_fn(|coordinate| {
        let offset = coordinate.wrapping_mul(11).wrapping_add(salt);
        let bits = u32::from_le_bytes([
            byte_at(data, offset, coordinate as u8),
            byte_at(data, offset + 1, (coordinate as u8).wrapping_mul(29)),
            byte_at(data, offset + 2, (coordinate as u8).wrapping_add(71)),
            byte_at(data, offset + 3, (coordinate as u8).wrapping_mul(47)),
        ]);
        let exponent = 90 + ((bits >> 23) % 75);
        f32::from_bits((bits & 0x807f_ffff) | (exponent << 23))
    })
}

fn codebook() -> &'static Pq96Codebook {
    static CODEBOOK: OnceLock<Pq96Codebook> = OnceLock::new();
    CODEBOOK.get_or_init(Pq96Codebook::fuzz_fixture)
}

fn table() -> &'static QuantizerTable {
    static TABLE: OnceLock<QuantizerTable> = OnceLock::new();
    TABLE.get_or_init(|| {
        QuantizerTable::train(&[[-0.25; DIMENSION], [0.25; DIMENSION]])
            .expect("the fixed fuzz quantizer calibration is finite")
    })
}

fuzz_target!(|data: &[u8]| {
    let mut seed_bytes = [0_u8; 8];
    for (index, byte) in data.iter().copied().take(seed_bytes.len()).enumerate() {
        seed_bytes[index] = byte;
    }
    let plan = TransformPlan::from_seed(u64::from_le_bytes(seed_bytes));
    let original = vector(data, 3);
    let query = vector(data, 11);
    let nibbles = array::from_fn(|coordinate| byte_at(data, coordinate, coordinate as u8) & 0x0f);
    let primary = DirectCode::from_nibbles(nibbles).expect("masked fuzz nibbles are valid");
    let residual = Pq96Code::from_bytes(array::from_fn(|subquantizer| {
        byte_at(
            data,
            subquantizer.wrapping_mul(11).wrapping_add(5),
            subquantizer as u8,
        )
    }));
    let block = ExhaustiveBlock::from_rows(
        CertificateBlockId::from_bytes([0x5a; 32]),
        1,
        [CertificateRow::new(0, &original, &primary, &residual)],
    )
    .expect("the fuzz row covers the entire one-row block");
    let scorer = FixedPointScorer::new();
    let prepared = scorer
        .prepare_query(&plan, &query, table(), codebook())
        .expect("finite fuzz inputs must prepare a fixed-point query");
    let certificate = build_exhaustive_certificate(&scorer, &plan, table(), codebook(), &block)
        .expect("every original row is available for the exhaustive certificate");
    let normalized_query = normalize_fp64(&query).expect("finite non-zero fuzz query");
    let normalized_original = normalize_fp64(&original).expect("finite non-zero fuzz original");
    let true_score = dot_f64(&normalized_query, &normalized_original);
    let candidate = block
        .candidate(0)
        .expect("the checked fuzz block has row zero");
    let primary_bounds = certificate
        .primary_bounds(
            certificate
                .score_primary(&scorer, &prepared, candidate)
                .expect("matching fixed-point provenance"),
        )
        .expect("matching primary certificate kind");
    let refined_bounds = certificate
        .refined_bounds(
            certificate
                .score_refined(&scorer, &prepared, candidate)
                .expect("matching fixed-point provenance"),
        )
        .expect("matching refined certificate kind");

    assert!(primary_bounds.lower <= true_score && true_score <= primary_bounds.upper);
    assert!(refined_bounds.lower <= true_score && true_score <= refined_bounds.upper);
});
