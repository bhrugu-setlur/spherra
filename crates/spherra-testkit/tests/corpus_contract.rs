//! The reproducibility promises a recorded measurement depends on.
//!
//! If a corpus is not a pure function of its descriptor and seed, or if a
//! pinned corpus can be swapped for different bytes, then every number measured
//! against it is unverifiable.

use std::fs;

use spherra_domain::DIMENSION;
use spherra_testkit::corpus::{CorpusDescriptor, FileBackedDescriptor};
use spherra_testkit::exact::{ExactOracle, Neighbor, recall_at, sort_by_score_then_row};

fn generated(name: &str) -> CorpusDescriptor {
    CorpusDescriptor::resolve(name).expect("a well-formed generated corpus name resolves")
}

#[test]
fn a_generated_corpus_is_a_pure_function_of_its_descriptor_and_seed() {
    let descriptor = generated("generated-correlated-768x64");
    let first = descriptor.load(7, 4).expect("the corpus loads");
    let second = descriptor.load(7, 4).expect("the corpus loads again");
    let other_seed = descriptor.load(8, 4).expect("another seed loads");

    assert_eq!(first.indexed(), second.indexed());
    assert_eq!(first.queries(), second.queries());
    assert_eq!(first.calibration(), second.calibration());
    assert_eq!(first.hash(), second.hash());
    assert_ne!(
        first.hash(),
        other_seed.hash(),
        "a different seed must produce a different corpus"
    );
}

#[test]
fn the_two_generators_produce_different_corpora() {
    let gaussian = generated("generated-gaussian-768x64")
        .load(7, 4)
        .expect("the gaussian corpus loads");
    let correlated = generated("generated-correlated-768x64")
        .load(7, 4)
        .expect("the correlated corpus loads");
    assert_ne!(gaussian.hash(), correlated.hash());
}

#[test]
fn training_and_query_rows_are_disjoint_from_the_indexed_rows() {
    let splits = generated("generated-correlated-768x64")
        .load(7, 4)
        .expect("the corpus loads");

    assert_eq!(splits.indexed().len(), 64);
    assert_eq!(splits.queries().len(), 4);
    assert!(!splits.calibration().is_empty());

    for held_out in splits.calibration().iter().chain(splits.queries()) {
        assert!(
            !splits.indexed().contains(held_out),
            "a calibration or query row must not also be an indexed row"
        );
    }
}

#[test]
fn unparsable_corpus_names_are_rejected() {
    for name in [
        "generated-uniform-768x64",
        "generated-correlated-768",
        "generated-correlated-768x0",
        "generated-correlated-768xmany",
    ] {
        assert!(
            CorpusDescriptor::resolve(name).is_err(),
            "{name} must not resolve"
        );
    }
}

#[test]
fn a_generated_corpus_of_the_wrong_dimension_is_rejected() {
    let descriptor = generated("generated-gaussian-512x64");
    assert!(descriptor.load(7, 4).is_err());
}

#[test]
fn a_file_backed_corpus_must_match_its_pinned_length_and_hash() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let vectors_path = directory.path().join("corpus.f32");
    let row_count = 40;

    let mut bytes = Vec::with_capacity(row_count * DIMENSION * 4);
    for row in 0..row_count {
        for coordinate in 0..DIMENSION {
            let value = (row as f32).mul_add(0.001, coordinate as f32 * 0.000_1) + 0.5;
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    fs::write(&vectors_path, &bytes).expect("the corpus file is written");

    let honest = FileBackedDescriptor {
        name: "pinned-test-corpus".to_owned(),
        path: vectors_path.clone(),
        byte_len: bytes.len() as u64,
        blake3: blake3::hash(&bytes).to_hex().to_string(),
        row_count,
        dimension: DIMENSION,
        normalization: "none".to_owned(),
        source_dataset_revision: "test@0000".to_owned(),
        embedding_model_revision: "test@0000".to_owned(),
        license: "test-only".to_owned(),
    };

    let descriptor_path = directory.path().join("corpus.json");
    let write_descriptor = |descriptor: &FileBackedDescriptor| {
        fs::write(
            &descriptor_path,
            serde_json::to_vec(descriptor).expect("the descriptor serializes"),
        )
        .expect("the descriptor is written");
        CorpusDescriptor::resolve(descriptor_path.to_str().expect("a UTF-8 path"))
            .expect("the descriptor parses")
    };

    let splits = write_descriptor(&honest)
        .load(7, 4)
        .expect("the pinned corpus loads");
    assert_eq!(splits.name(), "pinned-test-corpus");
    assert_eq!(splits.queries().len(), 4);
    assert_eq!(
        splits.indexed().len() + splits.calibration().len() + splits.queries().len(),
        row_count
    );

    let mut wrong_hash = honest.clone();
    wrong_hash.blake3 = "0".repeat(64);
    assert!(
        write_descriptor(&wrong_hash).load(7, 4).is_err(),
        "a corpus whose bytes do not match the pinned hash must be rejected"
    );

    let mut wrong_length = honest.clone();
    wrong_length.byte_len += 4;
    assert!(
        write_descriptor(&wrong_length).load(7, 4).is_err(),
        "a corpus whose length does not match the descriptor must be rejected"
    );

    let mut wrong_rows = honest.clone();
    wrong_rows.row_count += 1;
    assert!(
        write_descriptor(&wrong_rows).load(7, 4).is_err(),
        "a declared row count that cannot describe the bytes must be rejected"
    );

    let mut too_few_rows = honest;
    too_few_rows.row_count = row_count;
    assert!(
        write_descriptor(&too_few_rows).load(7, row_count).is_err(),
        "a corpus with no rows left to index must be rejected"
    );
}

#[test]
fn exact_top_k_orders_by_score_then_row() {
    // Two rows are identical, so their scores tie exactly and only the row
    // ordinal can break the tie.
    let mut corpus = vec![[0.0_f32; DIMENSION]; 3];
    corpus[0][0] = 1.0;
    corpus[1][0] = 1.0;
    corpus[2][1] = 1.0;

    let oracle = ExactOracle::new(&corpus).expect("finite non-zero rows");
    let mut query = [0.0_f32; DIMENSION];
    query[0] = 1.0;

    let neighbors = oracle.top_k(&query, 3).expect("the query is finite");
    assert_eq!(neighbors[0].row, 0);
    assert_eq!(neighbors[1].row, 1);
    assert_eq!(neighbors[2].row, 2);
    assert!((neighbors[0].score - 1.0).abs() < 1e-12);
    assert!(neighbors[2].score.abs() < 1e-12);
}

#[test]
fn recall_reports_the_fraction_of_exact_rows_recovered() {
    let exact = [
        Neighbor { row: 5, score: 0.9 },
        Neighbor { row: 2, score: 0.8 },
    ];
    let returned = [Neighbor { row: 2, score: 0.8 }];

    assert!((recall_at(&exact, &returned, 2) - 0.5).abs() < f64::EPSILON);
    assert!((recall_at(&exact, &exact, 2) - 1.0).abs() < f64::EPSILON);
    assert!(recall_at(&exact, &[], 2).abs() < f64::EPSILON);
}

/// Certified scores are converted from `i64` and can never be NaN, but the
/// ordering must still be a total order rather than a comparison that depends
/// on element order. `total_cmp` places a positive NaN above every finite score,
/// so descending order puts it first; ties fall back to the row ordinal.
#[test]
fn sorting_is_total_even_for_a_non_finite_score() {
    let unsorted = [
        Neighbor { row: 2, score: 1.0 },
        Neighbor {
            row: 1,
            score: f64::NAN,
        },
        Neighbor { row: 0, score: 1.0 },
    ];

    let mut neighbors = unsorted.to_vec();
    sort_by_score_then_row(&mut neighbors);
    assert_eq!(
        neighbors.iter().map(|n| n.row).collect::<Vec<_>>(),
        vec![1, 0, 2]
    );

    let mut reversed: Vec<Neighbor> = unsorted.iter().rev().copied().collect();
    sort_by_score_then_row(&mut reversed);
    assert_eq!(
        reversed.iter().map(|n| n.row).collect::<Vec<_>>(),
        vec![1, 0, 2],
        "the ranking must not depend on the order the scores arrived in"
    );
}
