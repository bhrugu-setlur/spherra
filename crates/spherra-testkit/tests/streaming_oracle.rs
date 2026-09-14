use spherra_testkit::{CorpusDescriptor, ExactOracle, StreamingOracle};
fn compare(corpus: spherra_testkit::CorpusSplits) {
    let expected = ExactOracle::new(corpus.indexed()).unwrap();
    let mut streaming = StreamingOracle::new(corpus.queries(), 100).unwrap();
    for rows in corpus.indexed().chunks(137) {
        streaming.extend(rows).unwrap();
    }
    assert_eq!(streaming.row_count(), corpus.indexed().len() as u64);
    let actual = streaming.top_k();
    for (i, q) in corpus.queries().iter().enumerate() {
        let expected = expected.top_k(q, 100).unwrap();
        assert_eq!(actual[i], expected);
        assert_eq!(
            actual[i]
                .iter()
                .map(|n| n.score.to_bits())
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|n| n.score.to_bits())
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(streaming.corpus_hash(), corpus.hash());
}
#[test]
fn streaming_oracle_matches_every_score_and_tie_on_small_chunks() {
    compare(
        CorpusDescriptor::resolve("generated-correlated-768x400")
            .unwrap()
            .load(20260804, 8)
            .unwrap(),
    );
    let mut oracle = StreamingOracle::new(&[[1.0; 768]], 100).unwrap();
    oracle.extend(&vec![[1.0; 768]; 201]).unwrap();
    assert_eq!(
        oracle.top_k()[0].iter().map(|n| n.row).collect::<Vec<_>>(),
        (0..100).collect::<Vec<_>>()
    );
    let before = oracle.top_k();
    let hash = oracle.corpus_hash();
    let mut invalid = vec![[1.0; 768]; 7];
    invalid[6] = [0.0; 768];
    assert!(oracle.extend(&invalid).is_err());
    assert_eq!(oracle.row_count(), 201);
    assert_eq!(oracle.top_k(), before);
    assert_eq!(oracle.corpus_hash(), hash);
    assert!(StreamingOracle::new(&[], 100).is_err());
    assert!(StreamingOracle::new(&[[1.0; 768]], 0).is_err());
    if let Ok(oversized) = usize::try_from(u64::from(u32::MAX) + 1) {
        assert!(StreamingOracle::new(&[[1.0; 768]], oversized).is_err());
    }
    assert!(StreamingOracle::new(&[[0.0; 768]], 100).is_err());
}
#[test]
#[ignore = "Task 10 release qualification: archived SciFact and generated 20k × 200 queries"]
fn streaming_oracle_matches_archived_and_generated_full_references() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut d = CorpusDescriptor::resolve(
        root.join("corpora/archive/scifact-mpnet-768-2026-09-13.json")
            .to_str()
            .unwrap(),
    )
    .unwrap();
    if let CorpusDescriptor::FileBacked(f) = &mut d {
        f.path = root.join(&f.path)
    }
    compare(d.load(20260804, 200).unwrap());
    compare(
        CorpusDescriptor::resolve("generated-correlated-768x20000")
            .unwrap()
            .load(20260804, 200)
            .unwrap(),
    );
}
