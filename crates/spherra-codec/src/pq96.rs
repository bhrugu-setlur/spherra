use core::fmt;
use core::ops::Range;

use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;
use spherra_domain::DIMENSION;

const PQ_SEED_DERIVATION_CONTEXT: &str = "spherra.pq96.kmeans-plus-plus.v1";
const PQ_CODEBOOK_ID_LEN: usize = 32;
const LLOYD_ITERATIONS: usize = 25;

type Centroids =
    [[[f32; Pq96Code::SUBVECTOR_DIMENSION]; Pq96Code::CENTROIDS]; Pq96Code::SUBQUANTIZERS];

/// A caller-provided transformed residual, `T(normalize(x)) - p`.
pub type ResidualVector = [f32; DIMENSION];

#[derive(Clone, Debug, PartialEq)]
pub enum CodecError {
    InsufficientCalibrationRows {
        required: usize,
        actual: usize,
    },
    NonFiniteResidual {
        row: usize,
        coordinate: usize,
    },
    NonFiniteEncodedResidual {
        coordinate: usize,
    },
    NonFinitePreparedQuery {
        coordinate: usize,
    },
    InvalidSubquantizer {
        index: usize,
    },
    InvalidRowRange {
        start: u32,
        end: u32,
    },
    PrimarySourceOverfilled {
        requested_rows: usize,
        output_capacity: usize,
        written: usize,
    },
    PrimarySourceReturnedUnexpectedRow {
        expected: u32,
        actual: u32,
    },
    PrimarySourceDidNotReturnCandidate {
        row: u32,
    },
    RowOverflow {
        row: u32,
    },
}

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InsufficientCalibrationRows { required, actual } => write!(
                formatter,
                "PQ96 training requires at least {required} calibration rows, received {actual}"
            ),
            Self::NonFiniteResidual { row, coordinate } => write!(
                formatter,
                "calibration residual row {row} has a non-finite component at coordinate {coordinate}"
            ),
            Self::NonFiniteEncodedResidual { coordinate } => write!(
                formatter,
                "residual to encode has a non-finite component at coordinate {coordinate}"
            ),
            Self::NonFinitePreparedQuery { coordinate } => write!(
                formatter,
                "prepared query has a non-finite component at coordinate {coordinate}"
            ),
            Self::InvalidSubquantizer { index } => {
                write!(formatter, "PQ96 subquantizer index {index} is out of range")
            }
            Self::InvalidRowRange { start, end } => {
                write!(formatter, "primary scan range {start}..{end} is invalid")
            }
            Self::PrimarySourceOverfilled {
                requested_rows,
                output_capacity,
                written,
            } => write!(
                formatter,
                "primary source wrote {written} scores for {requested_rows} rows with capacity {output_capacity}"
            ),
            Self::PrimarySourceReturnedUnexpectedRow { expected, actual } => write!(
                formatter,
                "primary source returned row {actual} while reranking requested row {expected}"
            ),
            Self::PrimarySourceDidNotReturnCandidate { row } => {
                write!(
                    formatter,
                    "primary source did not return requested candidate row {row}"
                )
            }
            Self::RowOverflow { row } => {
                write!(
                    formatter,
                    "candidate row {row} cannot form a one-row scan range"
                )
            }
        }
    }
}

impl std::error::Error for CodecError {}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Pq96Code([u8; Self::BYTE_LEN]);

impl Pq96Code {
    pub const SUBQUANTIZERS: usize = 96;
    pub const SUBVECTOR_DIMENSION: usize = 8;
    pub const CENTROIDS: usize = 256;
    pub const BYTE_LEN: usize = 96;

    pub const fn from_bytes(bytes: [u8; Self::BYTE_LEN]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; Self::BYTE_LEN] {
        &self.0
    }
}

/// The transformed query representation used by the scalar codec boundary.
///
/// Task 6 extends this value with fixed-point lookup tables while preserving the
/// residual-free primary scan capability defined below.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedQuery([f32; DIMENSION]);

impl PreparedQuery {
    pub fn from_transformed(transformed: [f32; DIMENSION]) -> Result<Self, CodecError> {
        for (coordinate, value) in transformed.iter().enumerate() {
            if !value.is_finite() {
                return Err(CodecError::NonFinitePreparedQuery { coordinate });
            }
        }

        Ok(Self(transformed))
    }

    pub const fn as_array(&self) -> &[f32; DIMENSION] {
        &self.0
    }
}

/// A primary candidate row.
///
/// Task 5 intentionally does not expose a comparison representation. Task 6 adds
/// the fixed-point score scale and lookup-table accumulation without replacing a
/// public floating-point score API.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrimaryScore {
    row: u32,
}

impl PrimaryScore {
    pub const fn for_row(row: u32) -> Self {
        Self { row }
    }

    pub const fn row(self) -> u32 {
        self.row
    }
}

/// A primary candidate prepared for Task 6 fixed-point comparison.
///
/// The decoded residual is retained after the one permitted candidate-only
/// residual-code read, so the fixed-point scorer can consume it without
/// reopening the residual source.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedCandidate {
    primary: PrimaryScore,
    residual_code: Pq96Code,
    codebook_identity: [u8; PQ_CODEBOOK_ID_LEN],
    decoded_residual: ResidualVector,
}

impl PreparedCandidate {
    pub const fn primary(&self) -> PrimaryScore {
        self.primary
    }

    pub const fn row(&self) -> u32 {
        self.primary.row()
    }

    pub const fn decoded_residual(&self) -> &ResidualVector {
        &self.decoded_residual
    }

    pub const fn residual_code(&self) -> &Pq96Code {
        &self.residual_code
    }

    pub(crate) const fn codebook_identity(&self) -> [u8; PQ_CODEBOOK_ID_LEN] {
        self.codebook_identity
    }
}

/// The capability available to resident primary-code scanners.
pub trait PrimaryCodes {
    fn scan_primary(
        &self,
        rows: Range<u32>,
        query: &PreparedQuery,
        out: &mut [PrimaryScore],
    ) -> Result<usize, CodecError>;
}

/// The capability available only to candidate reranking.
pub trait ResidualCodes {
    fn load_residual(&self, row: u32) -> Result<Pq96Code, CodecError>;
}

/// Scans resident primary codes without receiving any residual-code capability.
pub fn scan_primary(
    primary: &dyn PrimaryCodes,
    rows: Range<u32>,
    query: &PreparedQuery,
    out: &mut [PrimaryScore],
) -> Result<usize, CodecError> {
    if rows.start > rows.end {
        return Err(CodecError::InvalidRowRange {
            start: rows.start,
            end: rows.end,
        });
    }

    let requested_rows = (rows.end - rows.start) as usize;
    let written = primary.scan_primary(rows, query, out)?;
    if written > requested_rows || written > out.len() {
        return Err(CodecError::PrimarySourceOverfilled {
            requested_rows,
            output_capacity: out.len(),
            written,
        });
    }

    Ok(written)
}

/// Prepares a bounded candidate row slice for reranking by loading and decoding one
/// residual code per row.
///
/// Task 6 combines the retained primary candidate and decoded residual in its
/// fixed-point comparison path. This Task 5 boundary deliberately performs no
/// floating-point serving-score calculation and does not reopen residual codes.
pub fn rerank_candidates(
    primary: &dyn PrimaryCodes,
    residuals: &dyn ResidualCodes,
    codebook: &Pq96Codebook,
    query: &PreparedQuery,
    candidate_rows: &[u32],
    out: &mut [PreparedCandidate],
) -> Result<usize, CodecError> {
    let written = candidate_rows.len().min(out.len());

    for (index, row) in candidate_rows.iter().copied().take(written).enumerate() {
        let end = row.checked_add(1).ok_or(CodecError::RowOverflow { row })?;
        let mut primary_score = [PrimaryScore::for_row(row)];
        let primary_written = scan_primary(primary, row..end, query, &mut primary_score)?;
        if primary_written == 0 {
            return Err(CodecError::PrimarySourceDidNotReturnCandidate { row });
        }
        if primary_score[0].row() != row {
            return Err(CodecError::PrimarySourceReturnedUnexpectedRow {
                expected: row,
                actual: primary_score[0].row(),
            });
        }

        let residual_code = residuals.load_residual(row)?;
        out[index] = codebook.prepare_candidate(primary_score[0], residual_code);
    }

    Ok(written)
}

/// Deterministic training facts collected across all 96 subquantizers.
///
/// These diagnostics support validation of the scalar training path. They do
/// not participate in canonical codebook bytes or serving-score comparison.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Pq96TrainingDiagnostics {
    lloyd_iterations: u32,
    centroid_moves: u32,
    empty_cluster_reseeds: u32,
}

impl Pq96TrainingDiagnostics {
    pub const fn lloyd_iterations(self) -> u32 {
        self.lloyd_iterations
    }

    pub const fn centroid_moves(self) -> u32 {
        self.centroid_moves
    }

    pub const fn empty_cluster_reseeds(self) -> u32 {
        self.empty_cluster_reseeds
    }

    fn include(&mut self, other: Self) {
        self.lloyd_iterations += other.lloyd_iterations;
        self.centroid_moves += other.centroid_moves;
        self.empty_cluster_reseeds += other.empty_cluster_reseeds;
    }
}

#[derive(Clone, Debug)]
pub struct Pq96Codebook {
    centroids: Box<Centroids>,
    codebook_id: [u8; PQ_CODEBOOK_ID_LEN],
    training_diagnostics: Pq96TrainingDiagnostics,
}

impl Pq96Codebook {
    /// Trains on caller-provided transformed residuals, `T(normalize(x)) - p`.
    ///
    /// This API intentionally does not accept original vectors or a direct-code
    /// reconstruction: the caller owns formation of the residual training input.
    pub fn train(residuals: &[ResidualVector], seed: u64) -> Result<Self, CodecError> {
        validate_calibration_residuals(residuals)?;

        let mut centroids = Box::new(
            [[[0.0; Pq96Code::SUBVECTOR_DIMENSION]; Pq96Code::CENTROIDS]; Pq96Code::SUBQUANTIZERS],
        );
        let mut training_diagnostics = Pq96TrainingDiagnostics::default();
        for subquantizer in 0..Pq96Code::SUBQUANTIZERS {
            let (trained_centroids, subquantizer_diagnostics) =
                train_subquantizer(residuals, subquantizer, seed);
            centroids[subquantizer] = trained_centroids;
            training_diagnostics.include(subquantizer_diagnostics);
        }

        let mut codebook = Self {
            centroids,
            codebook_id: [0; PQ_CODEBOOK_ID_LEN],
            training_diagnostics,
        };
        codebook.codebook_id = *blake3::hash(&codebook.canonical_bytes()).as_bytes();
        Ok(codebook)
    }

    /// A finite, deterministic codebook reserved for the external fuzz crate.
    ///
    /// It avoids embedding a full 96-way Lloyd training run in every fuzz
    /// worker startup while retaining non-zero, code-dependent residual terms.
    /// This feature is intentionally absent from normal codec builds.
    #[cfg(feature = "fuzzing")]
    pub fn fuzz_fixture() -> Self {
        let mut centroids = Box::new(
            [[[0.0; Pq96Code::SUBVECTOR_DIMENSION]; Pq96Code::CENTROIDS]; Pq96Code::SUBQUANTIZERS],
        );
        for subquantizer in 0..Pq96Code::SUBQUANTIZERS {
            for code in 0..Pq96Code::CENTROIDS {
                for lane in 0..Pq96Code::SUBVECTOR_DIMENSION {
                    centroids[subquantizer][code][lane] = (code as f32 - 127.5) * 0.000_25
                        + subquantizer as f32 * 0.000_001
                        + lane as f32 * 0.000_000_1;
                }
            }
        }
        let mut codebook = Self {
            centroids,
            codebook_id: [0; PQ_CODEBOOK_ID_LEN],
            training_diagnostics: Pq96TrainingDiagnostics::default(),
        };
        codebook.codebook_id = *blake3::hash(&codebook.canonical_bytes()).as_bytes();
        codebook
    }

    pub fn encode(&self, residual: &ResidualVector) -> Result<Pq96Code, CodecError> {
        for (coordinate, value) in residual.iter().enumerate() {
            if !value.is_finite() {
                return Err(CodecError::NonFiniteEncodedResidual { coordinate });
            }
        }

        let mut codes = [0; Pq96Code::BYTE_LEN];
        for (subquantizer, code) in codes.iter_mut().enumerate() {
            *code = nearest_centroid(residual, subquantizer, &self.centroids[subquantizer]) as u8;
        }
        Ok(Pq96Code::from_bytes(codes))
    }

    /// Retains the one loaded residual code and its decoded value for candidate
    /// reranking.  Construction is deliberately owned by the codebook so a
    /// Task 6 scorer can reject decoded residuals from another representation.
    pub fn prepare_candidate(
        &self,
        primary: PrimaryScore,
        residual_code: Pq96Code,
    ) -> PreparedCandidate {
        PreparedCandidate {
            primary,
            residual_code,
            codebook_identity: self.codebook_id,
            decoded_residual: self.decode(&residual_code),
        }
    }

    pub fn decode(&self, code: &Pq96Code) -> ResidualVector {
        let mut residual = [0.0; DIMENSION];
        for (subquantizer, selected) in code.as_bytes().iter().copied().enumerate() {
            let start = subquantizer * Pq96Code::SUBVECTOR_DIMENSION;
            let end = start + Pq96Code::SUBVECTOR_DIMENSION;
            residual[start..end]
                .copy_from_slice(&self.centroids[subquantizer][usize::from(selected)]);
        }
        residual
    }

    pub fn centroid(
        &self,
        subquantizer: usize,
        code: u8,
    ) -> Result<&[f32; Pq96Code::SUBVECTOR_DIMENSION], CodecError> {
        self.centroids
            .get(subquantizer)
            .map(|centroids| &centroids[usize::from(code)])
            .ok_or(CodecError::InvalidSubquantizer {
                index: subquantizer,
            })
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(
            Pq96Code::SUBQUANTIZERS
                * Pq96Code::CENTROIDS
                * Pq96Code::SUBVECTOR_DIMENSION
                * size_of::<f32>(),
        );

        for subquantizer in self.centroids.iter() {
            for centroid in subquantizer {
                for value in centroid {
                    bytes.extend_from_slice(&canonical_f32_bits(*value).to_le_bytes());
                }
            }
        }
        bytes
    }

    pub const fn codebook_id(&self) -> &[u8; PQ_CODEBOOK_ID_LEN] {
        &self.codebook_id
    }

    pub const fn training_diagnostics(&self) -> Pq96TrainingDiagnostics {
        self.training_diagnostics
    }
}

fn validate_calibration_residuals(residuals: &[ResidualVector]) -> Result<(), CodecError> {
    if residuals.len() < Pq96Code::CENTROIDS {
        return Err(CodecError::InsufficientCalibrationRows {
            required: Pq96Code::CENTROIDS,
            actual: residuals.len(),
        });
    }

    for (row, residual) in residuals.iter().enumerate() {
        for (coordinate, value) in residual.iter().enumerate() {
            if !value.is_finite() {
                return Err(CodecError::NonFiniteResidual { row, coordinate });
            }
        }
    }
    Ok(())
}

fn train_subquantizer(
    residuals: &[ResidualVector],
    subquantizer: usize,
    seed: u64,
) -> (
    [[f32; Pq96Code::SUBVECTOR_DIMENSION]; Pq96Code::CENTROIDS],
    Pq96TrainingDiagnostics,
) {
    let mut centroids = initialize_centroids(residuals, subquantizer, seed);
    let mut assignments = vec![usize::MAX; residuals.len()];
    let mut diagnostics = Pq96TrainingDiagnostics::default();

    for _ in 0..LLOYD_ITERATIONS {
        diagnostics.lloyd_iterations += 1;
        let mut sums = [[0.0_f64; Pq96Code::SUBVECTOR_DIMENSION]; Pq96Code::CENTROIDS];
        let mut counts = [0_usize; Pq96Code::CENTROIDS];
        let mut squared_errors = vec![0.0_f64; residuals.len()];
        let mut assignments_changed = false;

        for (row, residual) in residuals.iter().enumerate() {
            let (centroid, squared_error) =
                nearest_centroid_with_error(residual, subquantizer, &centroids);
            assignments_changed |= assignments[row] != centroid;
            assignments[row] = centroid;
            squared_errors[row] = squared_error;
            counts[centroid] += 1;

            let start = subquantizer * Pq96Code::SUBVECTOR_DIMENSION;
            for lane in 0..Pq96Code::SUBVECTOR_DIMENSION {
                sums[centroid][lane] += f64::from(residual[start + lane]);
            }
        }

        let mut reseeded_rows = vec![false; residuals.len()];
        let mut centroids_changed = false;
        for centroid in 0..Pq96Code::CENTROIDS {
            if counts[centroid] == 0 {
                diagnostics.empty_cluster_reseeds += 1;
                let row = largest_error_row(&squared_errors, &reseeded_rows);
                reseeded_rows[row] = true;
                let reseeded_centroid = subvector(&residuals[row], subquantizer);
                let centroid_changed = centroids[centroid] != reseeded_centroid;
                diagnostics.centroid_moves += u32::from(centroid_changed);
                centroids_changed |= centroid_changed;
                centroids[centroid] = reseeded_centroid;
            } else {
                let count = counts[centroid] as f64;
                let mut updated_centroid = [0.0; Pq96Code::SUBVECTOR_DIMENSION];
                for (lane, value) in updated_centroid.iter_mut().enumerate() {
                    *value = canonicalize_zero((sums[centroid][lane] / count) as f32);
                }
                let centroid_changed = centroids[centroid] != updated_centroid;
                diagnostics.centroid_moves += u32::from(centroid_changed);
                centroids_changed |= centroid_changed;
                centroids[centroid] = updated_centroid;
            }
        }

        if lloyd_iteration_has_converged(assignments_changed, centroids_changed) {
            break;
        }
    }

    (centroids, diagnostics)
}

fn lloyd_iteration_has_converged(assignments_changed: bool, centroids_changed: bool) -> bool {
    !assignments_changed && !centroids_changed
}

fn initialize_centroids(
    residuals: &[ResidualVector],
    subquantizer: usize,
    seed: u64,
) -> [[f32; Pq96Code::SUBVECTOR_DIMENSION]; Pq96Code::CENTROIDS] {
    let mut centroids = [[0.0; Pq96Code::SUBVECTOR_DIMENSION]; Pq96Code::CENTROIDS];
    let mut rng = ChaCha20Rng::from_seed(derive_subquantizer_seed(seed, subquantizer));
    let mut selected = vec![false; residuals.len()];
    let mut minimum_squared_errors = vec![f64::INFINITY; residuals.len()];

    let first = uniform_index(&mut rng, residuals.len());
    selected[first] = true;
    centroids[0] = subvector(&residuals[first], subquantizer);

    for centroid in 1..Pq96Code::CENTROIDS {
        let previous = &centroids[centroid - 1];
        for (row, residual) in residuals.iter().enumerate() {
            let squared_error = squared_distance(residual, subquantizer, previous);
            if squared_error < minimum_squared_errors[row] {
                minimum_squared_errors[row] = squared_error;
            }
        }

        let total_squared_error = minimum_squared_errors.iter().sum::<f64>();
        let next = if total_squared_error == 0.0 {
            selected
                .iter()
                .position(|is_selected| !is_selected)
                .expect("training has at least as many rows as PQ centroids")
        } else {
            weighted_row(&mut rng, &minimum_squared_errors, total_squared_error)
        };
        selected[next] = true;
        centroids[centroid] = subvector(&residuals[next], subquantizer);
    }

    centroids
}

fn nearest_centroid(
    residual: &ResidualVector,
    subquantizer: usize,
    centroids: &[[f32; Pq96Code::SUBVECTOR_DIMENSION]; Pq96Code::CENTROIDS],
) -> usize {
    nearest_centroid_with_error(residual, subquantizer, centroids).0
}

fn nearest_centroid_with_error(
    residual: &ResidualVector,
    subquantizer: usize,
    centroids: &[[f32; Pq96Code::SUBVECTOR_DIMENSION]; Pq96Code::CENTROIDS],
) -> (usize, f64) {
    let mut nearest = 0;
    let mut minimum_squared_error = squared_distance(residual, subquantizer, &centroids[0]);

    for (candidate, centroid) in centroids.iter().enumerate().skip(1) {
        let squared_error = squared_distance(residual, subquantizer, centroid);
        if squared_error < minimum_squared_error {
            nearest = candidate;
            minimum_squared_error = squared_error;
        }
    }

    (nearest, minimum_squared_error)
}

fn squared_distance(
    residual: &ResidualVector,
    subquantizer: usize,
    centroid: &[f32; Pq96Code::SUBVECTOR_DIMENSION],
) -> f64 {
    let start = subquantizer * Pq96Code::SUBVECTOR_DIMENSION;
    (0..Pq96Code::SUBVECTOR_DIMENSION)
        .map(|lane| {
            let difference = f64::from(residual[start + lane]) - f64::from(centroid[lane]);
            difference * difference
        })
        .sum()
}

fn subvector(
    residual: &ResidualVector,
    subquantizer: usize,
) -> [f32; Pq96Code::SUBVECTOR_DIMENSION] {
    let start = subquantizer * Pq96Code::SUBVECTOR_DIMENSION;
    let mut result = [0.0; Pq96Code::SUBVECTOR_DIMENSION];
    for lane in 0..Pq96Code::SUBVECTOR_DIMENSION {
        result[lane] = canonicalize_zero(residual[start + lane]);
    }
    result
}

fn largest_error_row(squared_errors: &[f64], already_reseeded: &[bool]) -> usize {
    let mut selected = None;
    for (row, squared_error) in squared_errors.iter().copied().enumerate() {
        if already_reseeded[row] {
            continue;
        }

        if selected.is_none_or(|best| squared_error > squared_errors[best]) {
            selected = Some(row);
        }
    }
    selected.expect("every empty PQ centroid can select a calibration row")
}

fn weighted_row(rng: &mut ChaCha20Rng, squared_errors: &[f64], total_squared_error: f64) -> usize {
    let fraction = (rng.next_u64() >> 11) as f64 * (1.0 / ((1_u64 << 53) as f64));
    let threshold = fraction * total_squared_error;
    let mut cumulative = 0.0;

    for (row, squared_error) in squared_errors.iter().copied().enumerate() {
        cumulative += squared_error;
        if cumulative > threshold {
            return row;
        }
    }

    squared_errors
        .iter()
        .rposition(|squared_error| *squared_error > 0.0)
        .expect("a positive total squared error has a positive row")
}

fn derive_subquantizer_seed(seed: u64, subquantizer: usize) -> [u8; PQ_CODEBOOK_ID_LEN] {
    let mut material = [0_u8; 10];
    material[..8].copy_from_slice(&seed.to_le_bytes());
    material[8..].copy_from_slice(&(subquantizer as u16).to_le_bytes());
    blake3::derive_key(PQ_SEED_DERIVATION_CONTEXT, &material)
}

fn uniform_index(rng: &mut ChaCha20Rng, upper_exclusive: usize) -> usize {
    let bound = u64::try_from(upper_exclusive)
        .expect("the calibration corpus length fits in u64 on supported platforms");
    let accepted = u64::MAX - (u64::MAX % bound);

    loop {
        let candidate = rng.next_u64();
        if candidate < accepted {
            return (candidate % bound) as usize;
        }
    }
}

fn canonical_f32_bits(value: f32) -> u32 {
    canonicalize_zero(value).to_bits()
}

fn canonicalize_zero(value: f32) -> f32 {
    if value == 0.0 { 0.0 } else { value }
}

#[cfg(test)]
mod tests {
    use super::lloyd_iteration_has_converged;

    #[test]
    fn lloyd_iteration_stops_only_when_assignments_and_centroids_are_unchanged() {
        assert!(!lloyd_iteration_has_converged(false, true));
        assert!(lloyd_iteration_has_converged(false, false));
        assert!(!lloyd_iteration_has_converged(true, false));
        assert!(!lloyd_iteration_has_converged(true, true));
    }

    #[cfg(feature = "fuzzing")]
    #[test]
    fn fuzz_fixture_is_deterministic_and_nonzero() {
        let first_id = {
            let first = super::Pq96Codebook::fuzz_fixture();
            *first.codebook_id()
        };
        let second = super::Pq96Codebook::fuzz_fixture();

        assert_eq!(first_id, *second.codebook_id());
        assert_ne!(
            second.centroid(0, 0).expect("valid fixture centroid"),
            second.centroid(0, 1).expect("valid fixture centroid"),
        );
    }
}
