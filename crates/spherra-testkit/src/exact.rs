//! The exact original-space oracle a measured result is scored against.
//!
//! Certified truth is the FP64 dot product of the FP64-normalized query and the
//! FP64-normalized original direction — the same quantity the certificate
//! bounds must enclose. Ordering is score descending, then row ascending, so
//! two runs over the same corpus produce byte-identical neighbour lists even
//! when scores tie exactly.

use spherra_codec::{ScorerError, dot_f64, normalize_fp64};
use spherra_domain::DIMENSION;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Neighbor {
    pub row: u32,
    pub score: f64,
}

/// The FP64-normalized originals, computed once and reused by every query.
#[derive(Clone, Debug)]
pub struct ExactOracle {
    normalized: Vec<[f64; DIMENSION]>,
}

impl ExactOracle {
    pub fn new(corpus: &[[f32; DIMENSION]]) -> Result<Self, ScorerError> {
        let normalized = corpus
            .iter()
            .map(normalize_fp64)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { normalized })
    }

    pub fn row_count(&self) -> usize {
        self.normalized.len()
    }

    pub fn normalized_row(&self, row: usize) -> Option<&[f64; DIMENSION]> {
        self.normalized.get(row)
    }

    /// The certified truth for one stored row against an already normalized
    /// query.
    pub fn true_score(&self, normalized_query: &[f64; DIMENSION], row: usize) -> Option<f64> {
        self.normalized
            .get(row)
            .map(|stored| dot_f64(normalized_query, stored))
    }

    pub fn top_k(&self, query: &[f32; DIMENSION], k: usize) -> Result<Vec<Neighbor>, ScorerError> {
        let normalized = normalize_fp64(query)?;
        Ok(self.top_k_normalized(&normalized, k))
    }

    pub fn top_k_normalized(&self, normalized_query: &[f64; DIMENSION], k: usize) -> Vec<Neighbor> {
        let mut scored: Vec<Neighbor> = self
            .normalized
            .iter()
            .enumerate()
            .map(|(row, stored)| Neighbor {
                row: row as u32,
                score: dot_f64(normalized_query, stored),
            })
            .collect();
        sort_by_score_then_row(&mut scored);
        scored.truncate(k);
        scored
    }
}

/// Score descending, then row ascending. `total_cmp` keeps the order total even
/// if a degenerate input ever produced a NaN score, so the ranking can never
/// depend on comparison order.
pub fn sort_by_score_then_row(neighbors: &mut [Neighbor]) {
    neighbors.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.row.cmp(&right.row))
    });
}

/// The fraction of the exact top-`k` rows that a returned ranking recovers.
///
/// A budget smaller than `k` cannot reach 1.0, which is the honest reading: the
/// harness never pads a short candidate list to hide it.
pub fn recall_at(exact: &[Neighbor], returned: &[Neighbor], k: usize) -> f64 {
    let expected = exact.iter().take(k).map(|neighbor| neighbor.row);
    let found: Vec<u32> = returned
        .iter()
        .take(k)
        .map(|neighbor| neighbor.row)
        .collect();
    let mut hits = 0_usize;
    let mut total = 0_usize;
    for row in expected {
        total += 1;
        if found.contains(&row) {
            hits += 1;
        }
    }
    if total == 0 {
        return 0.0;
    }
    hits as f64 / total as f64
}
