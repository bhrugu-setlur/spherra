//! Exact integer scoring directly from a TILED_SOA_32 tile.
use crate::{DirectCode, FixedPointScorer, PreparedScorerQuery};
use spherra_domain::DIMENSION;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelPath {
    CheckedScalar,
    SafeTile,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelError {
    TileLength { actual: usize },
    Lanes { actual: usize },
}
impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TileLength { actual } => write!(
                f,
                "primary tile has {actual} bytes; expected {}",
                DIMENSION * 16
            ),
            Self::Lanes { actual } => {
                write!(f, "primary tile has {actual} active lanes; expected 1..=32")
            }
        }
    }
}
impl std::error::Error for KernelError {}

/// Scores the active rows of one exactly 12,288-byte TILED_SOA_32 tile.
/// Every result is identical to the checked primary scorer, in Q24 units.
/// Invalid geometry leaves all of `out` untouched; success writes only the
/// first `lanes` entries. This raw arithmetic operation grants no certificate.
pub fn score_tile_primary(
    query: &PreparedScorerQuery,
    tile: &[u8],
    lanes: usize,
    out: &mut [i64; 32],
) -> Result<KernelPath, KernelError> {
    if tile.len() != DIMENSION * 16 {
        return Err(KernelError::TileLength { actual: tile.len() });
    }
    if !(1..=32).contains(&lanes) {
        return Err(KernelError::Lanes { actual: lanes });
    }
    let maximum = query
        .lookup_scale_measurement()
        .maximum_primary_lookup_entry();
    let mut sums = [0_i64; 32];
    let path = if i128::from(maximum) * DIMENSION as i128 <= i128::from(i64::MAX) {
        // For every prefix t <= 768, |sum_t| <= t * maximum <= i64::MAX.
        // The immutable query owns both the entries and their measured maximum.
        if lanes == 32 {
            score_full_tile(query.primary_lookup_entries(), tile, &mut sums);
        } else {
            let pairs = lanes / 2;
            for (lookup, bytes) in query
                .primary_lookup_entries()
                .iter()
                .zip(tile.chunks_exact(16))
            {
                for (pair, &packed) in sums[..pairs * 2].chunks_exact_mut(2).zip(bytes) {
                    pair[0] += lookup[usize::from(packed & 15)];
                    pair[1] += lookup[usize::from(packed >> 4)];
                }
                if lanes % 2 == 1 {
                    sums[lanes - 1] += lookup[usize::from(bytes[pairs] & 15)];
                }
            }
        }
        KernelPath::SafeTile
    } else {
        let scorer = FixedPointScorer::new();
        for (lane, sum) in sums[..lanes].iter_mut().enumerate() {
            let code = DirectCode::from_nibbles(std::array::from_fn(|coordinate| {
                (tile[coordinate * 16 + lane / 2] >> ((lane % 2) * 4)) & 15
            }))
            .expect("masked four-bit code");
            *sum = scorer.score_primary(query, &code).raw();
        }
        KernelPath::CheckedScalar
    };
    out[..lanes].copy_from_slice(&sums[..lanes]);
    Ok(path)
}

// Called only after geometry validation and the i128 range proof. Keeping all
// eight pairs visible to the compiler lets it unroll the full-tile loop without
// keeping all 32 accumulators live at once. The two passes reuse the same small
// tile; each lane still adds its 768 entries in coordinate order.
fn score_full_tile(lookups: &[[i64; 16]; DIMENSION], tile: &[u8], sums: &mut [i64; 32]) {
    for (half, sums) in sums.chunks_exact_mut(16).enumerate() {
        for (lookup, bytes) in lookups.iter().zip(tile.chunks_exact(16)) {
            for (pair, &packed) in sums.chunks_exact_mut(2).zip(&bytes[half * 8..]) {
                pair[0] += lookup[usize::from(packed & 15)];
                pair[1] += lookup[usize::from(packed >> 4)];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_tile_matches_checked_sums_with_distinct_lanes_and_coordinates() {
        let limit = i64::MAX / DIMENSION as i64;
        let lookup = Box::new(std::array::from_fn(|coordinate| {
            std::array::from_fn(|code| {
                let magnitude = limit - (coordinate * 16 + code) as i64;
                if (coordinate + code) % 3 == 0 {
                    -magnitude
                } else {
                    magnitude
                }
            })
        }));
        let tile: [u8; DIMENSION * 16] = std::array::from_fn(|i| {
            let coordinate = i / 16;
            let pair = i % 16;
            let low = (coordinate + pair) % 16;
            let high = (coordinate / 16 + 15 - pair) % 16;
            (low | (high << 4)) as u8
        });
        let mut actual = [0; 32];
        score_full_tile(&lookup, &tile, &mut actual);
        for (lane, actual) in actual.into_iter().enumerate() {
            let expected = lookup.iter().enumerate().fold(0_i64, |sum, (c, entries)| {
                let code = (tile[c * 16 + lane / 2] >> ((lane % 2) * 4)) & 15;
                sum.checked_add(entries[usize::from(code)]).unwrap()
            });
            assert_eq!(actual, expected, "lane {lane}");
        }
    }
}
