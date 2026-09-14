use crate::{CreateOptions, Index, IndexBuilder};
use spherra_codec::{FixedPointScorer, KernelPath, score_tile_primary};

fn qualify(corpus: spherra_testkit::CorpusSplits) {
    let dir = tempfile::tempdir().unwrap();
    let mut builder = IndexBuilder::create(
        dir.path(),
        corpus.calibration(),
        CreateOptions {
            seed: 20260804,
            validation_rows: None,
        },
    )
    .unwrap();
    for row in corpus.indexed() {
        builder.push(row).unwrap();
    }
    builder.commit().unwrap();
    let index = Index::open(dir.path()).unwrap();
    let model = &index.data.model;
    let scorer = FixedPointScorer::new();
    let mut compared = 0_u64;
    for raw in corpus.queries().iter().take(20) {
        let query = scorer
            .prepare_query(&model.plan, raw, &model.quantizer, &model.codebook)
            .unwrap();
        for segment in &index.data.segments {
            for (ordinal, tile) in segment
                .tiles
                .chunks_exact(crate::open::TILE_BYTES)
                .enumerate()
            {
                let lanes = (segment.entry.row_count as usize - ordinal * 32).min(32);
                let mut out = [0; 32];
                assert_eq!(
                    score_tile_primary(&query, tile, lanes, &mut out).unwrap(),
                    KernelPath::SafeTile
                );
                for (lane, actual) in out[..lanes].iter().enumerate() {
                    let code = crate::search::decode_lane(tile, lane);
                    assert_eq!(*actual, scorer.score_primary(&query, &code).raw());
                    compared += 1;
                }
            }
        }
    }
    assert_eq!(compared, corpus.indexed().len() as u64 * 20);
    eprintln!(
        "{}: {compared} primary scores, zero differences",
        corpus.name()
    );
}

#[test]
#[ignore = "Task 11 release qualification: every row on archived SciFact and generated 100k over 20 queries"]
fn every_primary_score_matches_on_full_corpora() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut archive = spherra_testkit::CorpusDescriptor::resolve(
        root.join("corpora/archive/scifact-mpnet-768-2026-09-13.json")
            .to_str()
            .unwrap(),
    )
    .unwrap();
    if let spherra_testkit::CorpusDescriptor::FileBacked(d) = &mut archive {
        d.path = root.join(&d.path);
    }
    qualify(archive.load(20260804, 200).unwrap());
    qualify(
        spherra_testkit::CorpusDescriptor::resolve("generated-correlated-768x100000")
            .unwrap()
            .load(20260804, 20)
            .unwrap(),
    );
}
