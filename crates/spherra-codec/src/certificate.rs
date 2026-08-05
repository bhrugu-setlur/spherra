use core::{cmp::Ordering, fmt};

use spherra_domain::DIMENSION;

use crate::scorer::{
    ScoreKind, ScoreProvenance, next_down, next_up, transform_normalized_fp64, upward_add,
    upward_mul, upward_sqrt,
};
use crate::{
    DirectCode, FixedPointScore, FixedPointScorer, Pq96Code, Pq96Codebook, PreparedScorerQuery,
    QuantizerTable, ScorerError, TransformPlan, normalize_fp64,
};

const CERTIFICATE_BLOCK_ID_LEN: usize = 32;

/// Stable identity supplied by the immutable block/segment owner.
///
/// The codec cannot discover omitted storage rows on its own. The owner must
/// therefore supply the durable block identity and expected physical row count
/// when it creates an [`ExhaustiveBlock`]. Certificate construction verifies
/// the exact contiguous coverage `0..row_count` before considering any vector.
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
/// manifest count is exact, and rejects duplicate, skipped, or out-of-range
/// row ordinals. The resulting capability is also the sole source of rows
/// accepted by the public certificate-bound API.
#[derive(Debug)]
pub struct ExhaustiveBlock<'a> {
    identity: CertificateBlockId,
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
            row: row.row,
            primary: row.primary,
            residual: row.residual,
        })
    }
}

/// An actual row minted by [`ExhaustiveBlock`], not caller-supplied codes.
#[derive(Clone, Copy, Debug)]
pub struct CertificateBlockCandidate<'a> {
    block_identity: CertificateBlockId,
    row: u32,
    primary: &'a DirectCode,
    residual: &'a Pq96Code,
}

impl CertificateBlockCandidate<'_> {
    pub const fn row(self) -> u32 {
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CertifiedBlockScore {
    score: FixedPointScore,
    block_identity: CertificateBlockId,
    row: u32,
}

impl CertifiedBlockScore {
    pub const fn row(self) -> u32 {
        self.row
    }

    pub const fn raw(self) -> i64 {
        self.score.raw()
    }

    pub fn as_f64(self) -> f64 {
        self.score.as_f64()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockCertificate {
    block_identity: CertificateBlockId,
    provenance: ScoreProvenance,
    primary: ErrorCertificate,
    refined: ErrorCertificate,
}

impl BlockCertificate {
    pub const fn block_identity(self) -> CertificateBlockId {
        self.block_identity
    }

    pub const fn provenance(self) -> ScoreProvenance {
        self.provenance
    }

    pub const fn primary(self) -> ErrorCertificate {
        self.primary
    }

    pub const fn refined(self) -> ErrorCertificate {
        self.refined
    }

    pub fn score_primary(
        self,
        scorer: &FixedPointScorer,
        query: &PreparedScorerQuery,
        candidate: CertificateBlockCandidate<'_>,
    ) -> Result<CertifiedBlockScore, CertificateError> {
        self.validate_candidate(query, candidate)?;
        let score = scorer.score_primary(query, candidate.primary);
        Ok(CertifiedBlockScore {
            score,
            block_identity: self.block_identity,
            row: candidate.row,
        })
    }

    pub fn score_refined(
        self,
        scorer: &FixedPointScorer,
        query: &PreparedScorerQuery,
        candidate: CertificateBlockCandidate<'_>,
    ) -> Result<CertifiedBlockScore, CertificateError> {
        self.validate_candidate(query, candidate)?;
        let score = scorer.score_refined(query, candidate.primary, candidate.residual);
        Ok(CertifiedBlockScore {
            score,
            block_identity: self.block_identity,
            row: candidate.row,
        })
    }

    pub fn primary_bounds(
        self,
        score: CertifiedBlockScore,
    ) -> Result<ScoreBounds, CertificateError> {
        self.validate_certified_score(score, ScoreKind::Primary)?;
        Ok(self.primary.bounds(score.score))
    }

    pub fn refined_bounds(
        self,
        score: CertifiedBlockScore,
    ) -> Result<ScoreBounds, CertificateError> {
        self.validate_certified_score(score, ScoreKind::Refined)?;
        Ok(self.refined.bounds(score.score))
    }

    fn validate_candidate(
        self,
        query: &PreparedScorerQuery,
        candidate: CertificateBlockCandidate<'_>,
    ) -> Result<(), CertificateError> {
        if candidate.block_identity != self.block_identity {
            return Err(CertificateError::BlockIdentityMismatch);
        }
        if query.provenance() != self.provenance {
            return Err(CertificateError::ScoreProvenanceMismatch);
        }
        Ok(())
    }

    fn validate_certified_score(
        self,
        score: CertifiedBlockScore,
        expected_kind: ScoreKind,
    ) -> Result<(), CertificateError> {
        if score.block_identity != self.block_identity {
            return Err(CertificateError::BlockIdentityMismatch);
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
    BlockIdentityMismatch,
    ScoreProvenanceMismatch,
    ScoreKindMismatch,
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
            Self::BlockIdentityMismatch => formatter
                .write_str("certificate score or candidate belongs to another physical block"),
            Self::ScoreProvenanceMismatch => formatter.write_str(
                "certificate score uses a different transform, codec, or scorer identity",
            ),
            Self::ScoreKindMismatch => formatter
                .write_str("primary and refined certificates cannot be used interchangeably"),
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
    let query_norm_upper = upward_add(1.0, delta_t);
    let primary_serving_error = scorer.primary_serving_error(maximum_primary_norm);
    let refined_serving_error =
        scorer.refined_serving_error(maximum_primary_norm, maximum_residual_norm);
    Ok(BlockCertificate {
        block_identity: block.identity,
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
        let difference = f64::from(transformed[coordinate]) - f64::from(primary[coordinate]);
        squared_sum = upward_add(squared_sum, upward_mul(difference.abs(), difference.abs()));
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
        let reconstruction = f64::from(primary[coordinate]) + f64::from(residual[coordinate]);
        let difference = f64::from(transformed[coordinate]) - reconstruction;
        squared_sum = upward_add(squared_sum, upward_mul(difference.abs(), difference.abs()));
    }
    upward_sqrt(squared_sum)
}

fn outward_l2_norm(values: &[f32; DIMENSION]) -> f64 {
    let mut squared_sum = 0.0;
    for value in values {
        let value = f64::from(*value).abs();
        squared_sum = upward_add(squared_sum, upward_mul(value, value));
    }
    upward_sqrt(squared_sum)
}
