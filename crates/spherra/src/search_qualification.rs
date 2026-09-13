use crate::*;
use spherra_codec::{DirectCode, FixedPointScorer, Pq96Code, dot_f64, normalize_fp64};
use spherra_format::{PairedSegmentReaders, PrimaryFileReader, ResidualFileReader};
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
    let (_, manifest, model) = crate::storage::load(&crate::fs::RealFs, dir.path()).unwrap();
    let mut codes = Vec::new();
    for entry in &manifest.segments {
        let p = PrimaryFileReader::open_path(
            &dir.path()
                .join(format!("{}.primary", crate::storage::hex(&entry.id))),
            &model.expectations(),
        )
        .unwrap();
        let r = ResidualFileReader::open_path(
            &dir.path()
                .join(format!("{}.residual", crate::storage::hex(&entry.id))),
            &model.expectations(),
        )
        .unwrap();
        let paired = PairedSegmentReaders::open(p, r).unwrap();
        for row in 0..entry.row_count {
            let bytes = paired.primary().primary_code(row).unwrap();
            let primary = DirectCode::from_nibbles(std::array::from_fn(|c| {
                (bytes[c / 2] >> ((c % 2) * 4)) & 15
            }))
            .unwrap();
            codes.push((
                primary,
                Pq96Code::from_bytes(paired.residual_code(row).unwrap()),
            ));
        }
    }
    let scorer = FixedPointScorer::new();
    for query in corpus.queries() {
        let prepared = scorer
            .prepare_query(&model.plan, query, &model.quantizer, &model.codebook)
            .unwrap();
        let normalized = normalize_fp64(query).unwrap();
        for (k, budget) in [(10, 20), (10, 200), (100, 200), (10, codes.len())] {
            let mut primary: Vec<_> = codes
                .iter()
                .enumerate()
                .map(|(r, (p, _))| (r, scorer.score_primary(&prepared, p).raw()))
                .collect();
            primary.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            primary.truncate(budget.min(codes.len()));
            let mut refined: Vec<_> = primary
                .iter()
                .map(|(r, _)| {
                    (
                        *r,
                        scorer
                            .score_refined(&prepared, &codes[*r].0, &codes[*r].1)
                            .raw(),
                    )
                })
                .collect();
            refined.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            refined.truncate(k);
            let result = index
                .search(
                    query,
                    SearchOptions {
                        k,
                        candidate_budget: Some(budget),
                    },
                )
                .unwrap();
            assert_eq!(
                result
                    .hits()
                    .iter()
                    .map(|h| (h.row().get() as usize, h.raw()))
                    .collect::<Vec<_>>(),
                refined
            );
            for hit in result.hits() {
                let truth = dot_f64(
                    &normalized,
                    &normalize_fp64(&corpus.indexed()[hit.row().get() as usize]).unwrap(),
                );
                assert!(
                    hit.interval().0 <= truth && truth <= hit.interval().1,
                    "row {} interval {:?} truth {truth}",
                    hit.row().get(),
                    hit.interval()
                );
            }
        }
    }
    eprintln!(
        "{}: {} rows × {} queries, four k/budget combinations; zero integer-score/rank differences and enclosure failures",
        corpus.name(),
        codes.len(),
        corpus.queries().len()
    );
}
#[test]
fn checked_reference_search_smoke() {
    qualify(
        spherra_testkit::CorpusDescriptor::resolve("generated-correlated-768x400")
            .unwrap()
            .load(20260804, 4)
            .unwrap(),
    );
}
#[test]
#[ignore = "Task 8 release qualification uses archived SciFact and full generated 20k"]
fn checked_reference_search_full_corpora() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut descriptor = spherra_testkit::CorpusDescriptor::resolve(
        root.join("corpora/archive/scifact-mpnet-768-2026-09-13.json")
            .to_str()
            .unwrap(),
    )
    .unwrap();
    if let spherra_testkit::CorpusDescriptor::FileBacked(d) = &mut descriptor {
        d.path = root.join(&d.path);
    }
    qualify(descriptor.load(20260804, 200).unwrap());
    qualify(
        spherra_testkit::CorpusDescriptor::resolve("generated-correlated-768x20000")
            .unwrap()
            .load(20260804, 200)
            .unwrap(),
    );
}
