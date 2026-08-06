//! Reproducible corpus descriptors and their deterministic splits.
//!
//! A measurement is only evidence if the exact vectors behind it can be
//! reconstructed. Two descriptor kinds satisfy that:
//!
//! * A **generated** descriptor names its distribution, dimension, and row
//!   count; every vector follows from the descriptor plus the root seed.
//! * A **file-backed** descriptor pins a real corpus by path, byte length, and
//!   BLAKE3, and additionally records the upstream dataset revision, embedding
//!   model revision, normalization policy, and license. Loading verifies the
//!   recorded length and hash before any vector is used, so a silently
//!   re-embedded or truncated file cannot masquerade as the pinned corpus.
//!
//! Both kinds are split into three disjoint parts: the indexed corpus, a
//! calibration split used only for quantizer/codebook training, and the query
//! set. Training on the rows being measured would flatter the codec, so the
//! calibration split never overlaps the indexed rows.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use spherra_domain::DIMENSION;

/// Rows reserved for quantizer and codebook training.
pub const CALIBRATION_ROWS: usize = 4_096;

/// Distinct seed streams keep the indexed, calibration, and query vectors of a
/// generated corpus independent while remaining a pure function of the root
/// seed.
const STREAM_INDEXED: u64 = 0x1000_0000_0000_0001;
const STREAM_CALIBRATION: u64 = 0x2000_0000_0000_0002;
const STREAM_QUERIES: u64 = 0x3000_0000_0000_0003;

/// The correlation coefficient of the `correlated` generator's AR(1) chain.
const CORRELATION: f64 = 0.85;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GeneratedKind {
    Gaussian,
    Correlated,
}

impl GeneratedKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Gaussian => "gaussian",
            Self::Correlated => "correlated",
        }
    }
}

/// A generated smoke corpus: a distribution, a dimension, and a row count.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct GeneratedDescriptor {
    pub kind: GeneratedKind,
    pub dimension: usize,
    pub vector_count: usize,
}

impl GeneratedDescriptor {
    fn name(&self) -> String {
        format!(
            "generated-{}-{}x{}",
            self.kind.as_str(),
            self.dimension,
            self.vector_count
        )
    }
}

/// A real corpus pinned by content hash and upstream revisions.
///
/// The vector bytes themselves are never committed: only this descriptor is,
/// and only when the corpus licence permits reproducible retrieval.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FileBackedDescriptor {
    pub name: String,
    pub path: PathBuf,
    pub byte_len: u64,
    pub blake3: String,
    pub row_count: usize,
    pub dimension: usize,
    pub normalization: String,
    pub source_dataset_revision: String,
    pub embedding_model_revision: String,
    pub license: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CorpusDescriptor {
    Generated(GeneratedDescriptor),
    FileBacked(FileBackedDescriptor),
}

impl CorpusDescriptor {
    /// Resolves the `--corpus` argument: either a generated corpus name of the
    /// form `generated-<distribution>-<dimension>x<count>`, or the path of a
    /// checked-in file-backed descriptor.
    pub fn resolve(specification: &str) -> Result<Self, CorpusError> {
        if let Some(rest) = specification.strip_prefix("generated-") {
            return Ok(Self::Generated(parse_generated(specification, rest)?));
        }

        let path = Path::new(specification);
        let bytes = fs::read(path).map_err(|source| CorpusError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let descriptor: FileBackedDescriptor =
            serde_json::from_slice(&bytes).map_err(|source| CorpusError::Descriptor {
                path: path.to_path_buf(),
                message: source.to_string(),
            })?;
        Ok(Self::FileBacked(descriptor))
    }

    pub fn name(&self) -> String {
        match self {
            Self::Generated(descriptor) => descriptor.name(),
            Self::FileBacked(descriptor) => descriptor.name.clone(),
        }
    }

    pub const fn dimension(&self) -> usize {
        match self {
            Self::Generated(descriptor) => descriptor.dimension,
            Self::FileBacked(descriptor) => descriptor.dimension,
        }
    }

    /// Materializes the three disjoint splits this descriptor promises.
    pub fn load(&self, seed: u64, query_count: usize) -> Result<CorpusSplits, CorpusError> {
        if query_count == 0 {
            return Err(CorpusError::EmptyQuerySet);
        }
        if self.dimension() != DIMENSION {
            return Err(CorpusError::UnsupportedDimension {
                actual: self.dimension(),
            });
        }

        match self {
            Self::Generated(descriptor) => load_generated(descriptor, seed, query_count),
            Self::FileBacked(descriptor) => load_file_backed(descriptor, seed, query_count),
        }
    }
}

/// The indexed corpus plus its disjoint calibration and query splits.
#[derive(Clone, Debug)]
pub struct CorpusSplits {
    name: String,
    hash: String,
    indexed: Vec<[f32; DIMENSION]>,
    calibration: Vec<[f32; DIMENSION]>,
    queries: Vec<[f32; DIMENSION]>,
}

impl CorpusSplits {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Lowercase hex BLAKE3 over the canonical little-endian FP32 bytes of the
    /// indexed rows, in row-major order.
    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn indexed(&self) -> &[[f32; DIMENSION]] {
        &self.indexed
    }

    pub fn calibration(&self) -> &[[f32; DIMENSION]] {
        &self.calibration
    }

    pub fn queries(&self) -> &[[f32; DIMENSION]] {
        &self.queries
    }
}

fn parse_generated(specification: &str, rest: &str) -> Result<GeneratedDescriptor, CorpusError> {
    let unparsable = || CorpusError::UnparsableName {
        specification: specification.to_owned(),
    };
    let (kind, shape) = rest.rsplit_once('-').ok_or_else(unparsable)?;
    let kind = match kind {
        "gaussian" => GeneratedKind::Gaussian,
        "correlated" => GeneratedKind::Correlated,
        _ => return Err(unparsable()),
    };
    let (dimension, vector_count) = shape.split_once('x').ok_or_else(unparsable)?;
    let dimension: usize = dimension.parse().map_err(|_| unparsable())?;
    let vector_count: usize = vector_count.parse().map_err(|_| unparsable())?;
    if vector_count == 0 {
        return Err(unparsable());
    }
    Ok(GeneratedDescriptor {
        kind,
        dimension,
        vector_count,
    })
}

fn load_generated(
    descriptor: &GeneratedDescriptor,
    seed: u64,
    query_count: usize,
) -> Result<CorpusSplits, CorpusError> {
    let indexed = generate_rows(
        descriptor.kind,
        descriptor.vector_count,
        seed,
        STREAM_INDEXED,
    );
    let calibration = generate_rows(
        descriptor.kind,
        CALIBRATION_ROWS.min(descriptor.vector_count),
        seed,
        STREAM_CALIBRATION,
    );
    let queries = generate_rows(descriptor.kind, query_count, seed, STREAM_QUERIES);
    Ok(CorpusSplits {
        name: descriptor.name(),
        hash: hash_rows(&indexed),
        indexed,
        calibration,
        queries,
    })
}

/// A file-backed corpus is split by position, never by resampling: the tail
/// holds out the calibration and query rows, and the head is what gets indexed.
/// Holding out the query rows keeps a measured neighbour from being the query
/// itself.
fn load_file_backed(
    descriptor: &FileBackedDescriptor,
    seed: u64,
    query_count: usize,
) -> Result<CorpusSplits, CorpusError> {
    let bytes = fs::read(&descriptor.path).map_err(|source| CorpusError::Io {
        path: descriptor.path.clone(),
        source,
    })?;

    let actual_len = bytes.len() as u64;
    if actual_len != descriptor.byte_len {
        return Err(CorpusError::ByteLenMismatch {
            expected: descriptor.byte_len,
            actual: actual_len,
        });
    }

    let actual_hash = blake3::hash(&bytes).to_hex().to_string();
    if actual_hash != descriptor.blake3.to_ascii_lowercase() {
        return Err(CorpusError::HashMismatch {
            expected: descriptor.blake3.clone(),
            actual: actual_hash,
        });
    }

    let row_bytes = descriptor.dimension * size_of::<f32>();
    let expected_len = (descriptor.row_count * row_bytes) as u64;
    if expected_len != actual_len {
        return Err(CorpusError::RowCountMismatch {
            declared_rows: descriptor.row_count,
            byte_len: actual_len,
        });
    }

    let held_out = CALIBRATION_ROWS.min(descriptor.row_count / 4) + query_count;
    if descriptor.row_count <= held_out {
        return Err(CorpusError::TooFewRows {
            row_count: descriptor.row_count,
            required: held_out + 1,
        });
    }

    let mut rows: Vec<[f32; DIMENSION]> = Vec::with_capacity(descriptor.row_count);
    for chunk in bytes.chunks_exact(row_bytes) {
        let mut row = [0.0_f32; DIMENSION];
        for (slot, value) in row.iter_mut().zip(chunk.chunks_exact(size_of::<f32>())) {
            let mut raw = [0_u8; 4];
            raw.copy_from_slice(value);
            *slot = f32::from_le_bytes(raw);
        }
        if let Some(coordinate) = row.iter().position(|value| !value.is_finite()) {
            return Err(CorpusError::NonFiniteRow {
                row: rows.len(),
                coordinate,
            });
        }
        rows.push(row);
    }

    let queries = rows.split_off(descriptor.row_count - query_count);
    let calibration = rows.split_off(descriptor.row_count - held_out);
    let indexed = rows;

    // The root seed still selects the query order so that two runs over the
    // same pinned corpus with different seeds are not accidentally identical.
    let mut rng = stream_rng(seed, STREAM_QUERIES);
    let mut queries = queries;
    for index in (1..queries.len()).rev() {
        queries.swap(index, rng.random_range(0..=index));
    }

    Ok(CorpusSplits {
        name: descriptor.name.clone(),
        hash: hash_rows(&indexed),
        indexed,
        calibration,
        queries,
    })
}

fn stream_rng(seed: u64, stream: u64) -> ChaCha20Rng {
    let mut key = [0_u8; 32];
    key[..8].copy_from_slice(&seed.to_le_bytes());
    key[8..16].copy_from_slice(&stream.to_le_bytes());
    key[16..24].copy_from_slice(&seed.rotate_left(17).to_le_bytes());
    key[24..].copy_from_slice(&stream.rotate_left(29).to_le_bytes());
    ChaCha20Rng::from_seed(key)
}

fn generate_rows(
    kind: GeneratedKind,
    count: usize,
    seed: u64,
    stream: u64,
) -> Vec<[f32; DIMENSION]> {
    let mut rng = stream_rng(seed, stream);
    (0..count)
        .map(|_| match kind {
            GeneratedKind::Gaussian => gaussian_row(&mut rng),
            GeneratedKind::Correlated => correlated_row(&mut rng),
        })
        .collect()
}

fn gaussian_row(rng: &mut ChaCha20Rng) -> [f32; DIMENSION] {
    let mut row = [0.0_f32; DIMENSION];
    for slot in &mut row {
        *slot = standard_normal(rng) as f32;
    }
    row
}

/// An AR(1) chain across coordinates, which gives the neighbouring-coordinate
/// correlation that real embeddings show and that a purely independent
/// Gaussian corpus cannot exercise.
fn correlated_row(rng: &mut ChaCha20Rng) -> [f32; DIMENSION] {
    let mut row = [0.0_f32; DIMENSION];
    let innovation_scale = (1.0 - CORRELATION * CORRELATION).sqrt();
    let mut previous = standard_normal(rng);
    row[0] = previous as f32;
    for slot in row.iter_mut().skip(1) {
        previous = CORRELATION.mul_add(previous, innovation_scale * standard_normal(rng));
        *slot = previous as f32;
    }
    row
}

/// Box-Muller from two open-interval uniforms. Rejecting an exact zero keeps
/// the logarithm finite.
fn standard_normal(rng: &mut ChaCha20Rng) -> f64 {
    let mut uniform = rng.random::<f64>();
    while uniform <= f64::MIN_POSITIVE {
        uniform = rng.random::<f64>();
    }
    let angle = std::f64::consts::TAU * rng.random::<f64>();
    (-2.0 * uniform.ln()).sqrt() * angle.cos()
}

fn hash_rows(rows: &[[f32; DIMENSION]]) -> String {
    let mut hasher = blake3::Hasher::new();
    for row in rows {
        for value in row {
            hasher.update(&value.to_le_bytes());
        }
    }
    hasher.finalize().to_hex().to_string()
}

#[derive(Debug)]
pub enum CorpusError {
    UnparsableName { specification: String },
    UnsupportedDimension { actual: usize },
    EmptyQuerySet,
    Io { path: PathBuf, source: io::Error },
    Descriptor { path: PathBuf, message: String },
    ByteLenMismatch { expected: u64, actual: u64 },
    HashMismatch { expected: String, actual: String },
    RowCountMismatch { declared_rows: usize, byte_len: u64 },
    TooFewRows { row_count: usize, required: usize },
    NonFiniteRow { row: usize, coordinate: usize },
}

impl fmt::Display for CorpusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnparsableName { specification } => write!(
                formatter,
                "corpus {specification} is neither generated-<gaussian|correlated>-<dimension>x<count> nor a descriptor path",
            ),
            Self::UnsupportedDimension { actual } => write!(
                formatter,
                "corpus dimension {actual} is not the required {DIMENSION}",
            ),
            Self::EmptyQuerySet => write!(formatter, "a measurement requires at least one query"),
            Self::Io { path, source } => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::Descriptor { path, message } => write!(
                formatter,
                "{} is not a valid corpus descriptor: {message}",
                path.display(),
            ),
            Self::ByteLenMismatch { expected, actual } => write!(
                formatter,
                "corpus byte length {actual} does not match the pinned {expected}",
            ),
            Self::HashMismatch { expected, actual } => write!(
                formatter,
                "corpus BLAKE3 {actual} does not match the pinned {expected}",
            ),
            Self::RowCountMismatch {
                declared_rows,
                byte_len,
            } => write!(
                formatter,
                "declared row count {declared_rows} does not describe {byte_len} corpus bytes",
            ),
            Self::TooFewRows {
                row_count,
                required,
            } => write!(
                formatter,
                "corpus has {row_count} rows but a disjoint calibration and query split needs more than {required}",
            ),
            Self::NonFiniteRow { row, coordinate } => write!(
                formatter,
                "corpus row {row} is non-finite at coordinate {coordinate}",
            ),
        }
    }
}

impl std::error::Error for CorpusError {}
