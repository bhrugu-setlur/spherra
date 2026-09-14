//! Optional magnitude-aware search. It shares the cosine scan and refinement
//! machinery through `search::Ranking`; only the ordering key differs.
use crate::{
    Error, Index, RowId, SearchOptions, Vector,
    search::{Ranking, Selected},
};
use spherra_codec::FixedPointScorer;

/// Approximate original-vector dot product, with an enclosing score interval.
/// Ranking uses exact integer products; the displayed f64 score may round ties.
/// ```compile_fail
/// let hit = spherra::DotProductHit {};
/// ```
#[derive(Clone, Debug)]
pub struct DotProductHit {
    row: RowId,
    segment: u32,
    score: f64,
    interval: (f64, f64),
    magnitude_bits: u16,
}
impl DotProductHit {
    pub fn row(&self) -> RowId {
        self.row
    }
    pub fn segment(&self) -> u32 {
        self.segment
    }
    /// Approximate dot(query, original row), including the query's FP64 norm.
    pub fn score(&self) -> f64 {
        self.score
    }
    /// Encloses the original-space FP64 dot product under the index trust contract.
    /// Includes direction reconstruction, stored-length rounding and arithmetic.
    pub fn interval(&self) -> (f64, f64) {
        self.interval
    }
    pub fn stored_magnitude(&self) -> f32 {
        half::f16::from_bits(self.magnitude_bits).to_f32()
    }
}
#[derive(Clone, Debug)]
pub struct DotProductResult {
    generation: u64,
    candidate_budget: usize,
    rows_scanned: u64,
    rows_refined: u64,
    hits: Vec<DotProductHit>,
}
impl DotProductResult {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn candidate_budget(&self) -> usize {
        self.candidate_budget
    }
    pub fn rows_scanned(&self) -> u64 {
        self.rows_scanned
    }
    pub fn rows_refined(&self) -> u64 {
        self.rows_refined
    }
    pub fn hits(&self) -> &[DotProductHit] {
        &self.hits
    }
}

// Every finite nonnegative FP16 value is an integer multiple of 2^-24.
// The largest unit count is <2^40. Multiplying any i64 score is <2^103,
// so primary and refined comparisons fit i128 without rounding or saturation.
fn magnitude_units(bits: u16) -> i128 {
    let exponent = (bits >> 10) & 31;
    let fraction = i128::from(bits & 1023);
    if exponent == 0 {
        fraction
    } else {
        (1024 + fraction) << (exponent - 1)
    }
}
struct DotProduct;
impl Ranking for DotProduct {
    type Key = i128;
    #[inline(always)]
    fn key(score: i64, magnitudes: &[u16], local: usize) -> i128 {
        i128::from(score) * magnitude_units(magnitudes[local])
    }
}

// 2^-40 exceeds the FP64 gamma bounds for 768-term norm/dot reductions,
// square roots and normalization divisions (see the search amendment).
const REDUCTION_ALLOWANCE: f64 = 1.0 / 1_099_511_627_776.0;
fn stored_length_interval(bits: u16) -> (f64, f64) {
    let value = f64::from(half::f16::from_bits(bits).to_f32());
    let previous = if bits == 0 {
        0.0
    } else {
        f64::from(half::f16::from_bits(bits - 1).to_f32())
    };
    // The next ideal half value beyond maximum finite is 65536; using it
    // avoids infinity while conservatively enclosing every accepted input.
    let next = if bits == 0x7bff {
        65536.0
    } else {
        f64::from(half::f16::from_bits(bits + 1).to_f32())
    };
    let lower = ((previous + value) * 0.5 * (1.0 - REDUCTION_ALLOWANCE))
        .next_down()
        .max(0.0);
    let upper = ((value + next) * 0.5 * (1.0 + REDUCTION_ALLOWANCE)).next_up();
    (lower, upper)
}
fn query_length(query: &Vector) -> (f64, (f64, f64)) {
    let (mut sum, mut lower, mut upper) = (0.0_f64, 0.0_f64, 0.0_f64);
    for &x in query {
        let x = f64::from(x);
        // A square of a finite FP32 input is exact in FP64, including subnormals.
        let square = x * x;
        sum = x.mul_add(x, sum);
        lower = (lower + square).next_down().max(0.0);
        upper = (upper + square).next_up();
    }
    (
        sum.sqrt(),
        (lower.sqrt().next_down().max(0.0), upper.sqrt().next_up()),
    )
}
fn multiply(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let products = [a.0 * b.0, a.0 * b.1, a.1 * b.0, a.1 * b.1];
    (
        products
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min)
            .next_down(),
        products
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max)
            .next_up(),
    )
}
fn dot_interval(cosine: (f64, f64), bits: u16, query: (f64, f64)) -> Result<(f64, f64), Error> {
    let row = stored_length_interval(bits);
    let scaled = multiply(multiply(cosine, row), query);
    // Bridge the FP64 normalized-dot truth to the original unnormalized
    // FP64 reduction, including cancellation. This is an absolute error bound.
    let padding = ((row.1 * query.1).next_up() * REDUCTION_ALLOWANCE).next_up();
    let result = (
        (scaled.0 - padding).next_down(),
        (scaled.1 + padding).next_up(),
    );
    if !result.0.is_finite() || !result.1.is_finite() || result.0 > result.1 {
        return Err(Error::CertificateInvalid);
    }
    Ok(result)
}
impl Index {
    /// Searches by approximate dot(query, original row), using stored lengths
    /// during the full primary scan and residual refinement. Options and query
    /// validation match `search`; this method does not change cosine search.
    /// Candidate selection is approximate; intervals certify scores, not top-k.
    ///
    /// Stored lengths are FP16 with a fixed 2^-24 step below 2^-14. Lengths
    /// below about 3e-8 are stored as zero, so those rows score zero and tie in
    /// row-ID order; below about 1e-5 the rounding exceeds 0.5% of the length,
    /// so such short rows may be misordered among themselves. Intervals still
    /// enclose the true dot product.
    pub fn search_dot_product(
        &self,
        query: &Vector,
        options: SearchOptions,
    ) -> Result<DotProductResult, Error> {
        // Validation happens in `select` before the query norm is trusted.
        let (budget, selected) = self.select::<DotProduct>(query, options)?;
        let (query_norm, query_norm_interval) = query_length(query);
        let scale = FixedPointScorer::new().metadata().comparison_scale() as f64 * 16777216.0;
        let mut hits = Vec::with_capacity(selected.len());
        for Selected {
            key,
            primary,
            raw,
            row,
            segment,
            local,
        } in selected
        {
            let s = &self.data.segments[segment];
            let bits = s.magnitudes[local as usize];
            let cosine = s
                .certificate
                .interval(self.data.binding(segment), row, primary, raw)?;
            hits.push(DotProductHit {
                row: RowId(row),
                segment: segment as u32,
                score: (key as f64 / scale) * query_norm,
                interval: dot_interval(cosine, bits, query_norm_interval)?,
                magnitude_bits: bits,
            });
        }
        Ok(DotProductResult {
            generation: self.generation(),
            candidate_budget: budget,
            rows_scanned: self.len(),
            rows_refined: budget as u64,
            hits,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_finite_half_has_exact_units_and_conservative_rounding_cell() {
        for bits in 0..=0x7bff {
            let value = f64::from(half::f16::from_bits(bits).to_f32());
            assert_eq!(magnitude_units(bits) as f64, value * 16777216.0);
            let (lo, hi) = stored_length_interval(bits);
            assert!(lo <= value && value <= hi);
            assert!(lo >= 0.0 && hi.is_finite());
            let previous = if bits == 0 {
                0.0
            } else {
                f64::from(half::f16::from_bits(bits - 1).to_f32())
            };
            let next = if bits == 0x7bff {
                65536.0
            } else {
                f64::from(half::f16::from_bits(bits + 1).to_f32())
            };
            assert!(lo <= (previous + value) * 0.5 && hi >= (value + next) * 0.5);
            for raw in [i64::MIN, i64::MAX] {
                assert!(i128::from(raw).checked_mul(magnitude_units(bits)).is_some());
            }
        }
        assert!(stored_length_interval(0).1 >= 2.0_f64.powi(-25));
    }
    #[test]
    fn scaled_intervals_cover_signs_underflow_extremes_and_cancellation() {
        for value in [1e-12_f32, 1e-10, 1e-7, 1.0, 1.0004883, 14.0, 65504.0] {
            let bits = half::f16::from_f32(value).to_bits();
            for scale in [1e-12_f32, 1.0, 65504.0] {
                let mut query = [0.0; 768];
                query[0] = scale;
                let (_, q) = query_length(&query);
                for cosine in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                    let interval = dot_interval((cosine, cosine), bits, q).unwrap();
                    let truth = cosine * f64::from(value) * f64::from(scale);
                    assert!(interval.0 <= truth && truth <= interval.1);
                }
            }
        }
    }
}
