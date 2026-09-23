//! Integer equality and dispatch boundaries, including tables outside the
//! public query preparer's stricter range limit.
use super::{FixedPointScorer, PreparedScorerQuery};
use crate::{
    DirectCode, KernelPath, Pq96Codebook, QuantizerTable, TransformPlan, score_tile_primary,
};
use proptest::prelude::*;
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;
use spherra_domain::DIMENSION;

const TILE_BYTES: usize = DIMENSION * 16;

fn prepared(seed: u64) -> PreparedScorerQuery {
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    let mut sample = || ((rng.next_u32() >> 8) as f32 / 16_777_216.0) * 2.0 - 1.0;
    let mut centers = Vec::with_capacity(DIMENSION * 16);
    for _ in 0..DIMENSION {
        let mut coordinate: [f32; 16] = std::array::from_fn(|_| sample());
        coordinate.sort_by(f32::total_cmp);
        centers.extend(coordinate);
    }
    let mut query = std::array::from_fn(|_| sample());
    query[0] = 1.0;
    let table = QuantizerTable::from_centers(&centers).unwrap();
    let book = Pq96Codebook::from_centroids(&vec![0.0; 96 * 256 * 8]).unwrap();
    FixedPointScorer::new()
        .prepare_query(&TransformPlan::from_seed(seed), &query, &table, &book)
        .unwrap()
}

fn reference(query: &PreparedScorerQuery, tile: &[u8], lane: usize) -> i64 {
    let code = DirectCode::from_nibbles(std::array::from_fn(|coordinate| {
        (tile[coordinate * 16 + lane / 2] >> ((lane % 2) * 4)) & 15
    }))
    .unwrap();
    FixedPointScorer::new().score_primary(query, &code).raw()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn random_queries_and_tiles_match_checked_scores(
        seed in any::<u64>(), lanes in 1_usize..=32,
        tile in prop::collection::vec(any::<u8>(), TILE_BYTES),
    ) {
        let query = prepared(seed);
        if let Some(compact) = query.compact_primary_lookup() {
            for (compact, wide) in compact.iter().flatten().zip(query.primary_lookup.iter().flatten()) {
                prop_assert_eq!(i64::from(*compact), *wide);
            }
        }
        // Exercise the specialized full tile on every case as well as a
        // variable width; a random width alone rarely selects all 32 lanes.
        for width in [lanes, 32] {
            let mut out = [i64::MIN; 32];
            prop_assert_eq!(score_tile_primary(&query, &tile, width, &mut out).unwrap(), KernelPath::SafeTile);
            for (lane, actual) in out[..width].iter().enumerate() {
                prop_assert_eq!(*actual, reference(&query, &tile, lane));
            }
            prop_assert!(out[width..].iter().all(|v| *v == i64::MIN));
        }
    }
}

#[test]
fn every_partial_width_and_nibble_extreme_matches() {
    let query = prepared(20260804);
    for byte in [0x00, 0x0f, 0xf0, 0xff] {
        let tile = [byte; TILE_BYTES];
        for lanes in 1..=32 {
            let mut out = [i64::MIN; 32];
            assert_eq!(
                score_tile_primary(&query, &tile, lanes, &mut out).unwrap(),
                KernelPath::SafeTile
            );
            for (lane, actual) in out[..lanes].iter().enumerate() {
                assert_eq!(*actual, reference(&query, &tile, lane));
            }
        }
    }
}

#[test]
fn invalid_geometry_leaves_the_entire_output_untouched() {
    let query = prepared(42);
    for length in [0, 1, TILE_BYTES - 1, TILE_BYTES + 1] {
        let mut out = [123456789; 32];
        assert!(score_tile_primary(&query, &vec![0; length], 32, &mut out).is_err());
        assert_eq!(out, [123456789; 32]);
    }
    for lanes in [0, 33, usize::MAX] {
        let mut out = [123456789; 32];
        assert!(score_tile_primary(&query, &[0; TILE_BYTES], lanes, &mut out).is_err());
        assert_eq!(out, [123456789; 32]);
    }
}

#[test]
fn range_proof_uses_i128_and_falls_back_to_checked_reference() {
    let mut query = prepared(42);
    query.primary_compact = None;
    let limit = i64::MAX / DIMENSION as i64;
    for sign in [-1, 1] {
        query.primary_lookup.fill([sign * limit; 16]);
        query.lookup_scale_measurement.maximum_primary_lookup_entry = limit;
        let mut out = [0; 32];
        assert_eq!(
            score_tile_primary(&query, &[0xff; TILE_BYTES], 32, &mut out).unwrap(),
            KernelPath::SafeTile
        );
        assert_eq!(out, [sign * limit * DIMENSION as i64; 32]);
    }
    // The global maximum fails the range proof while each row's actual sum
    // still fits. The test does not change production query admission.
    query.primary_lookup.fill([0; 16]);
    query.primary_lookup[0].fill(i64::MAX);
    query.lookup_scale_measurement.maximum_primary_lookup_entry = i64::MAX;
    let mut out = [0; 32];
    assert_eq!(
        score_tile_primary(&query, &[0; TILE_BYTES], 32, &mut out).unwrap(),
        KernelPath::CheckedScalar
    );
    assert_eq!(out, [i64::MAX; 32]);
    assert_eq!(query.primary_lookup_entries()[0][0], i64::MAX);
}

#[test]
fn compact_primary_table_requires_a_proof_for_every_partial_sum() {
    let mut query = prepared(42);
    let limit = i64::from(i32::MAX) / DIMENSION as i64;
    for magnitude in [limit, limit + 1] {
        for sign in [-1, 1] {
            query.primary_lookup.fill([sign * magnitude; 16]);
            query.lookup_scale_measurement.maximum_primary_lookup_entry = magnitude;
            query.primary_compact = super::compact_primary_lookup(&query.primary_lookup, magnitude);
            assert_eq!(query.primary_compact.is_some(), magnitude == limit);
            for width in [1, 15, 16, 17, 31, 32] {
                let mut out = [i64::MIN; 32];
                assert_eq!(
                    score_tile_primary(&query, &[0xff; TILE_BYTES], width, &mut out).unwrap(),
                    KernelPath::SafeTile
                );
                assert_eq!(
                    out[..width],
                    vec![sign * magnitude * DIMENSION as i64; width]
                );
                assert!(out[width..].iter().all(|&v| v == i64::MIN));
            }
        }
    }
}

#[test]
#[ignore = "release-only paired scan timing; run without other Spherra tests or benchmarks"]
fn compare_compact_and_wide_scan_timing() {
    use std::{hint::black_box, time::Instant};
    assert!(!cfg!(debug_assertions), "timing requires --release");
    let compact = prepared(42);
    assert!(compact.primary_compact.is_some());
    let mut wide = compact.clone();
    wide.primary_compact = None;
    let mut tiles = vec![0_u8; TILE_BYTES * 1024];
    ChaCha20Rng::seed_from_u64(20260804).fill_bytes(&mut tiles);
    eprintln!(
        "scan input: 32768 rows; query seed 42; tiles BLAKE3 {}",
        blake3::hash(&tiles)
    );
    let scan = |query: &PreparedScorerQuery| {
        let start = Instant::now();
        let mut checksum = 0_i64;
        for tile in tiles.chunks_exact(TILE_BYTES) {
            let mut scores = [0; 32];
            assert_eq!(
                score_tile_primary(black_box(query), tile, 32, &mut scores).unwrap(),
                KernelPath::SafeTile
            );
            for score in black_box(scores) {
                checksum = checksum.wrapping_add(score);
            }
        }
        (start.elapsed().as_nanos(), checksum)
    };
    assert_eq!(scan(&compact).1, scan(&wide).1);
    for trial in 0..20 {
        let (compact_time, wide_time) = if trial % 2 == 0 {
            (scan(&compact), scan(&wide))
        } else {
            let wide_time = scan(&wide);
            (scan(&compact), wide_time)
        };
        assert_eq!(compact_time.1, wide_time.1);
        eprintln!(
            "scan trial {trial}: compact_ns={} wide_ns={}",
            compact_time.0, wide_time.0
        );
    }
}
