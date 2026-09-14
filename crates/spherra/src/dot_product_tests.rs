use crate::*;
use spherra_codec::{FixedPointScorer, Pq96Code};
use std::sync::OnceLock;

struct Fixture {
    dir: tempfile::TempDir,
    rows: Vec<Vector>,
    queries: Vec<Vector>,
}
fn fixture() -> &'static Fixture {
    static F: OnceLock<Fixture> = OnceLock::new();
    F.get_or_init(|| {
        let corpus = spherra_testkit::CorpusDescriptor::resolve("generated-correlated-768x400")
            .unwrap()
            .load(20260804, 8)
            .unwrap();
        let mut rows = corpus.indexed()[..65].to_vec();
        for (i, r) in rows.iter_mut().enumerate() {
            let factor = 2.0_f32.powi((i % 40) as i32 - 30);
            for x in r {
                *x *= factor;
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let mut b = IndexBuilder::create(
            dir.path(),
            corpus.calibration(),
            CreateOptions {
                seed: 20260804,
                validation_rows: None,
            },
        )
        .unwrap();
        for r in &rows[..33] {
            b.push(r).unwrap();
        }
        b.commit().unwrap();
        let mut b = IndexBuilder::append(dir.path()).unwrap();
        for r in &rows[33..] {
            b.push(r).unwrap();
        }
        b.commit().unwrap();
        Fixture {
            dir,
            rows,
            queries: corpus.queries().to_vec(),
        }
    })
}
fn truth(q: &Vector, x: &Vector) -> f64 {
    q.iter()
        .zip(x)
        .fold(0.0, |s, (&q, &x)| f64::from(q).mul_add(f64::from(x), s))
}
#[test]
fn dot_product_matches_scalar_full_scan_and_encloses_original_dot() {
    let f = fixture();
    for workers in [1, 6] {
        let index = Index::open_with_workers(f.dir.path(), workers).unwrap();
        let model = &index.data.model;
        let scorer = FixedPointScorer::new();
        for q in &f.queries {
            let prepared = scorer
                .prepare_query(&model.plan, q, &model.quantizer, &model.codebook)
                .unwrap();
            let qnorm = q
                .iter()
                .fold(0.0_f64, |s, &x| f64::from(x).mul_add(f64::from(x), s))
                .sqrt();
            let mut primary = Vec::new();
            for s in &index.data.segments {
                for local in 0..s.entry.row_count as usize {
                    let tile = &s.tiles[(local / 32) * crate::open::TILE_BYTES
                        ..(local / 32 + 1) * crate::open::TILE_BYTES];
                    let p = crate::search::decode_lane(tile, local % 32);
                    let r = Pq96Code::from_bytes(s.residual.residual_code(local as u32).unwrap());
                    let magnitude = f64::from(half::f16::from_bits(s.magnitudes[local]).to_f32());
                    let units = (magnitude * 16777216.0) as i128;
                    primary.push((
                        s.entry.first_row + local as u64,
                        i128::from(scorer.score_primary(&prepared, &p).raw()) * units,
                        i128::from(scorer.score_refined(&prepared, &p, &r).raw()) * units,
                    ));
                }
            }
            primary.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            for (k, budget) in [(1, 1), (10, 20), (10, 65), (100, 200)] {
                let mut expected = primary[..budget.min(primary.len())].to_vec();
                expected.sort_unstable_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
                expected.truncate(k);
                let result = index
                    .search_dot_product(
                        q,
                        SearchOptions {
                            k,
                            candidate_budget: Some(budget),
                        },
                    )
                    .unwrap();
                assert_eq!(result.rows_scanned(), 65);
                assert_eq!(result.rows_refined(), budget.min(65) as u64);
                assert_eq!(result.candidate_budget(), budget.min(65));
                assert_eq!(result.generation(), index.generation());
                assert_eq!(result.hits().len(), expected.len());
                for (hit, expected) in result.hits().iter().zip(expected) {
                    assert_eq!(hit.row().get(), expected.0);
                    assert_eq!(hit.score(), (expected.2 as f64 / 281474976710656.0) * qnorm);
                    assert_eq!(
                        hit.stored_magnitude(),
                        spherra_domain::ValidatedVector::new(f.rows[expected.0 as usize].to_vec())
                            .unwrap()
                            .radius_f32()
                    );
                    let actual = truth(q, &f.rows[expected.0 as usize]);
                    assert!(
                        hit.interval().0 <= actual && actual <= hit.interval().1,
                        "truth {actual}, interval {:?}",
                        hit.interval()
                    );
                }
            }
        }
    }
}
#[test]
fn dot_product_weights_before_candidate_admission_and_preserves_cosine() {
    let corpus = spherra_testkit::CorpusDescriptor::resolve("generated-correlated-768x400")
        .unwrap()
        .load(20260804, 4)
        .unwrap();
    let mut short = [0.0; 768];
    short[0] = 1.0;
    let mut long = [0.0; 768];
    long[0] = 10.0;
    long[1] = 10.0;
    let dir = tempfile::tempdir().unwrap();
    let mut b = IndexBuilder::create(
        dir.path(),
        corpus.calibration(),
        CreateOptions {
            seed: 20260804,
            validation_rows: None,
        },
    )
    .unwrap();
    b.push(&short).unwrap();
    b.push(&long).unwrap();
    b.commit().unwrap();
    let index = Index::open(dir.path()).unwrap();
    let cosine = index
        .search(
            &short,
            SearchOptions {
                k: 1,
                candidate_budget: Some(1),
            },
        )
        .unwrap();
    assert_eq!(cosine.hits()[0].row().get(), 0);
    let dot = index
        .search_dot_product(
            &short,
            SearchOptions {
                k: 1,
                candidate_budget: Some(1),
            },
        )
        .unwrap();
    assert_eq!(dot.hits()[0].row().get(), 1);
    assert!(dot.hits()[0].interval().0 <= 10.0 && dot.hits()[0].interval().1 >= 10.0);
    let again = index
        .search(
            &short,
            SearchOptions {
                k: 1,
                candidate_budget: Some(1),
            },
        )
        .unwrap();
    assert_eq!(cosine.hits()[0].raw(), again.hits()[0].raw());
    assert_eq!(cosine.hits()[0].interval(), again.hits()[0].interval());
    let mut negative = short;
    negative[0] = -2.0;
    let dot = index
        .search_dot_product(
            &negative,
            SearchOptions {
                k: 1,
                candidate_budget: Some(2),
            },
        )
        .unwrap();
    assert_eq!(dot.hits()[0].row().get(), 0);
    assert!(dot.hits()[0].interval().0 <= -2.0 && dot.hits()[0].interval().1 >= -2.0);
    let mut doubled = short;
    doubled[0] = 2.0;
    let doubled = index
        .search_dot_product(
            &doubled,
            SearchOptions {
                k: 1,
                candidate_budget: Some(1),
            },
        )
        .unwrap();
    let kept = doubled.hits()[0].clone();
    assert_eq!(kept.score(), dot_score(&index, &short) * 2.0);
    drop(index);
    assert!(kept.stored_magnitude() > 14.0);
}
fn dot_score(index: &Index, q: &Vector) -> f64 {
    index
        .search_dot_product(
            q,
            SearchOptions {
                k: 1,
                candidate_budget: Some(1),
            },
        )
        .unwrap()
        .hits()[0]
        .score()
}
#[test]
fn dot_product_rejects_invalid_options_and_queries_and_can_run_with_cosine() {
    let f = fixture();
    let index = Index::open(f.dir.path()).unwrap();
    for options in [
        SearchOptions {
            k: 0,
            candidate_budget: None,
        },
        SearchOptions {
            k: 2,
            candidate_budget: Some(1),
        },
        SearchOptions {
            k: usize::MAX,
            candidate_budget: None,
        },
    ] {
        assert!(matches!(
            index.search_dot_product(&f.queries[0], options),
            Err(Error::InvalidOptions)
        ));
    }
    for value in [0.0, f32::NAN, f32::INFINITY, 65504.0, 1e-20] {
        assert!(matches!(
            index.search_dot_product(
                &[value; 768],
                SearchOptions {
                    k: 1,
                    candidate_budget: None
                }
            ),
            Err(Error::InvalidVector { .. })
        ));
    }
    std::thread::scope(|scope| {
        for q in &f.queries {
            let index = &index;
            scope.spawn(move || {
                let cosine = index
                    .search(
                        q,
                        SearchOptions {
                            k: 10,
                            candidate_budget: None,
                        },
                    )
                    .unwrap();
                let dot = index
                    .search_dot_product(
                        q,
                        SearchOptions {
                            k: 10,
                            candidate_budget: None,
                        },
                    )
                    .unwrap();
                assert_eq!(dot.hits().len(), cosine.hits().len());
                let again = index
                    .search(
                        q,
                        SearchOptions {
                            k: 10,
                            candidate_budget: None,
                        },
                    )
                    .unwrap();
                assert_eq!(
                    cosine
                        .hits()
                        .iter()
                        .map(|h| (h.row(), h.raw(), h.interval()))
                        .collect::<Vec<_>>(),
                    again
                        .hits()
                        .iter()
                        .map(|h| (h.row(), h.raw(), h.interval()))
                        .collect::<Vec<_>>()
                );
            });
        }
    });
}

#[test]
fn unit_stored_lengths_keep_cosine_order_at_every_budget() {
    let corpus = spherra_testkit::CorpusDescriptor::resolve("generated-correlated-768x400")
        .unwrap()
        .load(20260804, 4)
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut b = IndexBuilder::create(
        dir.path(),
        corpus.calibration(),
        CreateOptions {
            seed: 20260804,
            validation_rows: None,
        },
    )
    .unwrap();
    for r in &corpus.indexed()[..33] {
        let norm = truth(r, r).sqrt();
        let unit = std::array::from_fn(|i| (f64::from(r[i]) / norm) as f32);
        b.push(&unit).unwrap();
    }
    b.commit().unwrap();
    let index = Index::open(dir.path()).unwrap();
    for q in corpus.queries() {
        for budget in [1, 10, 20, 33, 200] {
            let k = budget.min(10);
            let a = index
                .search(
                    q,
                    SearchOptions {
                        k,
                        candidate_budget: Some(budget),
                    },
                )
                .unwrap();
            let b = index
                .search_dot_product(
                    q,
                    SearchOptions {
                        k,
                        candidate_budget: Some(budget),
                    },
                )
                .unwrap();
            assert_eq!(
                a.hits().iter().map(|h| h.row()).collect::<Vec<_>>(),
                b.hits().iter().map(|h| h.row()).collect::<Vec<_>>()
            );
            assert!(b.hits().iter().all(|h| h.stored_magnitude() == 1.0));
        }
    }
}
