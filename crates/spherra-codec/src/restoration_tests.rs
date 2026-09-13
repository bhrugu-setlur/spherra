//! Restoration must preserve stored bits, codes, and every prepared lookup.
use core::array;

use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;
use spherra_domain::{DIMENSION, ValidatedVector};

use super::FixedPointScorer;
use crate::{
    CODEC_ID, Pq96Code, Pq96Codebook, QuantizerTable, RestoreError, TransformPlan, transform,
};

fn vector(rng: &mut ChaCha20Rng) -> [f32; DIMENSION] {
    array::from_fn(|_| ((rng.next_u32() >> 8) as f32 / 16_777_216.0) * 2.0 - 1.0)
}

fn centers_bytes(table: &QuantizerTable) -> Vec<u8> {
    table
        .centers()
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}

fn centroid_values(book: &Pq96Codebook) -> Vec<f32> {
    book.canonical_bytes()
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}

#[test]
fn restored_models_preserve_bytes_codes_lookups_and_scores() {
    let mut rng = ChaCha20Rng::seed_from_u64(20260804);
    let plan = TransformPlan::from_seed(20260804);
    let directions: Vec<_> = (0..1512)
        .map(|_| {
            let raw = vector(&mut rng);
            let validated = ValidatedVector::new(raw.to_vec()).unwrap();
            transform(&plan, validated.normalized_direction().unwrap())
        })
        .collect();
    let table = QuantizerTable::train(
        &directions[..512]
            .iter()
            .map(|v| *v.as_array())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let residuals: Vec<[f32; DIMENSION]> = directions
        .iter()
        .map(|v| {
            let decoded = table.decode(&table.encode(v));
            array::from_fn(|i| v.as_array()[i] - decoded[i])
        })
        .collect();
    let book = Pq96Codebook::train(&residuals[..512], 20260804).unwrap();
    let restored_table = QuantizerTable::from_centers(table.centers()).unwrap();
    let values = centroid_values(&book);
    let restored_book = Pq96Codebook::from_centroids(&values).unwrap();
    assert_eq!(table.identity(), restored_table.identity());
    assert_eq!(book.codebook_id(), restored_book.codebook_id());
    assert_eq!(centers_bytes(&table), centers_bytes(&restored_table));
    assert_eq!(book.canonical_bytes(), restored_book.canonical_bytes());
    for sub in 0..96 {
        for code in 0..=255 {
            let expected = book.centroid(sub, code).unwrap().map(f32::to_bits);
            assert_eq!(
                expected,
                restored_book.centroid(sub, code).unwrap().map(f32::to_bits)
            );
        }
    }
    let mut codes = Vec::new();
    for (direction, residual) in directions[512..].iter().zip(&residuals[512..]) {
        let primary = table.encode(direction);
        let residual_code = book.encode(residual).unwrap();
        assert_eq!(primary, restored_table.encode(direction));
        assert_eq!(residual_code, restored_book.encode(residual).unwrap());
        codes.push((primary, residual_code));
    }
    assert_eq!(codes.len(), 1000);
    let scorer = FixedPointScorer::new();
    for _ in 0..20 {
        let raw = vector(&mut rng);
        let trained = scorer.prepare_query(&plan, &raw, &table, &book).unwrap();
        let restored = scorer
            .prepare_query(&plan, &raw, &restored_table, &restored_book)
            .unwrap();
        assert_eq!(trained.primary_lookup, restored.primary_lookup);
        assert_eq!(trained.residual_lookup, restored.residual_lookup);
        assert_eq!(trained.provenance(), restored.provenance());
        for (primary, residual) in &codes {
            assert_eq!(
                scorer.score_primary(&trained, primary).raw(),
                scorer.score_primary(&restored, primary).raw()
            );
            assert_eq!(
                scorer.score_refined(&trained, primary, residual).raw(),
                scorer.score_refined(&restored, primary, residual).raw()
            );
        }
    }
}

#[test]
fn restoration_rejects_wrong_lengths() {
    for length in [0, 768 * 16 - 1, 768 * 16 + 1] {
        assert!(matches!(QuantizerTable::from_centers(&vec![0.0; length]),
            Err(RestoreError::Length { expected: 12288, actual }) if actual == length));
    }
    for length in [0, 96 * 256 * 8 - 1, 96 * 256 * 8 + 1] {
        assert!(matches!(Pq96Codebook::from_centroids(&vec![0.0; length]),
            Err(RestoreError::Length { expected: 196608, actual }) if actual == length));
    }
}

#[test]
fn restoration_rejects_nonfinite_values() {
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for index in [0, 1234, 12287] {
            let mut values = vec![0.0; 768 * 16];
            values[index] = bad;
            assert!(matches!(QuantizerTable::from_centers(&values),
                Err(RestoreError::NonFinite { index: actual }) if actual == index));
        }
        for index in [0, 54321, 196607] {
            let mut values = vec![0.0; 96 * 256 * 8];
            values[index] = bad;
            assert!(matches!(Pq96Codebook::from_centroids(&values),
                Err(RestoreError::NonFinite { index: actual }) if actual == index));
        }
    }
}

#[test]
fn restoration_rejects_negative_zero() {
    for index in [0, 1234, 12287] {
        let mut values = vec![0.0; 768 * 16];
        values[index] = -0.0;
        assert!(matches!(QuantizerTable::from_centers(&values),
            Err(RestoreError::NegativeZero { index: actual }) if actual == index));
    }
    for index in [0, 54321, 196607] {
        let mut values = vec![0.0; 96 * 256 * 8];
        values[index] = -0.0;
        assert!(matches!(Pq96Codebook::from_centroids(&values),
            Err(RestoreError::NegativeZero { index: actual }) if actual == index));
    }
}

#[test]
fn restoration_checks_order_within_each_quantizer_coordinate() {
    for coordinate in [0, 381, 767] {
        let mut values = vec![0.0; 768 * 16];
        values[coordinate * 16 + 7] = -1.0;
        assert!(matches!(QuantizerTable::from_centers(&values),
            Err(RestoreError::DecreasingCenters { coordinate: actual, code: 7 }) if actual == coordinate));
    }
    // A decrease between coordinates is valid, and repeated centers are valid.
    let values: Vec<_> = (0..768)
        .flat_map(|_| (0..16).map(|code| (code / 2) as f32))
        .collect();
    assert!(QuantizerTable::from_centers(&values).is_ok());
}

#[test]
fn restoration_preserves_extreme_finite_values_and_positive_zero() {
    let values: Vec<_> = (0..768)
        .flat_map(|_| {
            [
                -f32::MAX,
                -f32::MIN_POSITIVE,
                -f32::from_bits(1),
                0.0,
                f32::from_bits(1),
                f32::MIN_POSITIVE,
                1.0,
                2.0,
                3.0,
                4.0,
                5.0,
                6.0,
                7.0,
                8.0,
                9.0,
                f32::MAX,
            ]
        })
        .collect();
    let table = QuantizerTable::from_centers(&values).unwrap();
    assert_eq!(
        centers_bytes(&table),
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    );
    let residuals: Vec<_> = (0..96 * 256 * 8)
        .map(|i| values[i % values.len()])
        .collect();
    let book = Pq96Codebook::from_centroids(&residuals).unwrap();
    assert_eq!(
        book.canonical_bytes(),
        residuals
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    );
    assert_eq!(book.decode(&Pq96Code::from_bytes([0; 96]))[3].to_bits(), 0);
}

#[test]
fn codec_identity_matches_the_preimplementation_golden() {
    let expected = "0ceb4208a53bf92938a513426e8a0528c429a9f91ca345ce9ad679df6114df73";
    let actual: String = CODEC_ID.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(actual, expected);
}
