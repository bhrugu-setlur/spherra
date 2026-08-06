//! The measurement result schema is evidence policy, not formatting taste.
//!
//! A baseline JSON that silently omits an identity, corpus, byte-accounting, or
//! bound-soundness field cannot be audited later, so these tests pin the exact
//! required field set and prove that dropping any one of them is rejected.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use spherra_testkit::results::{
    CertificateSoakResult, CodecFormatMeasurement, SCHEMA_VERSION, validate_against_schema,
};

/// The exact fields the active plan requires of every codec/format baseline
/// entry, written out independently of the schema document so that a field
/// quietly dropped from either side fails.
const REQUIRED_MEASUREMENT_FIELDS: [&str; 39] = [
    "schema_version",
    "timestamp",
    "git_commit",
    "dirty_worktree",
    "os",
    "architecture",
    "cpu",
    "physical_memory_bytes",
    "rustc",
    "cargo_profile",
    "cache_state",
    "durability_mode",
    "command",
    "seed",
    "corpus_name",
    "corpus_hash",
    "dimension",
    "vector_count",
    "query_count",
    "transform_id",
    "codec_id",
    "scorer_version",
    "quantizer_id",
    "pq_codebook_id",
    "layout_id",
    "logical_primary_bytes_per_vector",
    "physical_primary_bytes",
    "tail_padding_bytes",
    "logical_residual_bytes_per_vector",
    "header_bytes",
    "recall_at_10",
    "recall_at_100",
    "candidate_budget",
    "primary_scan_vectors_per_second",
    "residual_reranks_per_second",
    "primary_bound_violation_count",
    "refined_bound_violation_count",
    "primary_bound_width_percentiles",
    "refined_bound_width_percentiles",
];

const REQUIRED_SOAK_FIELDS: [&str; 22] = [
    "schema_version",
    "timestamp",
    "git_commit",
    "dirty_worktree",
    "os",
    "architecture",
    "cpu",
    "physical_memory_bytes",
    "rustc",
    "cargo_profile",
    "command",
    "root_seed",
    "transform_seed_count",
    "transform_seeds",
    "requested_trials",
    "completed_trials",
    "primary_violation_count",
    "refined_violation_count",
    "maximum_normalized_primary_slack",
    "maximum_normalized_refined_slack",
    "elapsed_seconds",
    "first_failure",
];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the bench crate lives two directories below the repository root")
        .to_path_buf()
}

fn load_schema(name: &str) -> Value {
    let path = repository_root().join("docs/benchmarks").join(name);
    let bytes = fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "the checked-in schema {} must be readable: {error}",
            path.display()
        )
    });
    serde_json::from_slice(&bytes).expect("the checked-in schema is valid JSON")
}

fn measurement_schema() -> Value {
    load_schema("codec-format-baseline.schema.json")
}

fn soak_schema() -> Value {
    load_schema("certificate-soak.schema.json")
}

fn sample_measurement() -> Value {
    serde_json::to_value(CodecFormatMeasurement::sample())
        .expect("a measurement serializes without error")
}

fn sample_soak() -> Value {
    serde_json::to_value(CertificateSoakResult::sample())
        .expect("a soak result serializes without error")
}

#[test]
fn generated_measurement_validates_against_the_checked_in_schema() {
    let document = json!([sample_measurement()]);
    validate_against_schema(&measurement_schema(), &document)
        .expect("a generated measurement satisfies the checked-in schema");
}

#[test]
fn generated_soak_result_validates_against_the_checked_in_schema() {
    validate_against_schema(&soak_schema(), &sample_soak())
        .expect("a generated soak result satisfies the checked-in schema");
}

#[test]
fn schema_requires_every_planned_measurement_field() {
    let schema = measurement_schema();
    let required = schema["$defs"]["measurement"]["required"]
        .as_array()
        .expect("the measurement definition lists required fields");
    let mut required: Vec<&str> = required
        .iter()
        .map(|value| value.as_str().expect("required entries are strings"))
        .collect();
    let mut expected: Vec<&str> = REQUIRED_MEASUREMENT_FIELDS.to_vec();
    required.sort_unstable();
    expected.sort_unstable();
    assert_eq!(
        required, expected,
        "the schema's required measurement fields drifted from the active plan"
    );
}

#[test]
fn schema_requires_every_planned_soak_field() {
    let schema = soak_schema();
    let required = schema["required"]
        .as_array()
        .expect("the soak schema lists required fields");
    let mut required: Vec<&str> = required
        .iter()
        .map(|value| value.as_str().expect("required entries are strings"))
        .collect();
    let mut expected: Vec<&str> = REQUIRED_SOAK_FIELDS.to_vec();
    required.sort_unstable();
    expected.sort_unstable();
    assert_eq!(
        required, expected,
        "the schema's required soak fields drifted from the active plan"
    );
}

#[test]
fn omitting_any_required_measurement_field_is_rejected() {
    let schema = measurement_schema();
    for field in REQUIRED_MEASUREMENT_FIELDS {
        let mut measurement = sample_measurement();
        measurement
            .as_object_mut()
            .expect("a measurement is a JSON object")
            .remove(field)
            .unwrap_or_else(|| panic!("the generated measurement must contain {field}"));
        let document = json!([measurement]);
        assert!(
            validate_against_schema(&schema, &document).is_err(),
            "a measurement missing {field} must be rejected"
        );
    }
}

#[test]
fn omitting_any_required_soak_field_is_rejected() {
    let schema = soak_schema();
    for field in REQUIRED_SOAK_FIELDS {
        let mut soak = sample_soak();
        soak.as_object_mut()
            .expect("a soak result is a JSON object")
            .remove(field)
            .unwrap_or_else(|| panic!("the generated soak result must contain {field}"));
        assert!(
            validate_against_schema(&schema, &soak).is_err(),
            "a soak result missing {field} must be rejected"
        );
    }
}

#[test]
fn nested_percentile_summaries_require_every_quantile() {
    let schema = measurement_schema();
    for summary in [
        "primary_bound_width_percentiles",
        "refined_bound_width_percentiles",
    ] {
        for quantile in ["p50", "p90", "p99", "max"] {
            let mut measurement = sample_measurement();
            measurement[summary]
                .as_object_mut()
                .expect("a percentile summary is a JSON object")
                .remove(quantile)
                .unwrap_or_else(|| panic!("{summary} must contain {quantile}"));
            let document = json!([measurement]);
            assert!(
                validate_against_schema(&schema, &document).is_err(),
                "{summary} missing {quantile} must be rejected"
            );
        }
    }
}

#[test]
fn unknown_and_mistyped_measurement_fields_are_rejected() {
    let schema = measurement_schema();

    let mut extra = sample_measurement();
    extra["undeclared_field"] = json!(1);
    assert!(
        validate_against_schema(&schema, &json!([extra])).is_err(),
        "an undeclared measurement field must be rejected"
    );

    let mut mistyped = sample_measurement();
    mistyped["vector_count"] = json!("20000");
    assert!(
        validate_against_schema(&schema, &json!([mistyped])).is_err(),
        "a string vector count must be rejected"
    );

    let mut fractional = sample_measurement();
    fractional["candidate_budget"] = json!(10.5);
    assert!(
        validate_against_schema(&schema, &json!([fractional])).is_err(),
        "a fractional candidate budget must be rejected"
    );

    let mut negative = sample_measurement();
    negative["primary_bound_violation_count"] = json!(-1);
    assert!(
        validate_against_schema(&schema, &json!([negative])).is_err(),
        "a negative bound-violation count must be rejected"
    );

    let mut unknown_cache = sample_measurement();
    unknown_cache["cache_state"] = json!("lukewarm");
    assert!(
        validate_against_schema(&schema, &json!([unknown_cache])).is_err(),
        "an undeclared cache state must be rejected"
    );
}

#[test]
fn a_result_document_must_be_a_non_empty_array_of_measurements() {
    let schema = measurement_schema();
    assert!(
        validate_against_schema(&schema, &json!([])).is_err(),
        "an empty result array is not admissible evidence"
    );
    assert!(
        validate_against_schema(&schema, &sample_measurement()).is_err(),
        "a bare object is not a result document"
    );
}

#[test]
fn generated_results_declare_the_current_schema_version() {
    assert_eq!(
        sample_measurement()["schema_version"],
        json!(SCHEMA_VERSION)
    );
    assert_eq!(sample_soak()["schema_version"], json!(SCHEMA_VERSION));
}
