//! The recorded shape of a measurement, and the checker that enforces it.
//!
//! The structs below are the only writers of benchmark evidence, and the
//! checked-in JSON Schema documents under `docs/benchmarks/` are the contract
//! they must satisfy. [`validate_against_schema`] interprets those documents
//! directly, so a field added to one side and not the other fails the contract
//! test rather than producing an unauditable result file.
//!
//! The checker deliberately implements only the JSON Schema keywords these
//! documents use — `type`, `required`, `properties`, `additionalProperties:
//! false`, `items`, `minItems`, `minimum`, `enum`, `oneOf`, and local `$ref`
//! into `$defs`. An unrecognized keyword is an error rather than a silent pass,
//! so the schema cannot quietly grow a constraint that nothing enforces.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::machine::{DURABILITY_MODE_NOT_APPLICABLE, MachineProfile, SourceRevision};

pub const SCHEMA_VERSION: u32 = 1;

/// The four order statistics recorded for a certificate bound width.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct PercentileSummary {
    pub p50: f64,
    pub p90: f64,
    pub p99: f64,
    pub max: f64,
}

impl PercentileSummary {
    /// Nearest-rank percentiles over the observed sample. An empty sample is
    /// reported as all zeroes, which only happens when no candidate was scored.
    pub fn from_samples(samples: &mut [f64]) -> Self {
        if samples.is_empty() {
            return Self {
                p50: 0.0,
                p90: 0.0,
                p99: 0.0,
                max: 0.0,
            };
        }
        samples.sort_by(f64::total_cmp);
        Self {
            p50: nearest_rank(samples, 0.50),
            p90: nearest_rank(samples, 0.90),
            p99: nearest_rank(samples, 0.99),
            max: samples[samples.len() - 1],
        }
    }
}

fn nearest_rank(sorted: &[f64], quantile: f64) -> f64 {
    let rank = (quantile * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

/// One recorded codec/format baseline entry: one candidate budget, on one
/// corpus, under one representation identity.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct CodecFormatMeasurement {
    pub schema_version: u32,
    pub timestamp: String,
    pub git_commit: String,
    pub dirty_worktree: bool,
    pub os: String,
    pub architecture: String,
    pub cpu: String,
    pub physical_memory_bytes: u64,
    pub rustc: String,
    pub cargo_profile: String,
    pub cache_state: String,
    pub durability_mode: String,
    pub command: String,
    pub seed: u64,
    pub corpus_name: String,
    pub corpus_hash: String,
    pub dimension: u32,
    pub vector_count: u64,
    pub query_count: u64,
    pub transform_id: String,
    pub codec_id: String,
    pub scorer_version: u32,
    pub quantizer_id: String,
    pub pq_codebook_id: String,
    pub layout_id: String,
    pub logical_primary_bytes_per_vector: u64,
    pub physical_primary_bytes: u64,
    pub tail_padding_bytes: u64,
    pub logical_residual_bytes_per_vector: u64,
    pub header_bytes: u64,
    pub recall_at_10: f64,
    pub recall_at_100: f64,
    pub candidate_budget: u64,
    pub primary_scan_vectors_per_second: f64,
    pub residual_reranks_per_second: f64,
    pub primary_bound_violation_count: u64,
    pub refined_bound_violation_count: u64,
    pub primary_bound_width_percentiles: PercentileSummary,
    pub refined_bound_width_percentiles: PercentileSummary,
}

impl CodecFormatMeasurement {
    /// A structurally complete measurement used by the schema contract test.
    ///
    /// Its numbers are placeholders; its field set is not.
    pub fn sample() -> Self {
        let percentiles = PercentileSummary {
            p50: 0.001,
            p90: 0.002,
            p99: 0.003,
            max: 0.004,
        };
        Self {
            schema_version: SCHEMA_VERSION,
            timestamp: "2026-08-05T00:00:00Z".to_owned(),
            git_commit: "0".repeat(40),
            dirty_worktree: false,
            os: "Darwin 25.5.0".to_owned(),
            architecture: "aarch64".to_owned(),
            cpu: "Apple M1 Pro".to_owned(),
            physical_memory_bytes: 34_359_738_368,
            rustc: "rustc 1.88.0".to_owned(),
            cargo_profile: "release".to_owned(),
            cache_state: "warm".to_owned(),
            durability_mode: DURABILITY_MODE_NOT_APPLICABLE.to_owned(),
            command: "spherra-bench codec-format".to_owned(),
            seed: 20_260_804,
            corpus_name: "generated-correlated-768x20000".to_owned(),
            corpus_hash: "0".repeat(64),
            dimension: 768,
            vector_count: 20_000,
            query_count: 200,
            transform_id: "0".repeat(64),
            codec_id: "0".repeat(64),
            scorer_version: 1,
            quantizer_id: "0".repeat(64),
            pq_codebook_id: "0".repeat(64),
            layout_id: "tiled-soa-32".to_owned(),
            logical_primary_bytes_per_vector: 384,
            physical_primary_bytes: 7_680_000,
            tail_padding_bytes: 0,
            logical_residual_bytes_per_vector: 96,
            header_bytes: 744,
            recall_at_10: 0.9,
            recall_at_100: 0.5,
            candidate_budget: 100,
            primary_scan_vectors_per_second: 1_000_000.0,
            residual_reranks_per_second: 500_000.0,
            primary_bound_violation_count: 0,
            refined_bound_violation_count: 0,
            primary_bound_width_percentiles: percentiles,
            refined_bound_width_percentiles: percentiles,
        }
    }
}

/// The reproducible seed tuple of the first certificate enclosure failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct SoakFailure {
    pub trial: u64,
    pub transform_seed: u32,
    pub vector_seed: u64,
    pub query_seed: u64,
    pub kind: SoakScoreKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SoakScoreKind {
    Primary,
    Refined,
}

/// Durable identities of the transform and trained tables used for one soak seed.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct SoakSeedIdentity {
    pub transform_seed: u64,
    pub transform_id: String,
    pub quantizer_id: String,
    pub pq_codebook_id: String,
}

/// The result of `spherra-bench certify`.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct CertificateSoakResult {
    pub schema_version: u32,
    pub timestamp: String,
    pub git_commit: String,
    pub dirty_worktree: bool,
    pub os: String,
    pub architecture: String,
    pub cpu: String,
    pub physical_memory_bytes: u64,
    pub rustc: String,
    pub cargo_profile: String,
    pub command: String,
    pub dimension: u32,
    pub codec_id: String,
    pub scorer_version: u32,
    pub layout_id: String,
    pub root_seed: u64,
    pub transform_seed_count: u32,
    pub transform_seeds: Vec<u64>,
    pub seed_identities: Vec<SoakSeedIdentity>,
    pub requested_trials: u64,
    pub completed_trials: u64,
    pub primary_violation_count: u64,
    pub refined_violation_count: u64,
    pub maximum_normalized_primary_slack: f64,
    pub maximum_normalized_refined_slack: f64,
    pub elapsed_seconds: f64,
    pub first_failure: Option<SoakFailure>,
}

impl CertificateSoakResult {
    pub fn sample() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            timestamp: "2026-08-05T00:00:00Z".to_owned(),
            git_commit: "0".repeat(40),
            dirty_worktree: false,
            os: "Darwin 25.5.0".to_owned(),
            architecture: "aarch64".to_owned(),
            cpu: "Apple M1 Pro".to_owned(),
            physical_memory_bytes: 34_359_738_368,
            rustc: "rustc 1.88.0".to_owned(),
            cargo_profile: "release".to_owned(),
            command: "spherra-bench certify".to_owned(),
            dimension: 768,
            codec_id: "0".repeat(64),
            scorer_version: 1,
            layout_id: "tiled-soa-32".to_owned(),
            root_seed: 20_260_804,
            transform_seed_count: 4,
            transform_seeds: vec![0, 1, 2, 3],
            seed_identities: (0..4)
                .map(|transform_seed| SoakSeedIdentity {
                    transform_seed,
                    transform_id: "0".repeat(64),
                    quantizer_id: "0".repeat(64),
                    pq_codebook_id: "0".repeat(64),
                })
                .collect(),
            requested_trials: 2_000_000,
            completed_trials: 2_000_000,
            primary_violation_count: 0,
            refined_violation_count: 0,
            maximum_normalized_primary_slack: 0.25,
            maximum_normalized_refined_slack: 0.125,
            elapsed_seconds: 610.5,
            first_failure: None,
        }
    }
}

impl CodecFormatMeasurement {
    /// Applies a machine and revision capture in one place, so no caller can
    /// record half a provenance.
    pub fn apply_provenance(&mut self, profile: &MachineProfile, revision: &SourceRevision) {
        self.os = profile.os.clone();
        self.architecture = profile.architecture.clone();
        self.cpu = profile.cpu.clone();
        self.physical_memory_bytes = profile.physical_memory_bytes;
        self.rustc = profile.rustc.clone();
        self.cargo_profile = profile.cargo_profile.clone();
        self.git_commit = revision.commit.clone();
        self.dirty_worktree = revision.dirty;
    }
}

impl CertificateSoakResult {
    pub fn apply_provenance(&mut self, profile: &MachineProfile, revision: &SourceRevision) {
        self.os = profile.os.clone();
        self.architecture = profile.architecture.clone();
        self.cpu = profile.cpu.clone();
        self.physical_memory_bytes = profile.physical_memory_bytes;
        self.rustc = profile.rustc.clone();
        self.cargo_profile = profile.cargo_profile.clone();
        self.git_commit = revision.commit.clone();
        self.dirty_worktree = revision.dirty;
    }
}

/// Every constraint an instance violated, in discovery order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaViolations(Vec<String>);

impl SchemaViolations {
    pub fn messages(&self) -> &[String] {
        &self.0
    }
}

impl fmt::Display for SchemaViolations {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "schema validation failed: {}", self.0.join("; "))
    }
}

impl std::error::Error for SchemaViolations {}

/// Validates `instance` against `schema`, which must be one of the checked-in
/// measurement schema documents.
pub fn validate_against_schema(schema: &Value, instance: &Value) -> Result<(), SchemaViolations> {
    let mut violations = Vec::new();
    check(schema, schema, instance, "$", &mut violations);
    if violations.is_empty() {
        Ok(())
    } else {
        Err(SchemaViolations(violations))
    }
}

/// Keywords the checker understands. Anything else in a schema object is a
/// violation of the checker's own contract, reported against the instance so it
/// cannot pass unnoticed.
const KNOWN_KEYWORDS: [&str; 16] = [
    "$schema",
    "$id",
    "$ref",
    "$defs",
    "title",
    "description",
    "type",
    "required",
    "properties",
    "additionalProperties",
    "items",
    "minItems",
    "minimum",
    "enum",
    "oneOf",
    "examples",
];

fn check(root: &Value, schema: &Value, instance: &Value, path: &str, violations: &mut Vec<String>) {
    let Some(object) = schema.as_object() else {
        violations.push(format!("{path}: schema fragment is not an object"));
        return;
    };

    for keyword in object.keys() {
        if !KNOWN_KEYWORDS.contains(&keyword.as_str()) {
            violations.push(format!("{path}: unsupported schema keyword {keyword}"));
        }
    }

    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
        match resolve(root, reference) {
            Some(target) => check(root, target, instance, path, violations),
            None => violations.push(format!("{path}: unresolvable $ref {reference}")),
        }
        return;
    }

    if let Some(options) = object.get("oneOf").and_then(Value::as_array) {
        let matches = options
            .iter()
            .filter(|option| {
                let mut nested = Vec::new();
                check(root, option, instance, path, &mut nested);
                nested.is_empty()
            })
            .count();
        if matches != 1 {
            violations.push(format!(
                "{path}: matched {matches} oneOf branches, expected exactly one"
            ));
        }
        return;
    }

    if let Some(expected) = object.get("type").and_then(Value::as_str) {
        if !matches_type(expected, instance) {
            violations.push(format!("{path}: expected type {expected}"));
            return;
        }
    }

    if let Some(allowed) = object.get("enum").and_then(Value::as_array) {
        if !allowed.contains(instance) {
            violations.push(format!("{path}: value is not one of the allowed constants"));
        }
    }

    if let Some(minimum) = object.get("minimum").and_then(Value::as_f64) {
        if let Some(value) = instance.as_f64() {
            if value < minimum {
                violations.push(format!("{path}: {value} is below the minimum {minimum}"));
            }
        }
    }

    if let Some(instance_object) = instance.as_object() {
        let properties = object.get("properties").and_then(Value::as_object);

        for required in object
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !instance_object.contains_key(required) {
                violations.push(format!("{path}: missing required property {required}"));
            }
        }

        let additional_allowed = object
            .get("additionalProperties")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        for (name, value) in instance_object {
            match properties.and_then(|properties| properties.get(name)) {
                Some(property_schema) => {
                    check(
                        root,
                        property_schema,
                        value,
                        &format!("{path}.{name}"),
                        violations,
                    );
                }
                None if !additional_allowed => {
                    violations.push(format!("{path}: undeclared property {name}"));
                }
                None => {}
            }
        }
    }

    if let Some(instance_array) = instance.as_array() {
        if let Some(minimum) = object.get("minItems").and_then(Value::as_u64) {
            if (instance_array.len() as u64) < minimum {
                violations.push(format!("{path}: needs at least {minimum} items"));
            }
        }
        if let Some(item_schema) = object.get("items") {
            for (index, item) in instance_array.iter().enumerate() {
                check(
                    root,
                    item_schema,
                    item,
                    &format!("{path}[{index}]"),
                    violations,
                );
            }
        }
    }
}

fn matches_type(expected: &str, instance: &Value) -> bool {
    match expected {
        "object" => instance.is_object(),
        "array" => instance.is_array(),
        "string" => instance.is_string(),
        "boolean" => instance.is_boolean(),
        "null" => instance.is_null(),
        "number" => instance.is_number(),
        // A JSON number is an integer only if it carries no fractional part;
        // `10.5` must not satisfy an integer field.
        "integer" => instance.is_i64() || instance.is_u64(),
        _ => false,
    }
}

fn resolve<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let path = reference.strip_prefix("#/")?;
    let mut current = root;
    for segment in path.split('/') {
        current = current.get(segment)?;
    }
    Some(current)
}
