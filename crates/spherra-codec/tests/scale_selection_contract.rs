use core::array;

use spherra_codec::{
    FixedPointScorer, Pq96Code, Pq96Codebook, QuantizerTable, TransformPlan, TransformedDirection,
    transform,
};
use spherra_domain::{DIMENSION, ValidatedVector};

const TRANSFORM_SEED: u64 = 0x5153_4341_4c45_5f31;
const CALIBRATION_SEED: u64 = 0x5153_4341_4c45_5f32;
const PQ_TRAINING_SEED: u64 = 0x5153_4341_4c45_5f33;
const CALIBRATION_ROWS: usize = Pq96Code::CENTROIDS;
const QUERY_SEEDS: [u64; 3] = [
    0x5153_4341_4c45_6001,
    0x5153_4341_4c45_6002,
    0x5153_4341_4c45_6003,
];

fn deterministic_raw_vector(seed: u64) -> [f32; DIMENSION] {
    let mut state = seed;
    array::from_fn(|coordinate| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let fraction = ((state >> 40) as u32) as f32 / ((1_u32 << 24) as f32);
        fraction.mul_add(2.0, -1.0) + coordinate as f32 * 0.000_001
    })
}

fn transformed_from_raw(plan: &TransformPlan, raw: &[f32; DIMENSION]) -> TransformedDirection {
    let validated =
        ValidatedVector::new(raw.to_vec()).expect("the deterministic scale fixture is finite");
    transform(
        plan,
        validated
            .normalized_direction()
            .expect("the deterministic scale fixture is non-zero"),
    )
}

fn render_artifact(
    scorer: FixedPointScorer,
    maximum_primary_lookup_entry: i64,
    maximum_residual_lookup_entry: i64,
) -> String {
    let metadata = scorer.metadata();
    let maximum_lookup_entry = maximum_primary_lookup_entry.max(maximum_residual_lookup_entry);
    let measured_worst_case_sum = maximum_lookup_entry
        .checked_mul(metadata.refined_terms() as i64)
        .expect("the measured Q24 table entries fit in i64");
    format!(
        concat!(
            "{{\n",
            "  \"artifact_version\": 1,\n",
            "  \"fixture\": {{\n",
            "    \"generator\": \"scale-probe-lcg64-v1\",\n",
            "    \"transform_seed\": \"0x{transform_seed:016x}\",\n",
            "    \"calibration_seed\": \"0x{calibration_seed:016x}\",\n",
            "    \"calibration_rows\": {calibration_rows},\n",
            "    \"pq_training_seed\": \"0x{pq_training_seed:016x}\",\n",
            "    \"query_seeds\": [\"0x{query_0:016x}\", \"0x{query_1:016x}\", \"0x{query_2:016x}\"]\n",
            "  }},\n",
            "  \"selected_fractional_bits\": {fractional_bits},\n",
            "  \"comparison_scale\": {comparison_scale},\n",
            "  \"primary_terms\": {primary_terms},\n",
            "  \"residual_terms\": {residual_terms},\n",
            "  \"refined_terms\": {refined_terms},\n",
            "  \"measured_maximum_absolute_primary_lookup_entry\": {maximum_primary_lookup_entry},\n",
            "  \"measured_maximum_absolute_residual_lookup_entry\": {maximum_residual_lookup_entry},\n",
            "  \"measured_maximum_absolute_lookup_entry\": {maximum_lookup_entry},\n",
            "  \"measured_worst_case_absolute_i64_sum\": {measured_worst_case_sum},\n",
            "  \"admissible_maximum_absolute_table_entry\": {admissible_maximum_table_entry},\n",
            "  \"admissible_worst_case_absolute_i64_sum\": {admissible_worst_case_sum},\n",
            "  \"i64_max\": {i64_max}\n",
            "}}\n"
        ),
        transform_seed = TRANSFORM_SEED,
        calibration_seed = CALIBRATION_SEED,
        calibration_rows = CALIBRATION_ROWS,
        pq_training_seed = PQ_TRAINING_SEED,
        query_0 = QUERY_SEEDS[0],
        query_1 = QUERY_SEEDS[1],
        query_2 = QUERY_SEEDS[2],
        maximum_primary_lookup_entry = maximum_primary_lookup_entry,
        maximum_residual_lookup_entry = maximum_residual_lookup_entry,
        maximum_lookup_entry = maximum_lookup_entry,
        measured_worst_case_sum = measured_worst_case_sum,
        fractional_bits = metadata.fractional_bits(),
        comparison_scale = metadata.comparison_scale(),
        primary_terms = metadata.primary_terms(),
        residual_terms = Pq96Code::SUBQUANTIZERS,
        refined_terms = metadata.refined_terms(),
        admissible_maximum_table_entry = metadata.maximum_absolute_table_entry(),
        admissible_worst_case_sum = metadata.worst_case_refined_sum(),
        i64_max = i64::MAX,
    )
}

#[test]
fn q24_scale_selection_snapshot_is_reproducible_and_i64_safe() {
    let plan = TransformPlan::from_seed(TRANSFORM_SEED);
    let transformed_calibration: Vec<_> = (0..CALIBRATION_ROWS)
        .map(|row| {
            transformed_from_raw(
                &plan,
                &deterministic_raw_vector(CALIBRATION_SEED.wrapping_add(row as u64)),
            )
        })
        .collect();
    let table = QuantizerTable::train(
        &transformed_calibration
            .iter()
            .map(|direction| *direction.as_array())
            .collect::<Vec<_>>(),
    )
    .expect("the deterministic scale calibration is finite");
    let residual_calibration: Vec<_> = transformed_calibration
        .iter()
        .map(|direction| {
            let primary = table.encode(direction);
            let reconstruction = table.decode(&primary);
            array::from_fn(|coordinate| {
                direction.as_array()[coordinate] - reconstruction[coordinate]
            })
        })
        .collect();
    let codebook = Pq96Codebook::train(&residual_calibration, PQ_TRAINING_SEED)
        .expect("the deterministic residual calibration has 256 finite rows");

    let scorer = FixedPointScorer::new();
    let mut maximum_primary_lookup_entry = 0_i64;
    let mut maximum_residual_lookup_entry = 0_i64;
    for seed in QUERY_SEEDS {
        let query = scorer
            .prepare_query(&plan, &deterministic_raw_vector(seed), &table, &codebook)
            .expect("the deterministic scale query is finite and in range");
        let measurement = query.lookup_scale_measurement();
        maximum_primary_lookup_entry =
            maximum_primary_lookup_entry.max(measurement.maximum_primary_lookup_entry());
        maximum_residual_lookup_entry =
            maximum_residual_lookup_entry.max(measurement.maximum_residual_lookup_entry());
    }

    let metadata = scorer.metadata();
    assert!(maximum_primary_lookup_entry <= metadata.maximum_absolute_table_entry());
    assert!(maximum_residual_lookup_entry <= metadata.maximum_absolute_table_entry());
    assert_eq!(
        metadata
            .maximum_absolute_table_entry()
            .checked_mul(metadata.refined_terms() as i64),
        Some(metadata.worst_case_refined_sum()),
    );

    assert_eq!(
        render_artifact(
            scorer,
            maximum_primary_lookup_entry,
            maximum_residual_lookup_entry,
        ),
        include_str!("data/q24-scale-selection-v1.json"),
    );
}
