//! Reproducible codec, certificate, local-index quality, latency and memory
//! measurements. Commands validate their recorded output against embedded
//! schemas and return failure when their measured gate fails.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use spherra_codec::{
    CertificateBlockId, CertificateRow, ExhaustiveBlock, FixedPointScorer, Pq96Code, Pq96Codebook,
    PrimaryScore, QuantizerTable, TransformPlan, TransformedDirection,
    build_exhaustive_certificate, dot_f64, normalize_fp64, transform,
};
use spherra_domain::{DIMENSION, ValidatedVector};
use spherra_testkit::corpus::CorpusDescriptor;
use spherra_testkit::harness::{CodecFormatRun, PruneOutcome, codec_id_hex, hex};
use spherra_testkit::machine::{
    CacheState, DURABILITY_MODE_NOT_APPLICABLE, MachineProfile, SourceRevision,
    timestamp_rfc3339_utc,
};
use spherra_testkit::results::{
    CertificateSoakResult, CodecFormatMeasurement, SCHEMA_VERSION, SoakFailure, SoakScoreKind,
    SoakSeedIdentity, validate_against_schema,
};

mod index_quality;
mod local_index;
mod norm_audit;
mod original_rerank;

const LAYOUT_TILED_SOA_32: &str = "tiled-soa-32";
const SOAK_PROGRESS_INTERVAL: u64 = 100_000;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match run(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("spherra-bench: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: &[String]) -> Result<(), BenchError> {
    let (subcommand, rest) = arguments
        .split_first()
        .ok_or(BenchError::MissingSubcommand)?;
    let options = parse_options(rest)?;
    match subcommand.as_str() {
        "codec-format" => codec_format(&options),
        "certify" => certify(&options),
        "prune-rate" => prune_rate(&options),
        "oracle-reference" => local_index::oracle_reference(&options),
        "latency" => local_index::measured_child("latency", &options),
        "build-memory" => local_index::measured_child("build-memory", &options),
        "latency-child" => local_index::latency_child(&options),
        "build-memory-child" => local_index::memory_child(&options),
        "index" => index_quality::run(&options),
        "index-diagnose" => index_quality::diagnose(&options),
        "dataset-oracle" => index_quality::dataset_oracle(&options),
        "original-rerank" => original_rerank::run(&options),
        "norm-audit" => norm_audit::run(&options),
        other => Err(BenchError::UnknownSubcommand(other.to_owned())),
    }
}

fn codec_format(options: &Options) -> Result<(), BenchError> {
    let corpus = options.require("corpus")?;
    let query_count: usize = options.require_parsed("queries")?;
    let seed: u64 = options.require_parsed("seed")?;
    let budgets = options.require_list("candidate-budget")?;
    let layout = options.require("layout")?;
    let output = PathBuf::from(options.require("output")?);
    let cache_state = match options.get("cache-state") {
        Some(value) => CacheState::parse(value)
            .ok_or_else(|| BenchError::UnknownCacheState(value.to_owned()))?,
        None => CacheState::Warm,
    };

    if layout != LAYOUT_TILED_SOA_32 {
        return Err(BenchError::UnsupportedLayout(layout.to_owned()));
    }

    let descriptor = CorpusDescriptor::resolve(corpus).map_err(BenchError::corpus)?;
    let splits = descriptor
        .load(seed, query_count)
        .map_err(BenchError::corpus)?;
    let corpus_name = splits.name().to_owned();
    let corpus_hash = splits.hash().to_owned();
    let vector_count = splits.indexed().len() as u64;

    let run = CodecFormatRun::prepare(splits, seed).map_err(BenchError::harness)?;
    let outcomes = run.measure(&budgets).map_err(BenchError::harness)?;

    // The logical primary size is a property of the representation; the layout
    // computes it independently. A disagreement is an accounting bug, and it
    // must be visible rather than published.
    let logical_primary_bytes_per_vector = (DIMENSION / 2) as u64;
    if run.logical_primary_bytes() != logical_primary_bytes_per_vector * vector_count {
        return Err(BenchError::ByteAccounting {
            expected: logical_primary_bytes_per_vector * vector_count,
            recorded: run.logical_primary_bytes(),
        });
    }

    let profile = MachineProfile::capture();
    let revision = SourceRevision::capture();
    let timestamp = timestamp_rfc3339_utc();
    let command = format!(
        "spherra-bench codec-format --corpus {corpus} --queries {query_count} --seed {seed} \
         --candidate-budget {} --layout {layout}",
        budgets
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(","),
    );

    let measurements: Vec<CodecFormatMeasurement> = outcomes
        .iter()
        .map(|outcome| {
            let mut measurement = CodecFormatMeasurement {
                schema_version: SCHEMA_VERSION,
                timestamp: timestamp.clone(),
                git_commit: String::new(),
                dirty_worktree: true,
                os: String::new(),
                architecture: String::new(),
                cpu: String::new(),
                physical_memory_bytes: 0,
                rustc: String::new(),
                cargo_profile: String::new(),
                cache_state: cache_state.as_str().to_owned(),
                durability_mode: DURABILITY_MODE_NOT_APPLICABLE.to_owned(),
                command: command.clone(),
                seed,
                corpus_name: corpus_name.clone(),
                corpus_hash: corpus_hash.clone(),
                dimension: DIMENSION as u32,
                vector_count,
                query_count: query_count as u64,
                transform_id: run.transform_id(),
                codec_id: codec_id_hex(),
                scorer_version: run.scorer_version(),
                quantizer_id: run.quantizer_id(),
                pq_codebook_id: run.pq_codebook_id(),
                layout_id: LAYOUT_TILED_SOA_32.to_owned(),
                logical_primary_bytes_per_vector,
                physical_primary_bytes: run.physical_primary_bytes(),
                tail_padding_bytes: run.tail_padding_bytes(),
                logical_residual_bytes_per_vector: Pq96Code::BYTE_LEN as u64,
                header_bytes: run.header_bytes(),
                recall_at_10: outcome.recall_at_10,
                recall_at_100: outcome.recall_at_100,
                candidate_budget: outcome.candidate_budget,
                primary_scan_vectors_per_second: outcome.primary_scan_vectors_per_second,
                residual_reranks_per_second: outcome.residual_reranks_per_second,
                primary_bound_violation_count: outcome.primary_bound_violation_count,
                refined_bound_violation_count: outcome.refined_bound_violation_count,
                primary_bound_width_percentiles: outcome.primary_bound_width_percentiles,
                refined_bound_width_percentiles: outcome.refined_bound_width_percentiles,
            };
            measurement.apply_provenance(&profile, &revision);
            measurement
        })
        .collect();

    let document = serde_json::to_value(&measurements).map_err(BenchError::serialize)?;
    let schema = load_schema("codec-format-baseline.schema.json")?;
    validate_against_schema(&schema, &document)
        .map_err(|violations| BenchError::Schema(violations.to_string()))?;
    write_output(&output, &document)?;

    let violations: u64 = measurements
        .iter()
        .map(|measurement| {
            measurement.primary_bound_violation_count + measurement.refined_bound_violation_count
        })
        .sum();
    if violations > 0 {
        return Err(BenchError::BoundViolations(violations));
    }

    println!(
        "wrote {} measurement(s) to {}",
        measurements.len(),
        output.display()
    );
    Ok(())
}

/// Measures what certified bounds alone can prove out of the top-k, with no
/// candidate budget and no rerank. See
/// `docs/experiments/2026-09-12-certified-prune-rate-preregistration.md` for
/// the decision rule this feeds.
fn prune_rate(options: &Options) -> Result<(), BenchError> {
    let corpus = options.require("corpus")?;
    let query_count: usize = options.require_parsed("queries")?;
    let seed: u64 = options.require_parsed("seed")?;
    let k: usize = options.require_parsed("k")?;
    let output = PathBuf::from(options.require("output")?);

    let descriptor = CorpusDescriptor::resolve(corpus).map_err(BenchError::corpus)?;
    let splits = descriptor
        .load(seed, query_count)
        .map_err(BenchError::corpus)?;
    let corpus_name = splits.name().to_owned();
    let corpus_hash = splits.hash().to_owned();

    let run = CodecFormatRun::prepare(splits, seed).map_err(BenchError::harness)?;
    let outcome = run.measure_prune(k).map_err(BenchError::harness)?;

    let profile = MachineProfile::capture();
    let revision = SourceRevision::capture();
    let document = serde_json::json!({
        "timestamp": timestamp_rfc3339_utc(),
        "git_commit": revision.commit,
        "dirty_worktree": revision.dirty,
        "cpu": profile.cpu,
        "command": format!(
            "spherra-bench prune-rate --corpus {corpus} --queries {query_count} \
             --seed {seed} --k {k}"
        ),
        "seed": seed,
        "corpus_name": corpus_name,
        "corpus_hash": corpus_hash,
        "transform_id": run.transform_id(),
        "quantizer_id": run.quantizer_id(),
        "pq_codebook_id": run.pq_codebook_id(),
        "codec_id": codec_id_hex(),
        "scorer_version": run.scorer_version(),
        "outcome": &outcome,
    });
    write_output(&output, &document)?;
    report(&outcome);

    if outcome.soundness_failures > 0 {
        return Err(BenchError::BoundViolations(outcome.soundness_failures));
    }
    Ok(())
}

fn report(outcome: &PruneOutcome) {
    println!(
        "corpus rows {}, queries {}, k {}",
        outcome.row_count, outcome.query_count, outcome.k
    );
    println!(
        "survivors   p50 {:.0}  p90 {:.0}  p99 {:.0}  max {:.0}",
        outcome.survivors.p50, outcome.survivors.p90, outcome.survivors.p99, outcome.survivors.max
    );
    println!(
        "prune rate  p50 {:.4}  p90 {:.4}  p99 {:.4}  max {:.4}",
        outcome.prune_rate.p50,
        outcome.prune_rate.p90,
        outcome.prune_rate.p99,
        outcome.prune_rate.max
    );
    println!(
        "per-row     p50 {:.0}  p90 {:.0}  p99 {:.0}  max {:.0}   (prune p50 {:.4})",
        outcome.per_row_survivors.p50,
        outcome.per_row_survivors.p90,
        outcome.per_row_survivors.p99,
        outcome.per_row_survivors.max,
        outcome.per_row_prune_rate.p50
    );
    println!(
        "per-row epsilon  p50 {:.4e}  p90 {:.4e}  p99 {:.4e}  max {:.4e}",
        outcome.per_row_epsilon.p50,
        outcome.per_row_epsilon.p90,
        outcome.per_row_epsilon.p99,
        outcome.per_row_epsilon.max
    );
    println!("soundness failures {}", outcome.soundness_failures);
    for (label, attribution) in [
        ("primary", &outcome.primary_attribution),
        ("refined", &outcome.refined_attribution),
    ] {
        let epsilon = attribution.epsilon;
        println!(
            "{label} epsilon {:.6e}  = transform {:.3e} ({:.4}%) + reconstruction {:.3e} ({:.4}%) \
             + serving {:.3e} ({:.4}%)",
            epsilon,
            attribution.transform_dot_term,
            100.0 * attribution.transform_dot_term / epsilon,
            attribution.reconstruction_term,
            100.0 * attribution.reconstruction_term / epsilon,
            attribution.serving_term,
            100.0 * attribution.serving_term / epsilon,
        );
    }
    println!(
        "observed |certified - truth|  p50 {:.3e}  p99 {:.3e}  max {:.3e}",
        outcome.observed_primary_error.p50,
        outcome.observed_primary_error.p99,
        outcome.observed_primary_error.max
    );
    println!(
        "tightness: certified epsilon is {:.1}x the largest observed error",
        outcome.primary_attribution.epsilon
            / outcome.observed_primary_error.max.max(f64::MIN_POSITIVE)
    );
}

/// The result schemas are part of the benchmark binary's behavior, so embed
/// them instead of silently skipping validation when the source tree is absent.
fn embedded_schema(name: &str) -> Result<&'static str, BenchError> {
    match name {
        "codec-format-baseline.schema.json" => Ok(include_str!(
            "../../../docs/benchmarks/codec-format-baseline.schema.json"
        )),
        "certificate-soak.schema.json" => Ok(include_str!(
            "../../../docs/benchmarks/certificate-soak.schema.json"
        )),
        _ => Err(BenchError::Schema(format!(
            "unknown embedded schema {name}"
        ))),
    }
}

fn load_schema(name: &str) -> Result<serde_json::Value, BenchError> {
    serde_json::from_str(embedded_schema(name)?).map_err(BenchError::serialize)
}

fn certify(options: &Options) -> Result<(), BenchError> {
    let requested_trials: u64 = options.require_parsed("trials")?;
    let root_seed: u64 = options.require_parsed("seed")?;
    let transform_seed_count: u32 = options.require_parsed("transform-seeds")?;
    let output = PathBuf::from(options.require("output")?);

    if requested_trials == 0 {
        return Err(BenchError::EmptySoak("trials"));
    }
    if transform_seed_count == 0 {
        return Err(BenchError::EmptySoak("transform-seeds"));
    }

    let transform_seeds: Vec<u64> = (0..u64::from(transform_seed_count))
        .map(|index| mix(root_seed, index.wrapping_mul(0x9e37_79b9_7f4a_7c15)))
        .collect();

    let scorer = FixedPointScorer::new();
    let mut completed = 0_u64;
    let mut primary_violations = 0_u64;
    let mut refined_violations = 0_u64;
    let mut maximum_primary_slack = 0.0_f64;
    let mut maximum_refined_slack = 0.0_f64;
    let mut first_failure = None;
    let mut seed_identities = Vec::with_capacity(transform_seed_count as usize);
    let started = Instant::now();

    'soak: for (seed_index, transform_seed) in transform_seeds.iter().copied().enumerate() {
        let plan = TransformPlan::from_seed(transform_seed);
        let trials_for_seed = trials_for_seed(requested_trials, transform_seed_count, seed_index);
        let (quantizer, codebook) = soak_fixtures(&plan, transform_seed)?;
        seed_identities.push(SoakSeedIdentity {
            transform_seed,
            transform_id: hex(plan.identity()),
            quantizer_id: hex(quantizer.identity()),
            pq_codebook_id: hex(codebook.codebook_id()),
        });

        for trial in 0..trials_for_seed {
            let vector_seed = mix(transform_seed, trial.wrapping_mul(2).wrapping_add(1));
            let query_seed = mix(transform_seed, trial.wrapping_mul(2).wrapping_add(2));
            let original = soak_vector(vector_seed);
            let query = soak_vector(query_seed);

            let transformed = transform_row(&plan, &original)?;
            let primary = quantizer.encode(&transformed);
            let residual_values = residual_of(&quantizer, &transformed);
            let residual = codebook
                .encode(&residual_values)
                .map_err(BenchError::soak)?;

            let block = ExhaustiveBlock::from_rows(
                CertificateBlockId::from_bytes([0x5a; 32]),
                1,
                [CertificateRow::new(0, &original, &primary, &residual)],
            )
            .map_err(BenchError::soak)?;
            let certificate =
                build_exhaustive_certificate(&scorer, &plan, &quantizer, &codebook, &block)
                    .map_err(BenchError::soak)?;
            let prepared = scorer
                .prepare_query(&plan, &query, &quantizer, &codebook)
                .map_err(BenchError::soak)?;
            let candidate = block.candidate(0).map_err(BenchError::soak)?;

            let normalized_query = normalize_fp64(&query).map_err(BenchError::soak)?;
            let normalized_original = normalize_fp64(&original).map_err(BenchError::soak)?;
            let truth = dot_f64(&normalized_query, &normalized_original);

            let primary_score = certificate
                .score_primary(&scorer, &prepared, &candidate)
                .map_err(BenchError::soak)?;
            let primary_value = primary_score.as_f64();
            let primary_bounds = certificate
                .primary_bounds(primary_score)
                .map_err(BenchError::soak)?;

            let prepared_candidate = codebook.prepare_candidate(PrimaryScore::for_row(0), residual);
            let refined_score = certificate
                .score_refined(&scorer, &prepared, &candidate, &prepared_candidate)
                .map_err(BenchError::soak)?;
            let refined_value = refined_score.as_f64();
            let refined_bounds = certificate
                .refined_bounds(refined_score)
                .map_err(BenchError::soak)?;

            completed += 1;

            if truth < primary_bounds.lower || truth > primary_bounds.upper {
                primary_violations += 1;
                first_failure = Some(SoakFailure {
                    trial: completed,
                    transform_seed: seed_index as u32,
                    vector_seed,
                    query_seed,
                    kind: SoakScoreKind::Primary,
                });
                break 'soak;
            }
            if truth < refined_bounds.lower || truth > refined_bounds.upper {
                refined_violations += 1;
                first_failure = Some(SoakFailure {
                    trial: completed,
                    transform_seed: seed_index as u32,
                    vector_seed,
                    query_seed,
                    kind: SoakScoreKind::Refined,
                });
                break 'soak;
            }

            maximum_primary_slack = maximum_primary_slack.max(normalized_slack(
                certificate.primary().epsilon(),
                primary_value,
                truth,
            ));
            maximum_refined_slack = maximum_refined_slack.max(normalized_slack(
                certificate.refined().epsilon(),
                refined_value,
                truth,
            ));

            if completed.is_multiple_of(SOAK_PROGRESS_INTERVAL) {
                eprintln!(
                    "certify: {completed}/{requested_trials} trials, {:.1}s elapsed",
                    started.elapsed().as_secs_f64()
                );
            }
        }
    }

    let profile = MachineProfile::capture();
    let revision = SourceRevision::capture();
    let mut result = CertificateSoakResult {
        schema_version: SCHEMA_VERSION,
        timestamp: timestamp_rfc3339_utc(),
        git_commit: String::new(),
        dirty_worktree: true,
        os: String::new(),
        architecture: String::new(),
        cpu: String::new(),
        physical_memory_bytes: 0,
        rustc: String::new(),
        cargo_profile: String::new(),
        command: format!(
            "spherra-bench certify --trials {requested_trials} --seed {root_seed} \
             --transform-seeds {transform_seed_count}"
        ),
        dimension: DIMENSION as u32,
        codec_id: codec_id_hex(),
        scorer_version: scorer.metadata().scorer_version(),
        layout_id: LAYOUT_TILED_SOA_32.to_owned(),
        root_seed,
        transform_seed_count,
        transform_seeds,
        seed_identities,
        requested_trials,
        completed_trials: completed,
        primary_violation_count: primary_violations,
        refined_violation_count: refined_violations,
        maximum_normalized_primary_slack: maximum_primary_slack,
        maximum_normalized_refined_slack: maximum_refined_slack,
        elapsed_seconds: started.elapsed().as_secs_f64(),
        first_failure,
    };
    result.apply_provenance(&profile, &revision);

    let document = serde_json::to_value(&result).map_err(BenchError::serialize)?;
    let schema = load_schema("certificate-soak.schema.json")?;
    validate_against_schema(&schema, &document)
        .map_err(|violations| BenchError::Schema(violations.to_string()))?;
    write_output(&output, &document)?;

    if let Some(failure) = result.first_failure {
        return Err(BenchError::EnclosureFailure(failure));
    }

    println!(
        "certify: {completed} trials enclosed across {} transform seed(s); wrote {}",
        result.transform_seed_count,
        output.display()
    );
    Ok(())
}

/// Spreads the requested trials across transform seeds, giving the earliest
/// seeds the remainder so the total always matches the request exactly.
fn trials_for_seed(requested: u64, seed_count: u32, index: usize) -> u64 {
    let seed_count = u64::from(seed_count);
    let base = requested / seed_count;
    let remainder = requested % seed_count;
    base + u64::from((index as u64) < remainder)
}

/// The unused fraction of the certified error budget: 1.0 means the bound was
/// entirely slack, 0.0 means the observed error consumed the whole budget.
fn normalized_slack(epsilon: f64, score: f64, truth: f64) -> f64 {
    if epsilon <= 0.0 || !epsilon.is_finite() {
        return 0.0;
    }
    ((epsilon - (score - truth).abs()) / epsilon).clamp(0.0, 1.0)
}

/// One deterministic quantizer/codebook pair per transform seed. Training per
/// trial would dominate the soak without testing anything the certificate
/// depends on.
fn soak_fixtures(
    plan: &TransformPlan,
    seed: u64,
) -> Result<(QuantizerTable, Pq96Codebook), BenchError> {
    let calibration: Vec<TransformedDirection> = (0..Pq96Code::CENTROIDS)
        .map(|index| transform_row(plan, &soak_vector(mix(seed, (index as u64) | 1 << 40))))
        .collect::<Result<_, _>>()?;
    let quantizer = QuantizerTable::train(
        &calibration
            .iter()
            .map(|direction| *direction.as_array())
            .collect::<Vec<_>>(),
    )
    .map_err(BenchError::soak)?;
    let residuals: Vec<[f32; DIMENSION]> = calibration
        .iter()
        .map(|direction| residual_of(&quantizer, direction))
        .collect();
    let codebook = Pq96Codebook::train(&residuals, seed).map_err(BenchError::soak)?;
    Ok((quantizer, codebook))
}

/// A deterministic finite original-space vector. SplitMix64 keeps each trial's
/// vector a pure function of its recorded seed, so a reported failure tuple
/// reproduces exactly.
fn soak_vector(seed: u64) -> [f32; DIMENSION] {
    let mut state = seed;
    std::array::from_fn(|_| {
        state = mix(state, 0x9e37_79b9_7f4a_7c15);
        let unit = (state >> 11) as f64 / ((1_u64 << 53) as f64);
        unit.mul_add(2.0, -1.0) as f32
    })
}

fn mix(seed: u64, salt: u64) -> u64 {
    let mut value = seed.wrapping_add(salt).wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn transform_row(
    plan: &TransformPlan,
    row: &[f32; DIMENSION],
) -> Result<TransformedDirection, BenchError> {
    let validated = ValidatedVector::new(row.to_vec()).map_err(BenchError::soak)?;
    let direction = validated
        .normalized_direction()
        .ok_or_else(|| BenchError::Soak("raw vector is direction-unreliable".to_owned()))?;
    Ok(transform(plan, direction))
}

fn residual_of(quantizer: &QuantizerTable, direction: &TransformedDirection) -> [f32; DIMENSION] {
    let reconstruction = quantizer.decode(&quantizer.encode(direction));
    let values = direction.as_array();
    std::array::from_fn(|coordinate| values[coordinate] - reconstruction[coordinate])
}

fn write_output(output: &Path, document: &serde_json::Value) -> Result<(), BenchError> {
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| BenchError::Output {
            path: output.to_path_buf(),
            message: source.to_string(),
        })?;
    }
    let mut text = serde_json::to_string_pretty(document).map_err(BenchError::serialize)?;
    text.push('\n');
    fs::write(output, text).map_err(|source| BenchError::Output {
        path: output.to_path_buf(),
        message: source.to_string(),
    })
}

/// `--name value` pairs. Nothing here is positional, so an argument typo is an
/// error rather than a silently defaulted measurement parameter.
struct Options(BTreeMap<String, String>);

fn parse_options(arguments: &[String]) -> Result<Options, BenchError> {
    let mut options = BTreeMap::new();
    let mut index = 0;
    while index < arguments.len() {
        let name = arguments[index]
            .strip_prefix("--")
            .ok_or_else(|| BenchError::UnexpectedArgument(arguments[index].clone()))?;
        let value = arguments
            .get(index + 1)
            .ok_or_else(|| BenchError::MissingValue(name.to_owned()))?;
        if value.starts_with("--") {
            return Err(BenchError::MissingValue(name.to_owned()));
        }
        options.insert(name.to_owned(), value.clone());
        index += 2;
    }
    Ok(Options(options))
}

impl Options {
    fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    fn require(&self, name: &str) -> Result<&str, BenchError> {
        self.get(name)
            .ok_or_else(|| BenchError::MissingOption(name.to_owned()))
    }

    fn require_parsed<T>(&self, name: &str) -> Result<T, BenchError>
    where
        T: std::str::FromStr,
    {
        self.require(name)?
            .parse()
            .map_err(|_| BenchError::UnparsableOption(name.to_owned()))
    }

    fn require_list(&self, name: &str) -> Result<Vec<u64>, BenchError> {
        self.require(name)?
            .split(',')
            .map(|entry| {
                entry
                    .trim()
                    .parse::<u64>()
                    .map_err(|_| BenchError::UnparsableOption(name.to_owned()))
            })
            .collect()
    }
}

#[derive(Debug)]
enum BenchError {
    MissingSubcommand,
    UnknownSubcommand(String),
    UnexpectedArgument(String),
    MissingOption(String),
    MissingValue(String),
    UnparsableOption(String),
    UnknownCacheState(String),
    UnsupportedLayout(String),
    EmptySoak(&'static str),
    Corpus(String),
    Harness(String),
    Soak(String),
    Schema(String),
    Serialize(String),
    Output { path: PathBuf, message: String },
    ByteAccounting { expected: u64, recorded: u64 },
    BoundViolations(u64),
    EnclosureFailure(SoakFailure),
}

impl BenchError {
    fn corpus(error: impl fmt::Display) -> Self {
        Self::Corpus(error.to_string())
    }

    fn harness(error: impl fmt::Display) -> Self {
        Self::Harness(error.to_string())
    }

    fn soak(error: impl fmt::Display) -> Self {
        Self::Soak(error.to_string())
    }

    fn serialize(error: impl fmt::Display) -> Self {
        Self::Serialize(error.to_string())
    }
}

impl fmt::Display for BenchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSubcommand => {
                write!(
                    formatter,
                    "expected a subcommand: codec-format, certify, prune-rate, oracle-reference, index, index-diagnose, dataset-oracle, original-rerank, norm-audit, latency, or build-memory"
                )
            }
            Self::UnknownSubcommand(name) => write!(formatter, "unknown subcommand {name}"),
            Self::UnexpectedArgument(argument) => {
                write!(formatter, "expected --name value, found {argument}")
            }
            Self::MissingOption(name) => write!(formatter, "missing required --{name}"),
            Self::MissingValue(name) => write!(formatter, "--{name} requires a value"),
            Self::UnparsableOption(name) => write!(formatter, "--{name} has an unparsable value"),
            Self::UnknownCacheState(value) => {
                write!(formatter, "--cache-state {value} is not cold, warm, or hot")
            }
            Self::UnsupportedLayout(value) => write!(
                formatter,
                "--layout {value} is not the measured {LAYOUT_TILED_SOA_32}",
            ),
            Self::EmptySoak(option) => write!(formatter, "--{option} must be greater than zero"),
            Self::Corpus(message) | Self::Harness(message) => write!(formatter, "{message}"),
            Self::Soak(message) => write!(formatter, "certificate soak failed: {message}"),
            Self::Schema(message) => write!(
                formatter,
                "generated result violates its own schema: {message}",
            ),
            Self::Serialize(message) => write!(formatter, "cannot serialize result: {message}"),
            Self::Output { path, message } => {
                write!(formatter, "cannot write {}: {message}", path.display())
            }
            Self::ByteAccounting { expected, recorded } => write!(
                formatter,
                "logical primary byte accounting disagrees: expected {expected}, layout reports {recorded}",
            ),
            Self::BoundViolations(count) => write!(
                formatter,
                "{count} certificate bound violation(s); the result file records them",
            ),
            Self::EnclosureFailure(failure) => write!(
                formatter,
                "certificate enclosure failed at trial {} ({:?}) with transform seed index {}, vector seed {}, query seed {}",
                failure.trial,
                failure.kind,
                failure.transform_seed,
                failure.vector_seed,
                failure.query_seed,
            ),
        }
    }
}

impl std::error::Error for BenchError {}

#[cfg(test)]
mod tests {
    use core::array;

    use spherra_domain::{DIMENSION, ValidatedVector};

    use super::{TransformPlan, embedded_schema, transform, transform_row};

    #[test]
    fn measurement_schemas_are_embedded_in_the_binary() {
        assert!(embedded_schema("codec-format-baseline.schema.json").is_ok());
        assert!(embedded_schema("certificate-soak.schema.json").is_ok());
        assert!(embedded_schema("unknown.schema.json").is_err());
    }

    #[test]
    fn soak_rows_follow_the_single_normalization_ingest_path() {
        let plan = TransformPlan::from_seed(0x73_6f_61_6b);
        let mut state = 0x1234_5678_u64;
        let raw = (0..128)
            .find_map(|seed| {
                let values: [f64; DIMENSION] = array::from_fn(|coordinate| {
                    state = state
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407);
                    let fraction = ((state >> 40) as u32) as f64 / (1_u32 << 24) as f64;
                    let exponent = ((coordinate * 37 + seed * 19) % 151) as i32 - 75;
                    fraction.mul_add(2.0, -1.0) * 2.0_f64.powi(exponent)
                });
                let norm = values
                    .iter()
                    .fold(0.0, |sum, value| value.mul_add(*value, sum))
                    .sqrt();
                let raw = array::from_fn(|coordinate| (values[coordinate] / norm) as f32);
                let first = ValidatedVector::new(raw.to_vec()).ok()?;
                let first = first.normalized_direction()?.as_array();
                let second = ValidatedVector::new(first.to_vec()).ok()?;
                (first != second.normalized_direction()?.as_array()).then_some(raw)
            })
            .expect("fixture search finds a vector whose second normalization drifts");
        let validated = ValidatedVector::new(raw.to_vec()).expect("finite 768D fixture");
        let expected = transform(
            &plan,
            validated
                .normalized_direction()
                .expect("fixture has a reliable direction"),
        );

        assert_eq!(
            transform_row(&plan, &raw).expect("fixture transforms"),
            expected,
            "the soak row was normalized more than once",
        );
    }
}
