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
    let mut reordered = false;
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
                    let p = model.quantizer.decode(&codes[*r].0);
                    let e = model.codebook.decode(&codes[*r].1);
                    let length = p
                        .iter()
                        .zip(e)
                        .map(|(&p, e)| {
                            let v = f64::from(p) + f64::from(e);
                            v * v
                        })
                        .sum::<f64>()
                        .sqrt();
                    let raw = scorer
                        .score_refined(&prepared, &codes[*r].0, &codes[*r].1)
                        .raw();
                    let corrected = if length.is_normal() {
                        raw as f64 / length
                    } else {
                        raw as f64
                    };
                    (*r, raw, corrected / (1_u64 << 24) as f64)
                })
                .collect();
            let mut legacy = refined.clone();
            legacy.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            refined.sort_unstable_by(|a, b| b.2.total_cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
            reordered |= refined.iter().take(k).zip(&legacy).any(|(a, b)| a.0 != b.0);
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
                    .map(|h| (h.row().get() as usize, h.raw(), h.score()))
                    .collect::<Vec<_>>(),
                refined
            );
            for hit in result.hits() {
                let si = hit.segment() as usize;
                let row = hit.row().get();
                let s = &index.data.segments[si];
                let primary_raw = scorer
                    .score_primary(&prepared, &codes[row as usize].0)
                    .raw();
                // Ranking changed, but the same row's original-space proof is
                // still authenticated by exactly the uncorrected integer scores.
                assert_eq!(
                    hit.interval(),
                    s.certificate
                        .interval(index.data.binding(si), row, primary_raw, hit.raw())
                        .unwrap()
                );
                assert_ne!(hit.score(), hit.raw() as f64 / (1_u64 << 24) as f64);
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
    assert!(reordered, "fixture must exercise a changed finalist order");
    eprintln!(
        "{}: {} rows × {} queries, four k/budget combinations; zero raw-score/corrected-score/rank differences and enclosure failures",
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
