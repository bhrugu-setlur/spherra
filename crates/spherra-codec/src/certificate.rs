use core::{cmp::Ordering, fmt};
use std::sync::Arc;

use spherra_domain::DIMENSION;

use crate::scorer::{
    ScoreKind, ScoreProvenance, next_down, next_up, transform_normalized_fp64, upward_add,
    upward_mul, upward_sqrt,
};
use crate::{
    DirectCode, FixedPointScore, FixedPointScorer, Pq96Code, Pq96Codebook, PreparedCandidate,
    PreparedScorerQuery, QuantizerTable, ScorerError, TransformPlan, normalize_fp64,
};

const CERTIFICATE_BLOCK_ID_LEN: usize = 32;

/// Caller-supplied label for an in-memory certificate block.
///
/// This value is not an authentication proof: Task 6 has no checked durable
/// reader yet, so callers can reproduce any label. A future block/manifest
/// reader is the sole authority that may authenticate a durable identity and
/// physical row count. The certificate API instead uses a private per-instance
/// capability to keep a successful in-memory enumeration from being confused
/// with a different enumeration that reused this label.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CertificateBlockId([u8; CERTIFICATE_BLOCK_ID_LEN]);

impl CertificateBlockId {
    pub const fn from_bytes(bytes: [u8; CERTIFICATE_BLOCK_ID_LEN]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(self) -> [u8; CERTIFICATE_BLOCK_ID_LEN] {
        self.0
    }
}

/// An unforgeable capability allocated only after an exhaustive in-memory
/// enumeration has passed its count and contiguous-row checks.
///
/// The non-zero field ensures every `Arc` allocation has a distinct object;
/// equality is always `Arc::ptr_eq`, never the caller-supplied block label.
#[derive(Debug)]
struct ExhaustiveBlockCapability {
    _nonzero: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct CertificateRow<'a> {
    row: u32,
    original: &'a [f32; DIMENSION],
    primary: &'a DirectCode,
    residual: &'a Pq96Code,
}

impl<'a> CertificateRow<'a> {
    pub const fn new(
        row: u32,
        original: &'a [f32; DIMENSION],
        primary: &'a DirectCode,
        residual: &'a Pq96Code,
    ) -> Self {
        Self {
            row,
            original,
            primary,
            residual,
        }
    }

    pub const fn row(self) -> u32 {
        self.row
    }
}

/// A block-owning, checked input to exhaustive certificate construction.
///
/// `from_rows` collects the supplied storage rows, validates that the caller's
/// declared count is self-consistent, and rejects duplicate, skipped, or
/// out-of-range row ordinals. On success it creates an opaque capability that
/// binds every candidate and certificate to this exact enumeration. It cannot
/// authenticate a durable manifest by itself: only a future checked
/// block/manifest reader can vouch for an on-disk identity and physical count.
#[derive(Debug)]
pub struct ExhaustiveBlock<'a> {
    identity: CertificateBlockId,
    capability: Arc<ExhaustiveBlockCapability>,
    rows: Box<[CertificateRow<'a>]>,
}

impl<'a> ExhaustiveBlock<'a> {
    pub fn from_rows<I>(
        identity: CertificateBlockId,
        expected_row_count: u32,
        rows: I,
    ) -> Result<Self, CertificateError>
    where
        I: IntoIterator<Item = CertificateRow<'a>>,
    {
        if expected_row_count == 0 {
            return Err(CertificateError::EmptyBlock);
        }

        let mut rows: Vec<_> = rows.into_iter().collect();
        let expected = expected_row_count as usize;
        if rows.len() != expected {
            return Err(CertificateError::RowCountMismatch {
                expected: expected_row_count,
                actual: u32::try_from(rows.len()).unwrap_or(u32::MAX),
            });
        }

        rows.sort_by(|left, right| left.row.cmp(&right.row));
        for (expected_row, row) in rows.iter().enumerate() {
            let expected_row = expected_row as u32;
            if row.row >= expected_row_count {
                return Err(CertificateError::RowOutOfRange {
                    row: row.row,
                    expected_row_count,
                });
            }
            match row.row.cmp(&expected_row) {
                Ordering::Equal => {}
                Ordering::Less => {
                    return Err(CertificateError::DuplicateRow { row: row.row });
                }
                Ordering::Greater => {
                    return Err(CertificateError::MissingRow { expected_row });
                }
            }
        }

        Ok(Self {
            identity,
            capability: Arc::new(ExhaustiveBlockCapability { _nonzero: 1 }),
            rows: rows.into_boxed_slice(),
        })
    }

    pub const fn identity(&self) -> CertificateBlockId {
        self.identity
    }

    pub fn row_count(&self) -> u32 {
        self.rows.len() as u32
    }

    pub fn candidate(&self, row: u32) -> Result<CertificateBlockCandidate<'_>, CertificateError> {
        let row = self
            .rows
            .get(row as usize)
            .ok_or(CertificateError::RowOutOfRange {
                row,
                expected_row_count: self.row_count(),
            })?;
        Ok(CertificateBlockCandidate {
            block_identity: self.identity,
            capability: Arc::clone(&self.capability),
            row: row.row,
            primary: row.primary,
            residual: row.residual,
        })
    }
}

/// An actual row minted by [`ExhaustiveBlock`], not caller-supplied codes.
#[derive(Clone, Debug)]
pub struct CertificateBlockCandidate<'a> {
    block_identity: CertificateBlockId,
    capability: Arc<ExhaustiveBlockCapability>,
    row: u32,
    primary: &'a DirectCode,
    residual: &'a Pq96Code,
}

impl CertificateBlockCandidate<'_> {
    pub const fn block_identity(&self) -> CertificateBlockId {
        self.block_identity
    }

    pub const fn row(&self) -> u32 {
        self.row
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScoreBounds {
    pub lower: f64,
    pub upper: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ErrorCertificate {
    max_reconstruction_l2_error: f64,
    eta_transform_dot: f64,
    query_norm_upper: f64,
    eta_serving_score: f64,
    epsilon: f64,
}

impl ErrorCertificate {
    pub const fn max_reconstruction_l2_error(self) -> f64 {
        self.max_reconstruction_l2_error
    }

    pub const fn eta_transform_dot(self) -> f64 {
        self.eta_transform_dot
    }

    pub const fn query_norm_upper(self) -> f64 {
        self.query_norm_upper
    }

    pub const fn eta_serving_score(self) -> f64 {
        self.eta_serving_score
    }

    pub const fn epsilon(self) -> f64 {
        self.epsilon
    }

    fn bounds(self, score: FixedPointScore) -> ScoreBounds {
        let score = score.as_f64();
        ScoreBounds {
            lower: next_down(score - self.epsilon),
            upper: next_up(score + self.epsilon),
        }
    }
}

/// A score that has been bound to one certified physical block and one
/// representation identity. It can be converted to a numeric value for
/// ranking, but only its originating certificate can turn it into pruning
/// bounds.
#[derive(Clone, Debug)]
pub struct CertifiedBlockScore {
    score: FixedPointScore,
    block_identity: CertificateBlockId,
    capability: Arc<ExhaustiveBlockCapability>,
    row: u32,
}

impl CertifiedBlockScore {
    pub const fn block_identity(&self) -> CertificateBlockId {
        self.block_identity
    }

    pub const fn row(&self) -> u32 {
        self.row
    }

    pub const fn raw(&self) -> i64 {
        self.score.raw()
    }

    pub fn as_f64(&self) -> f64 {
        self.score.as_f64()
    }
}

#[derive(Clone, Debug)]
pub struct BlockCertificate {
    block_identity: CertificateBlockId,
    capability: Arc<ExhaustiveBlockCapability>,
    provenance: ScoreProvenance,
    primary: ErrorCertificate,
    refined: ErrorCertificate,
}

impl BlockCertificate {
    pub const fn block_identity(&self) -> CertificateBlockId {
        self.block_identity
    }

    pub const fn provenance(&self) -> ScoreProvenance {
        self.provenance
    }

    pub const fn primary(&self) -> ErrorCertificate {
        self.primary
    }

    pub const fn refined(&self) -> ErrorCertificate {
        self.refined
    }

    pub fn score_primary(
        &self,
        scorer: &FixedPointScorer,
        query: &PreparedScorerQuery,
        candidate: &CertificateBlockCandidate<'_>,
    ) -> Result<CertifiedBlockScore, CertificateError> {
        self.validate_candidate(query, candidate)?;
        let score = scorer.score_primary(query, candidate.primary);
        Ok(CertifiedBlockScore {
            score,
            block_identity: self.block_identity,
            capability: Arc::clone(&self.capability),
            row: candidate.row,
        })
    }

    pub fn score_refined(
        &self,
        scorer: &FixedPointScorer,
        query: &PreparedScorerQuery,
        candidate: &CertificateBlockCandidate<'_>,
        prepared: &PreparedCandidate,
    ) -> Result<CertifiedBlockScore, CertificateError> {
        self.validate_candidate(query, candidate)?;
        self.validate_prepared_candidate(candidate, prepared)?;
        let score = scorer
            .score_prepared_candidate(query, candidate.primary, prepared)
            .map_err(CertificateError::PreparedCandidateRejected)?;
        Ok(CertifiedBlockScore {
            score,
            block_identity: self.block_identity,
            capability: Arc::clone(&self.capability),
            row: candidate.row,
        })
    }

    pub fn primary_bounds(
        &self,
        score: CertifiedBlockScore,
    ) -> Result<ScoreBounds, CertificateError> {
        self.validate_certified_score(&score, ScoreKind::Primary)?;
        Ok(self.primary.bounds(score.score))
    }

    pub fn refined_bounds(
        &self,
        score: CertifiedBlockScore,
    ) -> Result<ScoreBounds, CertificateError> {
        self.validate_certified_score(&score, ScoreKind::Refined)?;
        Ok(self.refined.bounds(score.score))
    }

    fn validate_candidate(
        &self,
        query: &PreparedScorerQuery,
        candidate: &CertificateBlockCandidate<'_>,
    ) -> Result<(), CertificateError> {
        if !Arc::ptr_eq(&candidate.capability, &self.capability) {
            return Err(CertificateError::BlockCapabilityMismatch);
        }
        if query.provenance() != self.provenance {
            return Err(CertificateError::ScoreProvenanceMismatch);
        }
        Ok(())
    }

    /// Binds the Task 5 once-loaded residual to the checked certificate row.
    /// This compares the retained code with the code already held by the
    /// in-memory certificate input; it does not grant the primary path a
    /// residual capability and does not reopen residual storage.
    fn validate_prepared_candidate(
        &self,
        candidate: &CertificateBlockCandidate<'_>,
        prepared: &PreparedCandidate,
    ) -> Result<(), CertificateError> {
        if prepared.row() != candidate.row {
            return Err(CertificateError::PreparedCandidateRowMismatch {
                expected: candidate.row,
                actual: prepared.row(),
            });
        }
        if prepared.residual_code() != candidate.residual {
            return Err(CertificateError::PreparedCandidateResidualMismatch { row: candidate.row });
        }
        Ok(())
    }

    fn validate_certified_score(
        &self,
        score: &CertifiedBlockScore,
        expected_kind: ScoreKind,
    ) -> Result<(), CertificateError> {
        if !Arc::ptr_eq(&score.capability, &self.capability) {
            return Err(CertificateError::BlockCapabilityMismatch);
        }
        if score.score.provenance() != self.provenance {
            return Err(CertificateError::ScoreProvenanceMismatch);
        }
        if score.score.kind() != expected_kind {
            return Err(CertificateError::ScoreKindMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CertificateError {
    EmptyBlock,
    RowCountMismatch { expected: u32, actual: u32 },
    RowOutOfRange { row: u32, expected_row_count: u32 },
    DuplicateRow { row: u32 },
    MissingRow { expected_row: u32 },
    BlockCapabilityMismatch,
    ScoreProvenanceMismatch,
    ScoreKindMismatch,
    PreparedCandidateRowMismatch { expected: u32, actual: u32 },
    PreparedCandidateResidualMismatch { row: u32 },
    PreparedCandidateRejected(ScorerError),
    InvalidOriginal { row: u32, source: ScorerError },
    NonFiniteReconstruction { row: u32, coordinate: usize },
}

impl fmt::Display for CertificateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyBlock => formatter.write_str(
                "an exhaustive certificate requires at least one original vector from its block",
            ),
            Self::RowCountMismatch { expected, actual } => write!(
                formatter,
                "certificate block expected {expected} rows but received {actual}",
            ),
            Self::RowOutOfRange {
                row,
                expected_row_count,
            } => write!(
                formatter,
                "certificate row {row} is outside the declared 0..{expected_row_count} block range",
            ),
            Self::DuplicateRow { row } => {
                write!(
                    formatter,
                    "certificate block contains row {row} more than once"
                )
            }
            Self::MissingRow { expected_row } => {
                write!(formatter, "certificate block is missing row {expected_row}")
            }
            Self::BlockCapabilityMismatch => formatter.write_str(
                "certificate score or candidate belongs to another checked block enumeration",
            ),
            Self::ScoreProvenanceMismatch => formatter.write_str(
                "certificate score uses a different transform, codec, or scorer identity",
            ),
            Self::ScoreKindMismatch => formatter
                .write_str("primary and refined certificates cannot be used interchangeably"),
            Self::PreparedCandidateRowMismatch { expected, actual } => write!(
                formatter,
                "prepared candidate row {actual} does not match certified row {expected}",
            ),
            Self::PreparedCandidateResidualMismatch { row } => write!(
                formatter,
                "prepared candidate residual code does not match certified row {row}",
            ),
            Self::PreparedCandidateRejected(source) => {
                write!(formatter, "prepared candidate cannot be scored: {source}")
            }
            Self::InvalidOriginal { row, source } => {
                write!(
                    formatter,
                    "certificate original row {row} is invalid: {source}"
                )
            }
            Self::NonFiniteReconstruction { row, coordinate } => write!(
                formatter,
                "certificate reconstruction for row {row} is non-finite at coordinate {coordinate}",
            ),
        }
    }
}

impl std::error::Error for CertificateError {}

/// Computes primary and refined certificates from a complete, checked physical
/// block while its FP32 originals are still available.
pub fn build_exhaustive_certificate(
    scorer: &FixedPointScorer,
    plan: &TransformPlan,
    quantizer: &QuantizerTable,
    codebook: &Pq96Codebook,
    block: &ExhaustiveBlock<'_>,
) -> Result<BlockCertificate, CertificateError> {
    let expected_provenance = ScoreProvenance::new(
        *plan.identity(),
        *quantizer.identity(),
        *codebook.codebook_id(),
        scorer.metadata().scorer_version(),
    );
    let delta_t = scorer.metadata().transform_delta();
    let query_norm_upper = scorer.metadata().query_norm_upper();
    let mut maximum_primary_error = 0.0_f64;
    let mut maximum_refined_error = 0.0_f64;
    let mut maximum_primary_norm = 0.0_f64;
    let mut maximum_residual_norm = 0.0_f64;

    for row in &block.rows {
        let normalized =
            normalize_fp64(row.original).map_err(|source| CertificateError::InvalidOriginal {
                row: row.row,
                source,
            })?;
        let transformed = transform_normalized_fp64(plan, &normalized).map_err(|source| {
            CertificateError::InvalidOriginal {
                row: row.row,
                source,
            }
        })?;
        let primary = quantizer.decode(row.primary);
        let residual = codebook.decode(row.residual);
        if let Some(coordinate) = primary
            .iter()
            .chain(residual.iter())
            .position(|value| !value.is_finite())
        {
            return Err(CertificateError::NonFiniteReconstruction {
                row: row.row,
                coordinate: coordinate % DIMENSION,
            });
        }

        let primary_error = upward_add(outward_l2_primary(&transformed, &primary), delta_t);
        let refined_error = upward_add(
            outward_l2_refined(&transformed, &primary, &residual),
            delta_t,
        );
        maximum_primary_error = maximum_primary_error.max(primary_error);
        maximum_refined_error = maximum_refined_error.max(refined_error);
        maximum_primary_norm = maximum_primary_norm.max(outward_l2_norm(&primary));
        maximum_residual_norm = maximum_residual_norm.max(outward_l2_norm(&residual));
    }

    let eta_transform_dot = upward_add(upward_mul(2.0, delta_t), upward_mul(delta_t, delta_t));
    let primary_serving_error = scorer.primary_serving_error(maximum_primary_norm);
    let refined_serving_error =
        scorer.refined_serving_error(maximum_primary_norm, maximum_residual_norm);
    Ok(BlockCertificate {
        block_identity: block.identity,
        capability: Arc::clone(&block.capability),
        provenance: expected_provenance,
        primary: ErrorCertificate {
            max_reconstruction_l2_error: maximum_primary_error,
            eta_transform_dot,
            query_norm_upper,
            eta_serving_score: primary_serving_error,
            epsilon: epsilon(
                eta_transform_dot,
                query_norm_upper,
                maximum_primary_error,
                primary_serving_error,
            ),
        },
        refined: ErrorCertificate {
            max_reconstruction_l2_error: maximum_refined_error,
            eta_transform_dot,
            query_norm_upper,
            eta_serving_score: refined_serving_error,
            epsilon: epsilon(
                eta_transform_dot,
                query_norm_upper,
                maximum_refined_error,
                refined_serving_error,
            ),
        },
    })
}

fn epsilon(
    eta_transform_dot: f64,
    query_norm_upper: f64,
    maximum_reconstruction_l2_error: f64,
    eta_serving_score: f64,
) -> f64 {
    upward_add(
        upward_add(
            eta_transform_dot,
            upward_mul(query_norm_upper, maximum_reconstruction_l2_error),
        ),
        eta_serving_score,
    )
}

fn outward_l2_primary(transformed: &[f32; DIMENSION], primary: &[f32; DIMENSION]) -> f64 {
    let mut squared_sum = 0.0;
    for coordinate in 0..DIMENSION {
        let absolute_difference = outward_absolute_difference(
            f64::from(transformed[coordinate]),
            f64::from(primary[coordinate]),
        );
        squared_sum = upward_add(
            squared_sum,
            upward_mul(absolute_difference, absolute_difference),
        );
    }
    upward_sqrt(squared_sum)
}

/// The scalar serving contract treats `p + e` as the exact split sum of its
/// finite FP32 terms in the FP64 reduction. Do not materialize a rounded FP32
/// addition here: doing so defines a different scorer and leaves an unmodeled
/// reconstruction-addition error.
fn outward_l2_refined(
    transformed: &[f32; DIMENSION],
    primary: &[f32; DIMENSION],
    residual: &[f32; DIMENSION],
) -> f64 {
    let mut squared_sum = 0.0;
    for coordinate in 0..DIMENSION {
        let absolute_difference = outward_absolute_refined_difference(
            f64::from(transformed[coordinate]),
            f64::from(primary[coordinate]),
            f64::from(residual[coordinate]),
        );
        squared_sum = upward_add(
            squared_sum,
            upward_mul(absolute_difference, absolute_difference),
        );
    }
    upward_sqrt(squared_sum)
}

/// Returns an upward endpoint for `|transformed - primary|`.
///
/// Every input is a finite FP32 value lifted exactly into FP64.  Therefore the
/// additions below cannot overflow or underflow in FP64, and Knuth's `two_sum`
/// expansion gives the exact real difference as `difference + roundoff`.
/// Bounding the sum of both absolute expansion terms before squaring prevents
/// a nearest-FP64 subtraction from shrinking a certificate endpoint.
fn outward_absolute_difference(transformed: f64, primary: f64) -> f64 {
    let (difference, roundoff) = two_sum(transformed, -primary);
    upward_add(difference.abs(), roundoff.abs())
}

/// Returns an upward endpoint for `|transformed - (primary + residual)|`.
///
/// The two error-free expansions establish, in real arithmetic,
/// `primary + residual = reconstruction + reconstruction_roundoff` and
/// `transformed - reconstruction = difference + difference_roundoff`.
/// Consequently the requested difference is exactly
/// `difference + difference_roundoff - reconstruction_roundoff`; its absolute
/// value is at most the outward sum of the three expansion magnitudes.
fn outward_absolute_refined_difference(transformed: f64, primary: f64, residual: f64) -> f64 {
    let (reconstruction, reconstruction_roundoff) = two_sum(primary, residual);
    let (difference, difference_roundoff) = two_sum(transformed, -reconstruction);
    let upper = upward_add(difference.abs(), difference_roundoff.abs());
    upward_add(upper, reconstruction_roundoff.abs())
}

/// Error-free sum of two finite FP64 values whose exact sum is in FP64 range.
///
/// This is Knuth's `two_sum`: it returns `(sum, roundoff)` satisfying the
/// exact-real identity `left + right == sum + roundoff`. The callers provide
/// only finite FP32 values promoted to FP64, so their sums are far below the
/// FP64 overflow threshold and any nonzero residual is representable without
/// FP64 underflow.
fn two_sum(left: f64, right: f64) -> (f64, f64) {
    let sum = left + right;
    let right_virtual = sum - left;
    let left_roundoff = left - (sum - right_virtual);
    let right_roundoff = right - right_virtual;
    (sum, left_roundoff + right_roundoff)
}

fn outward_l2_norm(values: &[f32; DIMENSION]) -> f64 {
    let mut squared_sum = 0.0;
    for value in values {
        let value = f64::from(*value).abs();
        squared_sum = upward_add(squared_sum, upward_mul(value, value));
    }
    upward_sqrt(squared_sum)
}

#[cfg(test)]
mod tests {
    use super::{outward_l2_primary, outward_l2_refined};
    use spherra_domain::DIMENSION;

    #[test]
    fn refined_reconstruction_interval_preserves_an_exact_lost_dyadic_addend() {
        let mut transformed = [0.0; DIMENSION];
        let mut primary = [0.0; DIMENSION];
        let mut residual = [0.0; DIMENSION];
        transformed[0] = 1.0;
        primary[0] = 1.0;
        residual[0] = 2.0_f32.powi(-80);

        // Exactly: 1 - (1 + 2^-80) = -2^-80.  The operands are FP32
        // dyadics, so this is an independent exact-rational oracle rather
        // than a sampled floating-point estimate.
        let exact_distance = 2.0_f64.powi(-80);
        let upper = outward_l2_refined(&transformed, &primary, &residual);

        assert!(
            upper >= exact_distance,
            "the reconstruction interval must retain the exact split addend lost by nearest f64 addition"
        );
    }

    #[test]
    fn primary_reconstruction_interval_preserves_an_exact_exponent_gap() {
        let mut transformed = [0.0; DIMENSION];
        let mut primary = [0.0; DIMENSION];
        transformed[0] = 1.0;
        primary[0] = -2.0_f32.powi(-80);

        // The exact dyadic distance is 1 + 2^-80, strictly greater than one.
        // A binary64 nearest subtraction alone rounds it down to one.
        let upper = outward_l2_primary(&transformed, &primary);

        assert!(
            upper > 1.0,
            "the primary interval must retain an exponent-gap subtraction remainder"
        );
    }

    #[test]
    fn refined_reconstruction_interval_covers_exact_primary_residual_cancellation() {
        let transformed = [0.0; DIMENSION];
        let mut primary = [0.0; DIMENSION];
        let mut residual = [0.0; DIMENSION];
        primary[0] = 1.0;
        // This FP32 dyadic is exactly -1 + 2^-24.
        residual[0] = f32::from_bits(0xbf7f_ffff);

        let exact_distance = 2.0_f64.powi(-24);
        let upper = outward_l2_refined(&transformed, &primary, &residual);

        assert!(
            upper >= exact_distance,
            "the reconstruction interval must retain a primary/residual cancellation remainder"
        );
    }
}
