#![no_main]

use core::array;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use spherra_codec::{
    CertificateBlockId, CertificateRow, DirectCode, ExhaustiveBlock, FixedPointScorer, Pq96Code,
    Pq96Codebook, PrimaryScore, QuantizerTable, TransformPlan, build_exhaustive_certificate,
    dot_f64, normalize_fp64,
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

/// Builds a finite vector with a 200-bit component exponent gap. The FP64
/// normalization path must retain a sound norm/reduction certificate even when
/// the small squared term is far below the dominant squared term.
fn exponent_gap_vector(data: &[u8], salt: usize) -> [f32; DIMENSION] {
    let mut values = [0.0; DIMENSION];
    let dominant_sign = u32::from(byte_at(data, salt, 0) & 1) << 31;
    let tiny_sign = u32::from(byte_at(data, salt + 1, 1) & 1) << 31;
    let middle_sign = u32::from(byte_at(data, salt + 2, 0) & 1) << 31;
    let dominant_mantissa = u32::from(byte_at(data, salt + 3, 0)) << 15;
    let tiny_mantissa = u32::from(byte_at(data, salt + 4, 0)) << 15;
    let middle_mantissa = u32::from(byte_at(data, salt + 5, 0)) << 15;

    values[salt % DIMENSION] = f32::from_bits(dominant_sign | (227 << 23) | dominant_mantissa);
    values[(salt + 257) % DIMENSION] =
        f32::from_bits(tiny_sign | (27 << 23) | tiny_mantissa);
    values[(salt + 513) % DIMENSION] =
        f32::from_bits(middle_sign | (127 << 23) | middle_mantissa);
    values
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

fn cancellation_codebook() -> &'static Pq96Codebook {
    static CODEBOOK: OnceLock<Pq96Codebook> = OnceLock::new();
    CODEBOOK.get_or_init(Pq96Codebook::fuzz_cancellation_fixture)
}

fn cancellation_table() -> &'static QuantizerTable {
    static TABLE: OnceLock<QuantizerTable> = OnceLock::new();
    TABLE.get_or_init(|| {
        let magnitude = Pq96Codebook::FUZZ_CANCELLATION_MAGNITUDE;
        QuantizerTable::train(&[[-magnitude; DIMENSION], [magnitude; DIMENSION]])
            .expect("the cancellation quantizer calibration is finite")
    })
}

fuzz_target!(|data: &[u8]| {
    let mut seed_bytes = [0_u8; 8];
    for (index, byte) in data.iter().copied().take(seed_bytes.len()).enumerate() {
        seed_bytes[index] = byte;
    }
    let plan = TransformPlan::from_seed(u64::from_le_bytes(seed_bytes));
    let original = if byte_at(data, 2, 0) & 1 == 0 {
        vector(data, 3)
    } else {
        exponent_gap_vector(data, 3)
    };
    let query = if byte_at(data, 3, 0) & 1 == 0 {
        vector(data, 11)
    } else {
        exponent_gap_vector(data, 11)
    };
    let cancellation_fixture = byte_at(data, 1, 0) & 1 != 0;
    let (primary, residual, table, codebook) = if cancellation_fixture {
        (
            DirectCode::from_nibbles([0x0f; DIMENSION])
                .expect("the high cancellation code is valid"),
            Pq96Code::from_bytes([0; Pq96Code::BYTE_LEN]),
            cancellation_table(),
            cancellation_codebook(),
        )
    } else {
        let nibbles =
            array::from_fn(|coordinate| byte_at(data, coordinate, coordinate as u8) & 0x0f);
        let residual = Pq96Code::from_bytes(array::from_fn(|subquantizer| {
            byte_at(
                data,
                subquantizer.wrapping_mul(11).wrapping_add(5),
                subquantizer as u8,
            )
        }));
        (
            DirectCode::from_nibbles(nibbles).expect("masked fuzz nibbles are valid"),
            residual,
            table(),
            codebook(),
        )
    };
    if cancellation_fixture {
        let primary_values = table.decode(&primary);
        let residual_values = codebook.decode(&residual);
        assert!(primary_values
            .iter()
            .zip(residual_values)
            .all(|(primary, residual)| *primary == -residual));
    }
    let block = ExhaustiveBlock::from_rows(
        CertificateBlockId::from_bytes([0x5a; 32]),
        1,
        [CertificateRow::new(0, &original, &primary, &residual)],
    )
    .expect("the fuzz row covers the entire one-row block");
    let scorer = FixedPointScorer::new();
    let prepared = scorer
        .prepare_query(&plan, &query, table, codebook)
        .expect("finite fuzz inputs must prepare a fixed-point query");
    let certificate = build_exhaustive_certificate(&scorer, &plan, table, codebook, &block)
        .expect("every original row is available for the exhaustive certificate");
    let normalized_query = normalize_fp64(&query).expect("finite non-zero fuzz query");
    let normalized_original = normalize_fp64(&original).expect("finite non-zero fuzz original");
    let true_score = dot_f64(&normalized_query, &normalized_original);
    let candidate = block
        .candidate(0)
        .expect("the checked fuzz block has row zero");
    let prepared_candidate = codebook.prepare_candidate(PrimaryScore::for_row(0), residual);
    let primary_bounds = certificate
        .primary_bounds(
            certificate
                .score_primary(&scorer, &prepared, &candidate)
                .expect("matching fixed-point provenance"),
        )
        .expect("matching primary certificate kind");
    let refined_bounds = certificate
        .refined_bounds(
            certificate
                .score_refined(&scorer, &prepared, &candidate, &prepared_candidate)
                .expect("matching fixed-point provenance"),
        )
        .expect("matching refined certificate kind");

    assert!(primary_bounds.lower <= true_score && true_score <= primary_bounds.upper);
    assert!(refined_bounds.lower <= true_score && true_score <= refined_bounds.upper);
});
