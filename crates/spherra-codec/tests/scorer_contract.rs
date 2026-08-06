use core::array;
use std::sync::OnceLock;

use spherra_codec::{
    CertificateBlockId, CertificateError, CertificateRow, DirectCode, ExhaustiveBlock,
    FixedPointScorer, Pq96Code, Pq96Codebook, PrimaryScore, QuantizerTable, TransformPlan,
    TransformedDirection, build_exhaustive_certificate, dot_f64, normalize_fp64, transform,
};
use spherra_domain::{DIMENSION, ValidatedVector};

fn raw_vector(seed: u64) -> [f32; DIMENSION] {
    let mut state = seed;
    array::from_fn(|coordinate| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let fraction = ((state >> 40) as u32) as f32 / ((1_u32 << 24) as f32);
        fraction.mul_add(2.0, -1.0) + coordinate as f32 * 0.000_001
    })
}

fn basis_vector() -> [f32; DIMENSION] {
    let mut vector = [0.0; DIMENSION];
    vector[319] = 1.0;
    vector
}

fn alternating_vector() -> [f32; DIMENSION] {
    array::from_fn(|coordinate| {
        if coordinate.is_multiple_of(2) {
            1.0
        } else {
            -1.0
        }
    })
}

fn dense_equal_vector() -> [f32; DIMENSION] {
    [0.125; DIMENSION]
}

fn transformed_from_raw(plan: &TransformPlan, raw: &[f32; DIMENSION]) -> TransformedDirection {
    let normalized = normalize_fp64(raw).expect("the test vector is finite and non-zero");
    let kernel_input: Vec<f32> = normalized.iter().map(|value| *value as f32).collect();
    let validated =
        ValidatedVector::new(kernel_input).expect("the f32 kernel input remains a valid vector");
    transform(
        plan,
        validated
            .normalized_direction()
            .expect("a normalized test vector remains direction-reliable"),
    )
}

fn codebook() -> &'static Pq96Codebook {
    static CODEBOOK: OnceLock<Pq96Codebook> = OnceLock::new();
    CODEBOOK.get_or_init(|| {
        let calibration: Vec<[f32; DIMENSION]> = (0..Pq96Code::CENTROIDS)
            .map(|row| {
                array::from_fn(|coordinate| {
                    let subquantizer = coordinate / Pq96Code::SUBVECTOR_DIMENSION;
                    let lane = coordinate % Pq96Code::SUBVECTOR_DIMENSION;
                    row as f32 * 0.000_5 + subquantizer as f32 * 0.000_01 + lane as f32 * 0.000_001
                })
            })
            .collect();
        Pq96Codebook::train(&calibration, 0x5eed_fade)
            .expect("deterministic finite residual calibration")
    })
}

fn dot_transformed(query: &[f32; DIMENSION], vector: &[f32; DIMENSION]) -> f64 {
    let query = array::from_fn(|coordinate| f64::from(query[coordinate]));
    let vector = array::from_fn(|coordinate| f64::from(vector[coordinate]));
    dot_f64(&query, &vector)
}

/// Task 6's canonical scalar meaning of `p + e`: lift each stored FP32 term
/// into the fixed-point/FP64 comparison reduction without an intervening FP32
/// addition or a normalization of the reconstruction.
fn dot_transformed_refined(
    query: &[f32; DIMENSION],
    primary: &[f32; DIMENSION],
    residual: &[f32; DIMENSION],
) -> f64 {
    query
        .iter()
        .enumerate()
        .fold(0.0, |sum, (coordinate, query)| {
            f64::from(*query).mul_add(
                f64::from(primary[coordinate]) + f64::from(residual[coordinate]),
                sum,
            )
        })
}

fn block_id(value: u8) -> CertificateBlockId {
    CertificateBlockId::from_bytes([value; 32])
}

#[test]
fn fixed_point_scores_match_transformed_oracles_and_certificate_encloses_truth() {
    let plan = TransformPlan::from_seed(0x1234_5678_9abc_def0);
    let originals = [raw_vector(7), alternating_vector(), dense_equal_vector()];
    let transformed: Vec<_> = originals
        .iter()
        .map(|original| transformed_from_raw(&plan, original))
        .collect();
    let table = QuantizerTable::train(
        &transformed
            .iter()
            .map(|direction| *direction.as_array())
            .collect::<Vec<_>>(),
    )
    .expect("finite transformed calibration");
    let primary_codes: Vec<_> = transformed
        .iter()
        .map(|direction| table.encode(direction))
        .collect();
    let residual_codes: Vec<_> = transformed
        .iter()
        .zip(&primary_codes)
        .map(|(direction, primary)| {
            let primary = table.decode(primary);
            let residual =
                array::from_fn(|coordinate| direction.as_array()[coordinate] - primary[coordinate]);
            codebook()
                .encode(&residual)
                .expect("finite transformed residual")
        })
        .collect();
    let block = ExhaustiveBlock::from_rows(
        block_id(1),
        originals.len() as u32,
        originals
            .iter()
            .zip(&primary_codes)
            .zip(&residual_codes)
            .enumerate()
            .map(|(row, ((original, primary), residual))| {
                CertificateRow::new(row as u32, original, primary, residual)
            }),
    )
    .expect("all physical block rows are present exactly once");
    let scorer = FixedPointScorer::new();
    let query = raw_vector(11);
    let prepared = scorer
        .prepare_query(&plan, &query, &table, codebook())
        .expect("finite query and score tables");
    let certificate = build_exhaustive_certificate(&scorer, &plan, &table, codebook(), &block)
        .expect("every original row is available for the exhaustive certificate");

    assert_eq!(scorer.metadata().scorer_version(), 1);
    assert_eq!(scorer.metadata().fractional_bits(), 24);
    assert_eq!(scorer.metadata().primary_terms(), DIMENSION);
    assert_eq!(
        scorer.metadata().refined_terms(),
        DIMENSION + Pq96Code::SUBQUANTIZERS
    );
    assert!(
        scorer.metadata().maximum_absolute_table_entry()
            <= i64::MAX / scorer.metadata().refined_terms() as i64
    );

    for (row, ((original, primary), residual)) in originals
        .iter()
        .zip(&primary_codes)
        .zip(&residual_codes)
        .enumerate()
    {
        let primary_reconstruction = table.decode(primary);
        let residual_reconstruction = codebook().decode(residual);
        let oracle_primary = dot_transformed(prepared.transformed(), &primary_reconstruction);
        let oracle_refined = dot_transformed_refined(
            prepared.transformed(),
            &primary_reconstruction,
            &residual_reconstruction,
        );
        let primary_score = scorer.score_primary(&prepared, primary);
        let refined_score = scorer.score_refined(&prepared, primary, residual);
        let prepared_candidate =
            codebook().prepare_candidate(PrimaryScore::for_row(row as u32), *residual);
        let candidate_score = scorer
            .score_prepared_candidate(&prepared, primary, &prepared_candidate)
            .expect("the retained decoded residual matches the query codebook");

        assert!(
            (primary_score.as_f64() - oracle_primary).abs() <= prepared.primary_score_error(),
            "fixed-point primary score differs from its scalar oracle"
        );
        assert!(
            (refined_score.as_f64() - oracle_refined).abs() <= prepared.refined_score_error(),
            "fixed-point refined score differs from its exact split-sum oracle"
        );
        assert_eq!(candidate_score.raw(), refined_score.raw());

        let normalized_query = normalize_fp64(&query).expect("finite non-zero query");
        let normalized_original = normalize_fp64(original).expect("finite non-zero original");
        let true_score = dot_f64(&normalized_query, &normalized_original);
        let candidate = block
            .candidate(row as u32)
            .expect("row was checked in the block");
        let primary_bounds = certificate
            .primary_bounds(
                certificate
                    .score_primary(&scorer, &prepared, &candidate)
                    .expect("matching score provenance"),
            )
            .expect("a primary certified score has compatible provenance");
        let refined_bounds = certificate
            .refined_bounds(
                certificate
                    .score_refined(&scorer, &prepared, &candidate, &prepared_candidate)
                    .expect("matching score provenance"),
            )
            .expect("a refined certified score has compatible provenance");

        assert!(primary_bounds.lower <= true_score && true_score <= primary_bounds.upper);
        assert!(refined_bounds.lower <= true_score && true_score <= refined_bounds.upper);
    }
}

#[test]
fn refined_score_keeps_primary_plus_residual_unnormalized_and_exactly_split() {
    let plan = TransformPlan::from_seed(19);
    let table = QuantizerTable::train(&[[2.0; DIMENSION]]).expect("finite calibration");
    let residual_code = Pq96Code::from_bytes([0; Pq96Code::BYTE_LEN]);
    let primary_code = DirectCode::from_nibbles([0; DIMENSION]).expect("zero is a valid nibble");
    let scorer = FixedPointScorer::new();
    let prepared = scorer
        .prepare_query(&plan, &raw_vector(23), &table, codebook())
        .expect("finite query and score tables");
    let primary = table.decode(&primary_code);
    let residual = codebook().decode(&residual_code);
    let unnormalized = dot_transformed_refined(prepared.transformed(), &primary, &residual);
    let squared_norm = primary.iter().zip(&residual).fold(0.0_f64, |sum, (p, e)| {
        let value = f64::from(*p) + f64::from(*e);
        value.mul_add(value, sum)
    });
    let normalized = unnormalized / squared_norm.sqrt();
    let serving = scorer
        .score_refined(&prepared, &primary_code, &residual_code)
        .as_f64();

    assert!(
        (unnormalized - normalized).abs() > 0.1,
        "the regression fixture must distinguish normalized from unnormalized reconstruction"
    );
    assert!((serving - unnormalized).abs() <= prepared.refined_score_error());
    assert!((serving - normalized).abs() > 0.1);
}

#[test]
fn refined_score_never_materializes_a_rounded_fp32_primary_plus_residual() {
    let plan = TransformPlan::from_seed(29);
    let table = QuantizerTable::train(&[[100_000_000.0; DIMENSION]]).expect("finite calibration");
    let primary_code = DirectCode::from_nibbles([0; DIMENSION]).expect("zero is a valid nibble");
    let scorer = FixedPointScorer::new();
    let prepared = scorer
        .prepare_query(&plan, &raw_vector(97), &table, codebook())
        .expect("finite query and score tables");
    let residual_code = Pq96Code::from_bytes(array::from_fn(|subquantizer| {
        let start = subquantizer * Pq96Code::SUBVECTOR_DIMENSION;
        let low = codebook()
            .centroid(subquantizer, 0)
            .expect("valid low centroid");
        let high = codebook()
            .centroid(subquantizer, u8::MAX)
            .expect("valid high centroid");
        let low_score = (0..Pq96Code::SUBVECTOR_DIMENSION).fold(0.0, |sum, lane| {
            f64::from(prepared.transformed()[start + lane]).mul_add(f64::from(low[lane]), sum)
        });
        let high_score = (0..Pq96Code::SUBVECTOR_DIMENSION).fold(0.0, |sum, lane| {
            f64::from(prepared.transformed()[start + lane]).mul_add(f64::from(high[lane]), sum)
        });
        if high_score > low_score { u8::MAX } else { 0 }
    }));
    let primary = table.decode(&primary_code);
    let residual = codebook().decode(&residual_code);
    let exact_split = dot_transformed_refined(prepared.transformed(), &primary, &residual);
    let rounded_fp32 = array::from_fn(|coordinate| primary[coordinate] + residual[coordinate]);
    let rounded_oracle = dot_transformed(prepared.transformed(), &rounded_fp32);
    let serving = scorer
        .score_refined(&prepared, &primary_code, &residual_code)
        .as_f64();

    assert!(
        (exact_split - rounded_oracle).abs() > 0.01,
        "the large-primary fixture must distinguish exact split semantics from an FP32 addition"
    );
    assert!((serving - exact_split).abs() <= prepared.refined_score_error());
    assert!((serving - rounded_oracle).abs() > prepared.refined_score_error());
}

#[test]
fn exhaustive_certificate_encloses_adversarial_and_generated_pairs() {
    let plan = TransformPlan::from_seed(0x0ddc_0ffe_e15e_beef);
    let originals = [
        basis_vector(),
        alternating_vector(),
        dense_equal_vector(),
        raw_vector(31),
    ];
    let transformed: Vec<_> = originals
        .iter()
        .map(|original| transformed_from_raw(&plan, original))
        .collect();
    let table = QuantizerTable::train(
        &transformed
            .iter()
            .map(|direction| *direction.as_array())
            .collect::<Vec<_>>(),
    )
    .expect("finite transformed calibration");
    let primary_codes: Vec<_> = transformed
        .iter()
        .map(|direction| table.encode(direction))
        .collect();
    let residual_codes: Vec<_> = transformed
        .iter()
        .zip(&primary_codes)
        .map(|(direction, primary)| {
            let primary = table.decode(primary);
            let residual =
                array::from_fn(|coordinate| direction.as_array()[coordinate] - primary[coordinate]);
            codebook()
                .encode(&residual)
                .expect("finite transformed residual")
        })
        .collect();
    let block = ExhaustiveBlock::from_rows(
        block_id(2),
        originals.len() as u32,
        originals
            .iter()
            .zip(&primary_codes)
            .zip(&residual_codes)
            .enumerate()
            .map(|(row, ((original, primary), residual))| {
                CertificateRow::new(row as u32, original, primary, residual)
            }),
    )
    .expect("all physical block rows are present exactly once");
    let scorer = FixedPointScorer::new();
    let certificate = build_exhaustive_certificate(&scorer, &plan, &table, codebook(), &block)
        .expect("the complete block can be certified");

    for query in [
        basis_vector(),
        alternating_vector(),
        dense_equal_vector(),
        raw_vector(37),
    ] {
        let prepared = scorer
            .prepare_query(&plan, &query, &table, codebook())
            .expect("finite query and score tables");
        let normalized_query = normalize_fp64(&query).expect("finite non-zero query");
        for (row, original) in originals.iter().enumerate() {
            let true_score = dot_f64(
                &normalized_query,
                &normalize_fp64(original).expect("finite non-zero original"),
            );
            let candidate = block
                .candidate(row as u32)
                .expect("row was checked in the block");
            let prepared_candidate = codebook()
                .prepare_candidate(PrimaryScore::for_row(row as u32), residual_codes[row]);
            let primary_bounds = certificate
                .primary_bounds(
                    certificate
                        .score_primary(&scorer, &prepared, &candidate)
                        .expect("matching score provenance"),
                )
                .expect("matching primary kind");
            let refined_bounds = certificate
                .refined_bounds(
                    certificate
                        .score_refined(&scorer, &prepared, &candidate, &prepared_candidate)
                        .expect("matching score provenance"),
                )
                .expect("matching refined kind");

            assert!(primary_bounds.lower <= true_score && true_score <= primary_bounds.upper);
            assert!(refined_bounds.lower <= true_score && true_score <= refined_bounds.upper);
        }
    }
}

#[test]
fn certificate_requires_complete_coverage_and_matching_provenance() {
    let original = basis_vector();
    let plan = TransformPlan::from_seed(53);
    let transformed = transformed_from_raw(&plan, &original);
    let table = QuantizerTable::train(&[*transformed.as_array()]).expect("finite calibration");
    let primary = table.encode(&transformed);
    let residual = codebook()
        .encode(&array::from_fn(|coordinate| {
            transformed.as_array()[coordinate] - table.decode(&primary)[coordinate]
        }))
        .expect("finite residual");
    let row = CertificateRow::new(0, &original, &primary, &residual);

    assert!(matches!(
        ExhaustiveBlock::from_rows(block_id(3), 2, [row]),
        Err(CertificateError::RowCountMismatch {
            expected: 2,
            actual: 1
        })
    ));
    assert!(matches!(
        ExhaustiveBlock::from_rows(block_id(3), 2, [row, row]),
        Err(CertificateError::DuplicateRow { row: 0 })
    ));

    let block = ExhaustiveBlock::from_rows(block_id(3), 1, [row])
        .expect("the one physical row has complete coverage");
    let other_block = ExhaustiveBlock::from_rows(block_id(4), 1, [row])
        .expect("the same test row can stand in a distinct block");
    let scorer = FixedPointScorer::new();
    let certificate = build_exhaustive_certificate(&scorer, &plan, &table, codebook(), &block)
        .expect("the complete block can be certified");
    let prepared = scorer
        .prepare_query(&plan, &raw_vector(59), &table, codebook())
        .expect("finite query");
    let wrong_plan = TransformPlan::from_seed(61);
    let wrong_query = scorer
        .prepare_query(&wrong_plan, &raw_vector(59), &table, codebook())
        .expect("finite query with another transform identity");

    let block_candidate = block.candidate(0).expect("present row");
    assert!(matches!(
        certificate.score_primary(&scorer, &wrong_query, &block_candidate),
        Err(CertificateError::ScoreProvenanceMismatch)
    ));
    let other_candidate = other_block.candidate(0).expect("present row");
    assert!(matches!(
        certificate.score_primary(&scorer, &prepared, &other_candidate),
        Err(CertificateError::BlockCapabilityMismatch)
    ));
    let primary_score = certificate
        .score_primary(&scorer, &prepared, &block_candidate)
        .expect("matching certificate inputs");
    assert!(matches!(
        certificate.refined_bounds(primary_score),
        Err(CertificateError::ScoreKindMismatch)
    ));
}

#[test]
fn certificate_rejects_a_same_id_candidate_from_different_content() {
    let plan = TransformPlan::from_seed(0x91);
    let first_original = raw_vector(101);
    let second_original = raw_vector(103);
    let first_transformed = transformed_from_raw(&plan, &first_original);
    let second_transformed = transformed_from_raw(&plan, &second_original);
    let table = QuantizerTable::train(&[
        *first_transformed.as_array(),
        *second_transformed.as_array(),
    ])
    .expect("finite calibration");
    let first_primary = table.encode(&first_transformed);
    let second_primary = table.encode(&second_transformed);
    let first_residual = codebook()
        .encode(&array::from_fn(|coordinate| {
            first_transformed.as_array()[coordinate] - table.decode(&first_primary)[coordinate]
        }))
        .expect("finite first residual");
    let second_residual = codebook()
        .encode(&array::from_fn(|coordinate| {
            second_transformed.as_array()[coordinate] - table.decode(&second_primary)[coordinate]
        }))
        .expect("finite second residual");
    let reused_id = block_id(0xa1);
    let certified_block = ExhaustiveBlock::from_rows(
        reused_id,
        1,
        [CertificateRow::new(
            0,
            &first_original,
            &first_primary,
            &first_residual,
        )],
    )
    .expect("the first one-row block has complete coverage");
    let substituted_block = ExhaustiveBlock::from_rows(
        reused_id,
        1,
        [CertificateRow::new(
            0,
            &second_original,
            &second_primary,
            &second_residual,
        )],
    )
    .expect("the distinct one-row block also has self-consistent coverage");
    let scorer = FixedPointScorer::new();
    let certificate =
        build_exhaustive_certificate(&scorer, &plan, &table, codebook(), &certified_block)
            .expect("the first block can be certified");
    let prepared = scorer
        .prepare_query(&plan, &raw_vector(107), &table, codebook())
        .expect("finite query");

    let substituted_candidate = substituted_block
        .candidate(0)
        .expect("present substituted row");
    assert!(
        certificate
            .score_primary(&scorer, &prepared, &substituted_candidate)
            .is_err(),
        "a caller-controlled block ID must not let another block's row use this certificate"
    );
}

#[test]
fn certificate_rejects_a_sampled_count_substitution() {
    let plan = TransformPlan::from_seed(0x93);
    let first_original = raw_vector(109);
    let second_original = raw_vector(113);
    let first_transformed = transformed_from_raw(&plan, &first_original);
    let second_transformed = transformed_from_raw(&plan, &second_original);
    let table = QuantizerTable::train(&[
        *first_transformed.as_array(),
        *second_transformed.as_array(),
    ])
    .expect("finite calibration");
    let first_primary = table.encode(&first_transformed);
    let second_primary = table.encode(&second_transformed);
    let first_residual = codebook()
        .encode(&array::from_fn(|coordinate| {
            first_transformed.as_array()[coordinate] - table.decode(&first_primary)[coordinate]
        }))
        .expect("finite first residual");
    let second_residual = codebook()
        .encode(&array::from_fn(|coordinate| {
            second_transformed.as_array()[coordinate] - table.decode(&second_primary)[coordinate]
        }))
        .expect("finite second residual");
    let reused_id = block_id(0xa3);
    let complete_block = ExhaustiveBlock::from_rows(
        reused_id,
        2,
        [
            CertificateRow::new(0, &first_original, &first_primary, &first_residual),
            CertificateRow::new(1, &second_original, &second_primary, &second_residual),
        ],
    )
    .expect("the two physical rows have complete coverage");
    let sampled_block = ExhaustiveBlock::from_rows(
        reused_id,
        1,
        [CertificateRow::new(
            0,
            &second_original,
            &second_primary,
            &second_residual,
        )],
    )
    .expect("a malicious one-row self-declared count is internally consistent");
    let scorer = FixedPointScorer::new();
    let certificate =
        build_exhaustive_certificate(&scorer, &plan, &table, codebook(), &complete_block)
            .expect("the complete block can be certified");
    let prepared = scorer
        .prepare_query(&plan, &raw_vector(127), &table, codebook())
        .expect("finite query");

    let sampled_candidate = sampled_block.candidate(0).expect("present sampled row");
    assert!(
        certificate
            .score_primary(&scorer, &prepared, &sampled_candidate)
            .is_err(),
        "a sampled block with a caller-declared count must not borrow a complete block certificate"
    );
}

#[test]
fn prepared_candidate_produces_a_certified_refined_bound_without_reloading_residuals() {
    let plan = TransformPlan::from_seed(0xa7);
    let original = raw_vector(131);
    let transformed = transformed_from_raw(&plan, &original);
    let table = QuantizerTable::train(&[*transformed.as_array()]).expect("finite calibration");
    let primary = table.encode(&transformed);
    let residual = codebook()
        .encode(&array::from_fn(|coordinate| {
            transformed.as_array()[coordinate] - table.decode(&primary)[coordinate]
        }))
        .expect("finite residual");
    let block = ExhaustiveBlock::from_rows(
        block_id(0xa5),
        1,
        [CertificateRow::new(0, &original, &primary, &residual)],
    )
    .expect("the one physical row has complete coverage");
    let scorer = FixedPointScorer::new();
    let certificate = build_exhaustive_certificate(&scorer, &plan, &table, codebook(), &block)
        .expect("the complete block can be certified");
    let query = raw_vector(137);
    let prepared = scorer
        .prepare_query(&plan, &query, &table, codebook())
        .expect("finite query");
    let block_candidate = block.candidate(0).expect("present certificate row");
    let prepared_candidate = codebook().prepare_candidate(PrimaryScore::for_row(0), residual);
    let wrong_row_candidate = codebook().prepare_candidate(PrimaryScore::for_row(1), residual);
    let mut different_residual_bytes = *residual.as_bytes();
    different_residual_bytes[0] ^= 1;
    let wrong_residual_candidate = codebook().prepare_candidate(
        PrimaryScore::for_row(0),
        Pq96Code::from_bytes(different_residual_bytes),
    );

    assert!(matches!(
        certificate.score_refined(&scorer, &prepared, &block_candidate, &wrong_row_candidate,),
        Err(CertificateError::PreparedCandidateRowMismatch {
            expected: 0,
            actual: 1,
        })
    ));
    assert!(matches!(
        certificate.score_refined(
            &scorer,
            &prepared,
            &block_candidate,
            &wrong_residual_candidate,
        ),
        Err(CertificateError::PreparedCandidateResidualMismatch { row: 0 })
    ));
    let certified = certificate
        .score_refined(&scorer, &prepared, &block_candidate, &prepared_candidate)
        .expect("the retained candidate is bound to the certified block row");
    let bounds = certificate
        .refined_bounds(certified)
        .expect("the retained candidate produces a refined certificate score");
    let true_score = dot_f64(
        &normalize_fp64(&query).expect("finite query"),
        &normalize_fp64(&original).expect("finite original"),
    );

    assert!(bounds.lower <= true_score && true_score <= bounds.upper);
}

#[test]
fn certificate_rejects_quantizer_and_codebook_provenance_mutations() {
    let plan = TransformPlan::from_seed(0xab);
    let original = raw_vector(139);
    let transformed = transformed_from_raw(&plan, &original);
    let table = QuantizerTable::train(&[*transformed.as_array()]).expect("finite calibration");
    let primary = table.encode(&transformed);
    let residual = codebook()
        .encode(&array::from_fn(|coordinate| {
            transformed.as_array()[coordinate] - table.decode(&primary)[coordinate]
        }))
        .expect("finite residual");
    let block = ExhaustiveBlock::from_rows(
        block_id(0xa9),
        1,
        [CertificateRow::new(0, &original, &primary, &residual)],
    )
    .expect("the one physical row has complete coverage");
    let scorer = FixedPointScorer::new();
    let certificate = build_exhaustive_certificate(&scorer, &plan, &table, codebook(), &block)
        .expect("the original representation can be certified");
    let query = raw_vector(149);
    let prepared = scorer
        .prepare_query(&plan, &query, &table, codebook())
        .expect("finite query");
    let candidate = block.candidate(0).expect("present certificate row");

    let alternate_table = QuantizerTable::train(&[[-0.75; DIMENSION], [0.5; DIMENSION]])
        .expect("finite alternate quantizer calibration");
    let alternate_quantizer_query = scorer
        .prepare_query(&plan, &query, &alternate_table, codebook())
        .expect("finite alternate-quantizer query");
    assert_ne!(
        prepared.provenance().quantizer_identity(),
        alternate_quantizer_query.provenance().quantizer_identity(),
        "the fixture must exercise an alternate quantizer identity"
    );
    assert!(matches!(
        certificate.score_primary(&scorer, &alternate_quantizer_query, &candidate),
        Err(CertificateError::ScoreProvenanceMismatch)
    ));

    let alternate_calibration: Vec<_> = (0..Pq96Code::CENTROIDS)
        .map(|row| {
            array::from_fn(|coordinate| row as f32 * -0.000_7 + coordinate as f32 * 0.000_000_3)
        })
        .collect();
    let alternate_codebook = Pq96Codebook::train(&alternate_calibration, 0x0a11_ce11)
        .expect("finite alternate codebook calibration");
    let alternate_certificate =
        build_exhaustive_certificate(&scorer, &plan, &table, &alternate_codebook, &block)
            .expect("the same physical rows can be enumerated with another codebook identity");
    assert_ne!(
        certificate.provenance().codebook_identity(),
        alternate_certificate.provenance().codebook_identity(),
        "the fixture must exercise an alternate certificate codebook identity"
    );
    assert!(matches!(
        alternate_certificate.score_primary(&scorer, &prepared, &candidate),
        Err(CertificateError::ScoreProvenanceMismatch)
    ));
}

#[test]
fn prepared_candidate_must_share_the_query_codebook() {
    let plan = TransformPlan::from_seed(67);
    let original = raw_vector(71);
    let transformed = transformed_from_raw(&plan, &original);
    let table = QuantizerTable::train(&[*transformed.as_array()]).expect("finite calibration");
    let primary = table.encode(&transformed);
    let other_calibration: Vec<_> = (0..Pq96Code::CENTROIDS)
        .map(|row| [row as f32 * -0.000_3; DIMENSION])
        .collect();
    let other_codebook =
        Pq96Codebook::train(&other_calibration, 73).expect("finite alternate codebook");
    let scorer = FixedPointScorer::new();
    let prepared = scorer
        .prepare_query(&plan, &raw_vector(79), &table, codebook())
        .expect("finite query");
    let candidate = other_codebook.prepare_candidate(
        PrimaryScore::for_row(0),
        Pq96Code::from_bytes([0; Pq96Code::BYTE_LEN]),
    );

    assert!(matches!(
        scorer.score_prepared_candidate(&prepared, &primary, &candidate),
        Err(spherra_codec::ScorerError::CandidateCodebookMismatch)
    ));
}
