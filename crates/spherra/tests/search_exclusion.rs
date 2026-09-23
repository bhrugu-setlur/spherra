use spherra::{CreateOptions, Error, Index, IndexBuilder, RowId, SearchOptions, Vector};

fn options(k: usize, candidate_budget: Option<usize>) -> SearchOptions {
    SearchOptions {
        k,
        candidate_budget,
    }
}

fn index_with_duplicates() -> (tempfile::TempDir, Vector, [RowId; 3]) {
    let directory = tempfile::tempdir().unwrap();
    let query = [1.0; 768];
    let mut builder = IndexBuilder::create(
        directory.path(),
        &vec![query; 344],
        CreateOptions {
            seed: 7,
            validation_rows: None,
        },
    )
    .unwrap();
    let first = builder.push(&query).unwrap();
    let second = builder.push(&query).unwrap();
    builder.commit().unwrap();
    let mut append = IndexBuilder::append(directory.path()).unwrap();
    let third = append.push(&query).unwrap();
    append.commit().unwrap();
    (directory, query, [first, second, third])
}

#[test]
fn cosine_excludes_only_the_supplied_id_before_candidate_admission() {
    let (directory, query, ids) = index_with_duplicates();
    let index = Index::open(directory.path()).unwrap();
    let ordinary = index.search(&query, options(1, Some(1))).unwrap();
    assert_eq!(ordinary.hits()[0].row(), ids[0]);

    let excluded = index
        .search_excluding(&query, ids[0], options(1, Some(1)))
        .unwrap();
    assert_eq!(excluded.hits()[0].row(), ids[1]);
    assert_eq!(excluded.candidate_budget(), 1);
    assert_eq!(excluded.rows_refined(), 1);
    assert_eq!(excluded.rows_scanned(), 3);

    let all_others = index
        .search_excluding(&query, ids[1], options(3, None))
        .unwrap();
    assert_eq!(
        all_others
            .hits()
            .iter()
            .map(|hit| hit.row())
            .collect::<Vec<_>>(),
        vec![ids[0], ids[2]]
    );
    assert_eq!(all_others.candidate_budget(), 2);
}

#[test]
fn dot_product_excludes_only_the_supplied_id_before_candidate_admission() {
    let (directory, query, ids) = index_with_duplicates();
    let index = Index::open(directory.path()).unwrap();
    let excluded = index
        .search_dot_product_excluding(&query, ids[0], options(1, Some(1)))
        .unwrap();
    assert_eq!(excluded.hits()[0].row(), ids[1]);
    assert_eq!(excluded.candidate_budget(), 1);
    assert_eq!(excluded.rows_refined(), 1);
    assert_eq!(excluded.rows_scanned(), 3);
}

#[test]
fn excluding_the_only_vector_returns_no_hits_and_rejects_unknown_ids() {
    let directory = tempfile::tempdir().unwrap();
    let query = [1.0; 768];
    let mut builder = IndexBuilder::create(
        directory.path(),
        &vec![query; 344],
        CreateOptions {
            seed: 7,
            validation_rows: None,
        },
    )
    .unwrap();
    let only = builder.push(&query).unwrap();
    builder.commit().unwrap();
    let index = Index::open(directory.path()).unwrap();
    let cosine = index
        .search_excluding(&query, only, options(1, None))
        .unwrap();
    let dot = index
        .search_dot_product_excluding(&query, only, options(1, None))
        .unwrap();
    assert!(cosine.hits().is_empty());
    assert!(dot.hits().is_empty());
    assert_eq!(cosine.rows_scanned(), 1);
    assert_eq!(dot.rows_scanned(), 1);
    assert_eq!(cosine.rows_refined(), 0);
    assert_eq!(dot.rows_refined(), 0);

    let other_directory = tempfile::tempdir().unwrap();
    let mut builder = IndexBuilder::create(
        other_directory.path(),
        &vec![query; 344],
        CreateOptions {
            seed: 7,
            validation_rows: None,
        },
    )
    .unwrap();
    builder.push(&query).unwrap();
    let unknown = builder.push(&query).unwrap();
    assert!(matches!(
        index.search_excluding(&query, unknown, options(1, None)),
        Err(Error::InvalidOptions)
    ));
    assert!(matches!(
        index.search_dot_product_excluding(&query, unknown, options(1, None)),
        Err(Error::InvalidOptions)
    ));
}
