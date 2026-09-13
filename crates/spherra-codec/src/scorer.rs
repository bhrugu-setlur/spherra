use core::{array, fmt};

use spherra_domain::DIMENSION;

use crate::transform::{KernelInputDirection, transform_kernel_input};
use crate::{DirectCode, Pq96Code, Pq96Codebook, PreparedCandidate, QuantizerTable, TransformPlan};

const PRIMARY_CODES_PER_COORDINATE: usize = 16;
const TRANSFORM_ROUNDS: usize = 2;
const HADAMARD_STAGES: usize = 7;
const HADAMARD_BLOCK_LEN: usize = 128;
const FP32_UNIT_ROUNDOFF: f64 = 1.0 / 16_777_216.0;
const FP64_UNIT_ROUNDOFF: f64 = 1.0 / 9_007_199_254_740_992.0;
const FIXED_POINT_SCORER_VERSION: u32 = 1;
const FRACTIONAL_BITS: u32 = 24;
const COMPARISON_SCALE: i64 = 1_i64 << FRACTIONAL_BITS;
const PRIMARY_TERMS: usize = DIMENSION;
const REFINED_TERMS: usize = DIMENSION + Pq96Code::SUBQUANTIZERS;
const MAX_ABSOLUTE_TABLE_ENTRY: i64 = i64::MAX / REFINED_TERMS as i64;

#[derive(Clone, Copy, Debug, PartialEq)]
struct TransformErrorBudget {
    normalization_l2_error: f64,
    kernel_input_l2_error: f64,
    transform_round_l2_error: f64,
    transform_delta: f64,
    query_norm_upper: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScorerMetadata {
    scorer_version: u32,
    fractional_bits: u32,
    comparison_scale: i64,
    primary_terms: usize,
    refined_terms: usize,
    maximum_absolute_table_entry: i64,
    normalization_l2_error: f64,
    kernel_input_l2_error: f64,
    transform_round_l2_error: f64,
    transform_delta: f64,
    query_norm_upper: f64,
}

impl ScorerMetadata {
    pub const fn scorer_version(self) -> u32 {
        self.scorer_version
    }

    pub const fn fractional_bits(self) -> u32 {
        self.fractional_bits
    }

    pub const fn comparison_scale(self) -> i64 {
        self.comparison_scale
    }

    pub const fn primary_terms(self) -> usize {
        self.primary_terms
    }

    pub const fn refined_terms(self) -> usize {
        self.refined_terms
    }

    pub const fn maximum_absolute_table_entry(self) -> i64 {
        self.maximum_absolute_table_entry
    }

    /// Outward L2 error of the FP64 norm reduction, square root, and divisions
    /// relative to exact-real normalization of the finite FP32 input.
    pub const fn normalization_l2_error(self) -> f64 {
        self.normalization_l2_error
    }

    /// Outward L2 error from exact-real normalization through the one allowed
    /// FP64-to-FP32 kernel-input conversion.
    pub const fn kernel_input_l2_error(self) -> f64 {
        self.kernel_input_l2_error
    }

    /// Outward L2 error of the two scalar transformed-space rounds after the
    /// kernel-input error has been accounted for.
    pub const fn transform_round_l2_error(self) -> f64 {
        self.transform_round_l2_error
    }

    pub const fn transform_delta(self) -> f64 {
        self.transform_delta
    }

    /// Constructive upper bound for the norm of a transformed serving query.
    pub const fn query_norm_upper(self) -> f64 {
        self.query_norm_upper
    }

    /// The proof input used by the checked fixed-point accumulation path.
    pub const fn worst_case_refined_sum(self) -> i64 {
        self.maximum_absolute_table_entry * self.refined_terms as i64
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScorerError {
    NonFiniteInput { coordinate: usize },
    ZeroNorm,
    KernelInputRejected,
    NonFiniteLookup { table: LookupTable, index: usize },
    LookupOutOfRange { table: LookupTable, index: usize },
    NonFiniteCandidateResidual { coordinate: usize },
    CandidateCodebookMismatch,
}

impl fmt::Display for ScorerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteInput { coordinate } => {
                write!(
                    formatter,
                    "scoring input has a non-finite component at coordinate {coordinate}"
                )
            }
            Self::ZeroNorm => formatter.write_str("scoring input has zero norm"),
            Self::KernelInputRejected => formatter.write_str(
                "the FP64-normalized value could not be converted into a reliable transform input",
            ),
            Self::NonFiniteLookup { table, index } => {
                write!(formatter, "{table} lookup entry {index} is non-finite")
            }
            Self::LookupOutOfRange { table, index } => write!(
                formatter,
                "{table} lookup entry {index} exceeds the fixed-point overflow proof"
            ),
            Self::NonFiniteCandidateResidual { coordinate } => write!(
                formatter,
                "prepared candidate residual has a non-finite component at coordinate {coordinate}"
            ),
            Self::CandidateCodebookMismatch => formatter.write_str(
                "prepared candidate was decoded with a different PQ96 codebook than this query",
            ),
        }
    }
}

impl std::error::Error for ScorerError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LookupTable {
    Primary,
    Residual,
}

impl fmt::Display for LookupTable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Primary => formatter.write_str("primary"),
            Self::Residual => formatter.write_str("residual"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoreKind {
    Primary,
    Refined,
}

/// Immutable representation identities carried by every serving score.
///
/// A fixed-point integer is comparable only with scores prepared using the
/// same transform, direct-int4 table, PQ codebook, and scorer version. The
/// certificate API checks this value before it admits a score into a bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScoreProvenance {
    transform_identity: [u8; 32],
    quantizer_identity: [u8; 32],
    codebook_identity: [u8; 32],
    scorer_version: u32,
}

impl ScoreProvenance {
    pub(crate) const fn new(
        transform_identity: [u8; 32],
        quantizer_identity: [u8; 32],
        codebook_identity: [u8; 32],
        scorer_version: u32,
    ) -> Self {
        Self {
            transform_identity,
            quantizer_identity,
            codebook_identity,
            scorer_version,
        }
    }

    pub const fn transform_identity(self) -> [u8; 32] {
        self.transform_identity
    }

    pub const fn quantizer_identity(self) -> [u8; 32] {
        self.quantizer_identity
    }

    pub const fn codebook_identity(self) -> [u8; 32] {
        self.codebook_identity
    }

    pub const fn scorer_version(self) -> u32 {
        self.scorer_version
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixedPointScore {
    raw: i64,
    provenance: ScoreProvenance,
    kind: ScoreKind,
}

impl FixedPointScore {
    pub const fn raw(self) -> i64 {
        self.raw
    }

    pub fn as_f64(self) -> f64 {
        self.raw as f64 / COMPARISON_SCALE as f64
    }

    pub const fn provenance(self) -> ScoreProvenance {
        self.provenance
    }

    pub const fn kind(self) -> ScoreKind {
        self.kind
    }
}

/// Fixed-point query tables built exactly once for a normalized raw query.
///
/// Primary entries are coordinate-by-four-bit-code lookups. Residual entries
/// are subquantizer-by-byte-code lookups, so a refined candidate is accumulated
/// in a fixed coordinate/subquantizer order with 768 + 96 `i64` terms.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LookupScaleMeasurement {
    maximum_primary_lookup_entry: i64,
    maximum_residual_lookup_entry: i64,
}

impl LookupScaleMeasurement {
    pub const fn maximum_primary_lookup_entry(self) -> i64 {
        self.maximum_primary_lookup_entry
    }

    pub const fn maximum_residual_lookup_entry(self) -> i64 {
        self.maximum_residual_lookup_entry
    }

    pub const fn maximum_lookup_entry(self) -> i64 {
        if self.maximum_primary_lookup_entry > self.maximum_residual_lookup_entry {
            self.maximum_primary_lookup_entry
        } else {
            self.maximum_residual_lookup_entry
        }
    }
}

#[derive(Clone, Debug)]
pub struct PreparedScorerQuery {
    transformed: [f32; DIMENSION],
    primary_lookup: Box<[[i64; PRIMARY_CODES_PER_COORDINATE]; DIMENSION]>,
    residual_lookup: Box<[[i64; Pq96Code::CENTROIDS]; Pq96Code::SUBQUANTIZERS]>,
    primary_score_error: f64,
    refined_score_error: f64,
    lookup_scale_measurement: LookupScaleMeasurement,
    provenance: ScoreProvenance,
}

impl PreparedScorerQuery {
    pub const fn transformed(&self) -> &[f32; DIMENSION] {
        &self.transformed
    }

    /// A constructive bound for the scalar transformed-space primary oracle.
    pub const fn primary_score_error(&self) -> f64 {
        self.primary_score_error
    }

    /// A constructive bound for the scalar transformed-space refined oracle.
    pub const fn refined_score_error(&self) -> f64 {
        self.refined_score_error
    }

    /// Measured absolute lookup-entry maxima for this prepared query.
    ///
    /// The values are evidence for choosing the fixed-point scale; the
    /// scorer's independent admissible maximum still proves every 768 + 96
    /// accumulation fits in `i64` for arbitrary checked lookups.
    pub const fn lookup_scale_measurement(&self) -> LookupScaleMeasurement {
        self.lookup_scale_measurement
    }

    pub const fn provenance(&self) -> ScoreProvenance {
        self.provenance
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FixedPointScorer {
    metadata: ScorerMetadata,
}

impl Default for FixedPointScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl FixedPointScorer {
    pub fn new() -> Self {
        let transform_errors = transform_error_budget();
        Self {
            metadata: ScorerMetadata {
                scorer_version: FIXED_POINT_SCORER_VERSION,
                fractional_bits: FRACTIONAL_BITS,
                comparison_scale: COMPARISON_SCALE,
                primary_terms: PRIMARY_TERMS,
                refined_terms: REFINED_TERMS,
                maximum_absolute_table_entry: MAX_ABSOLUTE_TABLE_ENTRY,
                normalization_l2_error: transform_errors.normalization_l2_error,
                kernel_input_l2_error: transform_errors.kernel_input_l2_error,
                transform_round_l2_error: transform_errors.transform_round_l2_error,
                transform_delta: transform_errors.transform_delta,
                query_norm_upper: transform_errors.query_norm_upper,
            },
        }
    }

    pub const fn metadata(self) -> ScorerMetadata {
        self.metadata
    }

    pub fn prepare_query(
        &self,
        plan: &TransformPlan,
        raw_query: &[f32; DIMENSION],
        quantizer: &QuantizerTable,
        codebook: &Pq96Codebook,
    ) -> Result<PreparedScorerQuery, ScorerError> {
        let normalized = normalize_fp64(raw_query)?;
        let transformed = transform_normalized_fp64(plan, &normalized)?;

        let mut primary_maximum_absolute_product = 0.0_f64;
        let mut maximum_primary_lookup_entry = 0_i64;
        let mut primary_lookup = Box::new([[0_i64; PRIMARY_CODES_PER_COORDINATE]; DIMENSION]);
        for coordinate in 0..DIMENSION {
            for code in 0..PRIMARY_CODES_PER_COORDINATE {
                let center = quantizer
                    .center(coordinate, code)
                    .expect("a fixed primary lookup index is valid");
                let product = f64::from(transformed[coordinate]) * f64::from(center);
                primary_maximum_absolute_product =
                    primary_maximum_absolute_product.max(product.abs());
                let entry = quantize_lookup(product, LookupTable::Primary, coordinate * 16 + code)?;
                maximum_primary_lookup_entry =
                    maximum_primary_lookup_entry.max(absolute_lookup_entry(entry));
                primary_lookup[coordinate][code] = entry;
            }
        }

        let mut residual_maximum_absolute_product_sum = 0.0_f64;
        let mut maximum_residual_lookup_entry = 0_i64;
        let mut residual_error = None;
        let residual_lookup = Box::new(array::from_fn(|subquantizer| {
            array::from_fn(|code| {
                let centroid = codebook
                    .centroid(subquantizer, code as u8)
                    .expect("a fixed residual lookup index is valid");
                let start = subquantizer * Pq96Code::SUBVECTOR_DIMENSION;
                let mut product_sum = 0.0;
                let mut absolute_product_sum = 0.0;
                for lane in 0..Pq96Code::SUBVECTOR_DIMENSION {
                    let product = f64::from(transformed[start + lane]) * f64::from(centroid[lane]);
                    product_sum = f64::from(transformed[start + lane])
                        .mul_add(f64::from(centroid[lane]), product_sum);
                    absolute_product_sum = upward_add(absolute_product_sum, product.abs());
                }
                residual_maximum_absolute_product_sum =
                    residual_maximum_absolute_product_sum.max(absolute_product_sum);
                match quantize_lookup(
                    product_sum,
                    LookupTable::Residual,
                    subquantizer * Pq96Code::CENTROIDS + code,
                ) {
                    Ok(value) => {
                        maximum_residual_lookup_entry =
                            maximum_residual_lookup_entry.max(absolute_lookup_entry(value));
                        value
                    }
                    Err(error) => {
                        residual_error = Some(error);
                        0
                    }
                }
            })
        }));
        if let Some(error) = residual_error {
            return Err(error);
        }

        let primary_absolute_sum =
            upward_mul(PRIMARY_TERMS as f64, primary_maximum_absolute_product);
        let residual_absolute_sum = upward_mul(
            Pq96Code::SUBQUANTIZERS as f64,
            residual_maximum_absolute_product_sum,
        );
        Ok(PreparedScorerQuery {
            transformed,
            primary_lookup,
            residual_lookup,
            primary_score_error: prepared_score_error(PRIMARY_TERMS, primary_absolute_sum, 0.0),
            refined_score_error: prepared_score_error(
                REFINED_TERMS,
                primary_absolute_sum,
                residual_absolute_sum,
            ),
            lookup_scale_measurement: LookupScaleMeasurement {
                maximum_primary_lookup_entry,
                maximum_residual_lookup_entry,
            },
            provenance: ScoreProvenance::new(
                *plan.identity(),
                *quantizer.identity(),
                *codebook.codebook_id(),
                self.metadata.scorer_version,
            ),
        })
    }

    pub fn score_primary(
        &self,
        query: &PreparedScorerQuery,
        primary: &DirectCode,
    ) -> FixedPointScore {
        let mut sum = 0_i64;
        for coordinate in 0..DIMENSION {
            let value = query.primary_lookup[coordinate][primary.nibble_at(coordinate) as usize];
            sum = checked_accumulate(sum, value);
        }
        FixedPointScore {
            raw: sum,
            provenance: query.provenance,
            kind: ScoreKind::Primary,
        }
    }

    /// Adds the 96 residual lookup entries to a cached primary score, in the
    /// same order as `score_refined`. The caller must supply the primary raw
    /// score produced for this query and candidate; this method cannot verify
    /// that association. It grants no certificate provenance.
    ///
    /// # Panics
    /// Panics if an invalid cached value causes checked accumulation to overflow.
    /// A primary score produced by this scorer satisfies the accumulation bound.
    pub fn refine_from_primary(
        &self,
        query: &PreparedScorerQuery,
        primary_raw: i64,
        residual: &Pq96Code,
    ) -> i64 {
        let mut sum = primary_raw;
        for (subquantizer, code) in residual.as_bytes().iter().copied().enumerate() {
            sum = checked_accumulate(sum, query.residual_lookup[subquantizer][usize::from(code)]);
        }
        sum
    }

    pub fn score_refined(
        &self,
        query: &PreparedScorerQuery,
        primary: &DirectCode,
        residual: &Pq96Code,
    ) -> FixedPointScore {
        let mut sum = self.score_primary(query, primary).raw();
        for subquantizer in 0..Pq96Code::SUBQUANTIZERS {
            let value =
                query.residual_lookup[subquantizer][residual.as_bytes()[subquantizer] as usize];
            sum = checked_accumulate(sum, value);
        }
        FixedPointScore {
            raw: sum,
            provenance: query.provenance,
            kind: ScoreKind::Refined,
        }
    }

    /// Scores a Task 5 prepared candidate without reopening the residual source.
    ///
    /// The retained decoded residual is reduced as 96 eight-lane terms, matching
    /// the refined `i64` comparison cardinality. The canonical serving meaning
    /// of `p + e` is an exact split sum of the two finite FP32 reconstruction
    /// terms lifted into the FP64 lookup reduction; it is never materialized as
    /// a separately rounded FP32 vector and is never normalized.
    pub fn score_prepared_candidate(
        &self,
        query: &PreparedScorerQuery,
        primary: &DirectCode,
        candidate: &PreparedCandidate,
    ) -> Result<FixedPointScore, ScorerError> {
        if candidate.codebook_identity() != query.provenance.codebook_identity {
            return Err(ScorerError::CandidateCodebookMismatch);
        }
        let mut sum = self.score_primary(query, primary).raw();
        let residual = candidate.decoded_residual();
        for (coordinate, value) in residual.iter().enumerate() {
            if !value.is_finite() {
                return Err(ScorerError::NonFiniteCandidateResidual { coordinate });
            }
        }

        for subquantizer in 0..Pq96Code::SUBQUANTIZERS {
            let start = subquantizer * Pq96Code::SUBVECTOR_DIMENSION;
            let mut product_sum = 0.0;
            for lane in 0..Pq96Code::SUBVECTOR_DIMENSION {
                product_sum = f64::from(query.transformed[start + lane])
                    .mul_add(f64::from(residual[start + lane]), product_sum);
            }
            let value = quantize_lookup(product_sum, LookupTable::Residual, subquantizer)?;
            sum = checked_accumulate(sum, value);
        }
        Ok(FixedPointScore {
            raw: sum,
            provenance: query.provenance,
            kind: ScoreKind::Refined,
        })
    }

    pub(crate) fn primary_serving_error(&self, reconstruction_norm: f64) -> f64 {
        serving_error(
            PRIMARY_TERMS,
            reconstruction_norm,
            0.0,
            self.metadata.query_norm_upper,
        )
    }

    pub(crate) fn refined_serving_error(&self, primary_norm: f64, residual_norm: f64) -> f64 {
        serving_error(
            REFINED_TERMS,
            primary_norm,
            residual_norm,
            self.metadata.query_norm_upper,
        )
    }
}

/// Normalizes a finite raw FP32 input using the required FP64 reduction and
/// component-division order. This is the truth normalization used by serving
/// preparation and certificate construction.
pub fn normalize_fp64(values: &[f32; DIMENSION]) -> Result<[f64; DIMENSION], ScorerError> {
    let mut squared_norm = 0.0_f64;
    for (coordinate, value) in values.iter().copied().enumerate() {
        if !value.is_finite() {
            return Err(ScorerError::NonFiniteInput { coordinate });
        }
        let value = f64::from(value);
        squared_norm = value.mul_add(value, squared_norm);
    }
    if squared_norm == 0.0 {
        return Err(ScorerError::ZeroNorm);
    }
    let norm = squared_norm.sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return Err(ScorerError::ZeroNorm);
    }
    Ok(array::from_fn(|coordinate| {
        f64::from(values[coordinate]) / norm
    }))
}

/// Deterministic FP64 dot reduction used for the original-space truth oracle.
pub fn dot_f64(left: &[f64; DIMENSION], right: &[f64; DIMENSION]) -> f64 {
    left.iter()
        .zip(right)
        .fold(0.0, |sum, (left, right)| left.mul_add(*right, sum))
}

pub(crate) fn transform_normalized_fp64(
    plan: &TransformPlan,
    normalized: &[f64; DIMENSION],
) -> Result<[f32; DIMENSION], ScorerError> {
    let kernel_input = KernelInputDirection::from_normalized_fp64(normalized)
        .ok_or(ScorerError::KernelInputRejected)?;
    Ok(*transform_kernel_input(plan, &kernel_input).as_array())
}

/// Constructive L2 error of [`normalize_fp64`] relative to exact-real
/// normalization of the finite FP32 input.
///
/// Each input lifts exactly into FP64. The 768 non-negative FMA accumulation
/// steps therefore satisfy the standard `gamma(768, u64)` relative bound.
/// Correctly rounded square root adds one `u64` relative error; replacing the
/// square-root response to the accumulated-norm perturbation by the larger
/// `gamma` term is conservative. For the final divisions, inversion of that
/// norm enclosure and one correctly rounded FP64 division give
/// `(eta_norm + u64) / (1 - eta_norm)` coordinate-relative error. Its L2 form
/// has the same bound because the exact normalized direction has L2 norm one.
/// The raw FP32 range keeps every nonzero norm and every nonzero division
/// result normal in FP64, so no unmodeled FP64 underflow term is needed.
pub(crate) fn normalization_l2_error() -> f64 {
    let reduction = gamma(DIMENSION, FP64_UNIT_ROUNDOFF);
    let norm_relative_error = upward_add(
        upward_add(reduction, FP64_UNIT_ROUNDOFF),
        upward_mul(reduction, FP64_UNIT_ROUNDOFF),
    );
    upward_div(
        upward_add(norm_relative_error, FP64_UNIT_ROUNDOFF),
        next_down(1.0 - norm_relative_error),
    )
}

fn transform_error_budget() -> TransformErrorBudget {
    let normalization_l2_error = normalization_l2_error();
    // The private `KernelInputDirection` route performs exactly one conversion
    // per coordinate and never recomputes a norm, square root, or division.
    // A normal FP32 conversion has relative error at most u32; its subnormal
    // absolute error is smaller than u32 as well.  The normalized FP64 vector
    // has norm at most 1 + normalization_l2_error, so sqrt(768) copies give
    // this constructive conversion envelope.
    let conversion_l2_error = upward_mul(
        upward_sqrt(DIMENSION as f64),
        upward_mul(upward_add(1.0, normalization_l2_error), FP32_UNIT_ROUNDOFF),
    );
    let kernel_input_l2_error = upward_add(normalization_l2_error, conversion_l2_error);

    let round_error = normalized_hadamard_error();
    let composed_round_error = match TRANSFORM_ROUNDS {
        2 => upward_add(
            upward_mul(2.0, round_error),
            upward_mul(round_error, round_error),
        ),
        _ => unreachable!("the frozen transform always has two rounds"),
    };
    let transform_round_l2_error =
        upward_mul(upward_add(1.0, kernel_input_l2_error), composed_round_error);
    let transform_delta = upward_add(kernel_input_l2_error, transform_round_l2_error);
    TransformErrorBudget {
        normalization_l2_error,
        kernel_input_l2_error,
        transform_round_l2_error,
        transform_delta,
        query_norm_upper: upward_add(1.0, transform_delta),
    }
}

/// A normwise enclosure for one scalar normalized H128 application.
///
/// The kernel performs seven FP32 butterfly stages, then obtains the fixed
/// `1 / sqrt(128)` normalization through a correctly rounded square root and
/// reciprocal, and finally multiplies every output.  The butterfly matrix has
/// a `sqrt(128)` normwise amplification before normalization, so the first
/// term below bounds its seven additions/subtractions.  The second term is a
/// three-operation relative envelope for sqrt, reciprocal, and final
/// multiplication.  Their product is retained explicitly rather than hidden
/// in a single `gamma(8, u)` term, making the arithmetic proof auditable.
fn normalized_hadamard_error() -> f64 {
    let butterfly = upward_mul(
        upward_sqrt(HADAMARD_BLOCK_LEN as f64),
        gamma(HADAMARD_STAGES, FP32_UNIT_ROUNDOFF),
    );
    let normalization = gamma(3, FP32_UNIT_ROUNDOFF);
    upward_add(
        upward_add(butterfly, normalization),
        upward_mul(butterfly, normalization),
    )
}

pub(crate) fn upward_add(left: f64, right: f64) -> f64 {
    next_up(left + right)
}

pub(crate) fn upward_mul(left: f64, right: f64) -> f64 {
    next_up(left * right)
}

pub(crate) fn upward_div(left: f64, right: f64) -> f64 {
    next_up(left / right)
}

pub(crate) fn upward_sqrt(value: f64) -> f64 {
    next_up(value.sqrt())
}

pub(crate) fn next_up(value: f64) -> f64 {
    if value.is_nan() || value == f64::INFINITY {
        return value;
    }
    if value == 0.0 {
        return f64::from_bits(1);
    }
    let bits = value.to_bits();
    if value.is_sign_positive() {
        f64::from_bits(bits + 1)
    } else {
        f64::from_bits(bits - 1)
    }
}

pub(crate) fn next_down(value: f64) -> f64 {
    if value.is_nan() || value == f64::NEG_INFINITY {
        return value;
    }
    if value == 0.0 {
        return -f64::from_bits(1);
    }
    let bits = value.to_bits();
    if value.is_sign_positive() {
        f64::from_bits(bits - 1)
    } else {
        f64::from_bits(bits + 1)
    }
}

fn quantize_lookup(value: f64, table: LookupTable, index: usize) -> Result<i64, ScorerError> {
    if !value.is_finite() {
        return Err(ScorerError::NonFiniteLookup { table, index });
    }
    let scaled = value * COMPARISON_SCALE as f64;
    let limit = MAX_ABSOLUTE_TABLE_ENTRY as f64 - 0.5;
    if !scaled.is_finite() || scaled.abs() > limit {
        return Err(ScorerError::LookupOutOfRange { table, index });
    }
    let rounded = if scaled.is_sign_negative() {
        (scaled - 0.5).ceil()
    } else {
        (scaled + 0.5).floor()
    };
    let rounded = rounded as i64;
    if rounded.unsigned_abs() > MAX_ABSOLUTE_TABLE_ENTRY as u64 {
        return Err(ScorerError::LookupOutOfRange { table, index });
    }
    Ok(rounded)
}

fn absolute_lookup_entry(entry: i64) -> i64 {
    i64::try_from(entry.unsigned_abs())
        .expect("quantize_lookup rejects i64::MIN before a table entry can be stored")
}

fn checked_accumulate(sum: i64, value: i64) -> i64 {
    sum.checked_add(value).expect(
        "the scorer metadata bounds every table entry and proves at most 864 additions fit in i64",
    )
}

fn prepared_score_error(
    terms: usize,
    primary_absolute_sum: f64,
    residual_absolute_sum: f64,
) -> f64 {
    let quantization = upward_div(upward_mul(terms as f64, 0.5), COMPARISON_SCALE as f64);
    let primary_reduction = upward_mul(gamma(DIMENSION, FP64_UNIT_ROUNDOFF), primary_absolute_sum);
    let residual_reduction = upward_mul(
        upward_add(
            gamma(DIMENSION, FP64_UNIT_ROUNDOFF),
            gamma(Pq96Code::SUBVECTOR_DIMENSION, FP64_UNIT_ROUNDOFF),
        ),
        residual_absolute_sum,
    );
    upward_add(
        upward_add(quantization, primary_reduction),
        upward_add(residual_reduction, comparison_conversion_error()),
    )
}

fn serving_error(
    terms: usize,
    reconstruction_norm: f64,
    residual_norm: f64,
    query_norm_upper: f64,
) -> f64 {
    let primary_absolute_sum = upward_mul(query_norm_upper, reconstruction_norm);
    let residual_absolute_sum = upward_mul(query_norm_upper, residual_norm);
    prepared_score_error(terms, primary_absolute_sum, residual_absolute_sum)
}

fn comparison_conversion_error() -> f64 {
    // At i64::MAX a binary64 conversion has a 2^11 ULP, so half an ULP is
    // 2^10 fixed-point units. Division by a power-of-two comparison scale is exact.
    upward_div(1024.0, COMPARISON_SCALE as f64)
}

fn gamma(operations: usize, unit_roundoff: f64) -> f64 {
    let numerator = upward_mul(operations as f64, unit_roundoff);
    let denominator = next_down(1.0 - numerator);
    upward_div(numerator, denominator)
}

#[cfg(test)]
mod tests {
    use core::array;

    use spherra_domain::DIMENSION;

    use crate::TransformPlan;
    use crate::transform::transform_f64_reference;

    use super::{
        FixedPointScorer, normalization_l2_error, normalize_fp64, transform_normalized_fp64,
    };

    #[test]
    fn normalization_error_budget_covers_exact_pythagorean_oracles_at_extreme_exponents() {
        // These are exact rational reference directions: every nonzero input
        // pair is a scaled 3-4-5 triangle, so its real normalized components
        // are exactly 3/5 and 4/5.  The cases exercise ordinary, very large,
        // very small, and many-term reductions.  They validate the analytical
        // bound below; they are not its source of truth.
        let mut ordinary = [0.0; DIMENSION];
        ordinary[0] = 3.0;
        ordinary[1] = 4.0;

        let mut large = [0.0; DIMENSION];
        large[0] = 3.0_f32 * 2.0_f32.powi(100);
        large[1] = 4.0_f32 * 2.0_f32.powi(100);

        let mut small = [0.0; DIMENSION];
        small[0] = 3.0_f32 * 2.0_f32.powi(-100);
        small[1] = 4.0_f32 * 2.0_f32.powi(-100);

        let mut many_terms = [0.0; DIMENSION];
        for pair in 0..25 {
            many_terms[pair * 2] = 3.0;
            many_terms[pair * 2 + 1] = 4.0;
        }

        for raw in [ordinary, large, small, many_terms] {
            let normalized = normalize_fp64(&raw).expect("the exact oracle is non-zero");
            let mut exact = [0.0; DIMENSION];
            if raw.iter().filter(|value| **value != 0.0).count() == 2 {
                exact[0] = 3.0 / 5.0;
                exact[1] = 4.0 / 5.0;
            } else {
                for pair in 0..25 {
                    exact[pair * 2] = 3.0 / 25.0;
                    exact[pair * 2 + 1] = 4.0 / 25.0;
                }
            }
            let error = normalized
                .iter()
                .zip(exact)
                .fold(0.0, |sum, (actual, expected)| {
                    let difference = *actual - expected;
                    difference.mul_add(difference, sum)
                })
                .sqrt();
            assert!(error <= normalization_l2_error());
        }
    }

    #[test]
    fn constructive_transform_delta_covers_adversarial_and_generated_vectors() {
        let mut basis = [0.0; DIMENSION];
        basis[511] = 1.0;
        let alternating = array::from_fn(|coordinate| {
            if coordinate.is_multiple_of(2) {
                1.0
            } else {
                -1.0
            }
        });
        let dense_equal = [0.125; DIMENSION];
        let mut state = 0x5eed_fade_cafe_beef_u64;
        let generated = array::from_fn(|coordinate| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (((state >> 40) as u32) as f32 / ((1_u32 << 24) as f32))
                .mul_add(2.0, -1.0 + coordinate as f32 * 0.000_001)
        });
        let plan = TransformPlan::from_seed(0x1234_5678_9abc_def0);
        let transform_delta = FixedPointScorer::new().metadata().transform_delta();

        for raw in [basis, alternating, dense_equal, generated] {
            let normalized = normalize_fp64(&raw).expect("the adversarial vector is non-zero");
            let reference = transform_f64_reference(&plan, normalized);
            let implemented = transform_normalized_fp64(&plan, &normalized)
                .expect("the FP64-normalized vector has a valid kernel conversion");
            let squared_error =
                reference
                    .iter()
                    .zip(implemented)
                    .fold(0.0, |sum, (reference, implemented)| {
                        let difference = *reference - f64::from(implemented);
                        difference.mul_add(difference, sum)
                    });
            assert!(squared_error.sqrt() <= transform_delta);
        }
    }
}

#[cfg(test)]
#[path = "restoration_tests.rs"]
mod restoration_tests;
