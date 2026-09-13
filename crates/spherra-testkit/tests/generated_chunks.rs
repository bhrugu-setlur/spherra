use spherra_testkit::{CorpusDescriptor, GeneratedChunks};
#[test]
fn chunk_seeds_define_deterministic_concatenation_and_independent_streams() {
    let source = GeneratedChunks::new(13, 5, 20260804).unwrap();
    let again = GeneratedChunks::new(13, 5, 20260804).unwrap();
    assert_eq!(source.descriptor(), again.descriptor());
    let mut concatenated = Vec::new();
    for chunk in 0..3 {
        let rows = source.chunk(chunk).unwrap();
        assert_eq!(rows, again.chunk(chunk).unwrap());
        let n = if chunk == 2 { 3 } else { 5 };
        let reference = CorpusDescriptor::resolve(&format!("generated-correlated-768x{n}"))
            .unwrap()
            .load(source.descriptor().chunk_seeds[chunk], 1)
            .unwrap();
        assert_eq!(rows, reference.indexed());
        concatenated.extend(rows);
    }
    assert_eq!(concatenated.len(), 13);
    assert!(source.chunk(3).is_err());
    assert_ne!(
        source.descriptor().chunk_seeds[0],
        source.descriptor().chunk_seeds[1]
    );
    assert_eq!(
        source.chunk(0).unwrap(),
        GeneratedChunks::new(5, 5, 20260804)
            .unwrap()
            .chunk(0)
            .unwrap()
    );
    assert_ne!(source.training(5), source.queries(5));
    assert_ne!(source.training(5), source.chunk(0).unwrap());
    assert_eq!(source.queries(5), source.queries(10)[..5]);
    assert!(GeneratedChunks::new(0, 5, 1).is_err());
    assert!(GeneratedChunks::new(1, 0, 1).is_err());
}
