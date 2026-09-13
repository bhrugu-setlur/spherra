use spherra::{CreateOptions, Error, IndexBuilder, MAX_TRAINING_ROWS, Vector};

fn rows(n: usize) -> Vec<Vector> {
    (0..n)
        .map(|r| {
            std::array::from_fn(|c| {
                let h = blake3::hash(&((r * 768 + c) as u64).to_le_bytes());
                (i16::from_le_bytes(h.as_bytes()[..2].try_into().unwrap()) as f32) / 32768.0
            })
        })
        .collect()
}
fn options() -> CreateOptions {
    CreateOptions {
        seed: 20260804,
        validation_rows: None,
    }
}
#[test]
fn training_options_precede_filesystem_and_invalid_rows_report_positions() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("index");
    let excessive = vec![[0.0; 768]; MAX_TRAINING_ROWS + 1];
    assert!(matches!(
        IndexBuilder::create(&target, &excessive, options()),
        Err(Error::InvalidTraining)
    ));
    assert!(!target.exists());
    for (n, validation) in [
        (0, None),
        (300, Some(0)),
        (300, Some(45)),
        (300, Some(usize::MAX)),
    ] {
        assert!(matches!(
            IndexBuilder::create(
                &target,
                &vec![[1.0; 768]; n],
                CreateOptions {
                    seed: 1,
                    validation_rows: validation
                }
            ),
            Err(Error::InvalidTraining)
        ));
        assert!(!target.exists());
    }
    for bad in [f32::NAN, f32::INFINITY, 65504.0, 0.0, 1e-20] {
        let mut training = rows(344);
        training[17] = [bad; 768];
        assert!(matches!(
            IndexBuilder::create(&target, &training, options()),
            Err(Error::InvalidVector { position: 17 })
        ));
    }
}
#[test]
fn empty_commit_publishes_nothing_and_push_validation_is_recoverable() {
    let dir = tempfile::tempdir().unwrap();
    let training = rows(344);
    let builder = IndexBuilder::create(dir.path(), &training, options()).unwrap();
    assert!(matches!(builder.commit(), Err(Error::EmptyCommit)));
    assert!(!dir.path().join("CURRENT").exists());
    let mut builder = IndexBuilder::create(dir.path(), &training, options()).unwrap();
    assert_eq!(builder.push(&training[0]).unwrap().get(), 0);
    for bad in [f32::NAN, 65504.0, 0.0] {
        assert!(matches!(
            builder.push(&[bad; 768]),
            Err(Error::InvalidVector { position: 1 })
        ));
    }
    assert_eq!(builder.push(&training[1]).unwrap().get(), 1);
    let report = builder.commit().unwrap();
    assert_eq!(report.generation(), 1);
    assert_eq!(report.first_row().get(), 0);
    assert_eq!(report.rows_added(), 2);
    assert!(report.cleanup_complete());
    assert!(report.drift().insufficient_sample());
    assert!(!report.drift().warned());
    let mut append = IndexBuilder::append(dir.path()).unwrap();
    assert_eq!(append.push(&training[2]).unwrap().get(), 2);
    let report = append.commit().unwrap();
    assert_eq!(report.generation(), 2);
    assert_eq!(report.first_row().get(), 2);
}
