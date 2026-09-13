use spherra::{CreateOptions, Error, Index, IndexBuilder, SearchOptions};
#[test]
fn search_options_ties_and_segments_follow_the_public_contract() {
    let dir = tempfile::tempdir().unwrap();
    let row = [1.0; 768];
    let mut builder = IndexBuilder::create(
        dir.path(),
        &vec![row; 344],
        CreateOptions {
            seed: 7,
            validation_rows: None,
        },
    )
    .unwrap();
    for _ in 0..17 {
        builder.push(&row).unwrap();
    }
    builder.commit().unwrap();
    let mut builder = IndexBuilder::append(dir.path()).unwrap();
    for _ in 0..16 {
        builder.push(&row).unwrap();
    }
    builder.commit().unwrap();
    let index = Index::open(dir.path()).unwrap();
    assert_eq!(
        (index.len(), index.generation(), index.segment_count()),
        (33, 2, 2)
    );
    for options in [
        SearchOptions {
            k: 0,
            candidate_budget: None,
        },
        SearchOptions {
            k: usize::MAX,
            candidate_budget: None,
        },
        SearchOptions {
            k: 2,
            candidate_budget: Some(1),
        },
    ] {
        assert!(matches!(
            index.search(&row, options),
            Err(Error::InvalidOptions)
        ));
    }
    assert!(matches!(
        index.search(
            &[0.0; 768],
            SearchOptions {
                k: 1,
                candidate_budget: None
            }
        ),
        Err(Error::InvalidVector { position: 0 })
    ));
    assert!(matches!(
        index.search(
            &[0.0; 768],
            SearchOptions {
                k: 0,
                candidate_budget: None
            }
        ),
        Err(Error::InvalidOptions)
    ));
    let explicit = index
        .search(
            &row,
            SearchOptions {
                k: usize::MAX,
                candidate_budget: Some(usize::MAX),
            },
        )
        .unwrap();
    assert_eq!(explicit.hits().len(), 33);
    let result = index
        .search(
            &row,
            SearchOptions {
                k: 40,
                candidate_budget: None,
            },
        )
        .unwrap();
    assert_eq!(
        (
            result.rows_scanned(),
            result.rows_refined(),
            result.candidate_budget()
        ),
        (33, 33, 33)
    );
    assert_eq!(result.hits().len(), 33);
    assert_eq!(result.generation(), 2);
    for (r, hit) in result.hits().iter().enumerate() {
        assert_eq!(hit.row().get(), r as u64);
        assert_eq!(hit.segment(), u32::from(r >= 17));
        assert!(hit.interval().0 <= 1.0 && hit.interval().1 >= 1.0);
    }
    assert_eq!(index.certificates(1).unwrap().first_row().get(), 17);
    assert!(index.certificates(2).is_none());
    let expected: Vec<_> = result
        .hits()
        .iter()
        .map(|h| (h.row(), h.score(), h.interval()))
        .collect();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let index = &index;
            let expected = &expected;
            scope.spawn(move || {
                let result = index
                    .search(
                        &row,
                        SearchOptions {
                            k: 40,
                            candidate_budget: None,
                        },
                    )
                    .unwrap();
                assert_eq!(
                    &result
                        .hits()
                        .iter()
                        .map(|h| (h.row(), h.score(), h.interval()))
                        .collect::<Vec<_>>(),
                    expected
                );
            });
        }
    });
}
