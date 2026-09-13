//! Full-size builder gates; explicit release runs keep routine debug CI bounded.
use crate::{builder::stored, fs::RealFs, model::Model, storage, *};
use spherra_codec::{
    CertificateBlockId, CertificateRow, ExhaustiveBlock, FixedPointScorer,
    build_exhaustive_certificate,
};
use spherra_format::{PairedSegmentReaders, PrimaryFileReader, ResidualFileReader};
fn options() -> CreateOptions {
    CreateOptions {
        seed: 20260804,
        validation_rows: None,
    }
}
fn corpus(n: usize) -> spherra_testkit::CorpusSplits {
    spherra_testkit::CorpusDescriptor::resolve(&format!("generated-correlated-768x{n}"))
        .unwrap()
        .load(20260804, 20)
        .unwrap()
}
fn inspect(dir: &std::path::Path, originals: &[Vector]) -> Vec<([u8; 384], [u8; 96])> {
    let (_, manifest, model) = storage::load(&RealFs, dir).unwrap();
    let mut all = Vec::new();
    for entry in &manifest.segments {
        assert!((1..=65536).contains(&entry.row_count));
        let p = PrimaryFileReader::open_path(
            &dir.join(format!("{}.primary", storage::hex(&entry.id))),
            &model.expectations(),
        )
        .unwrap();
        let r = ResidualFileReader::open_path(
            &dir.join(format!("{}.residual", storage::hex(&entry.id))),
            &model.expectations(),
        )
        .unwrap();
        let paired = PairedSegmentReaders::open(p, r).unwrap();
        let start = entry.first_row as usize;
        let rows = &originals[start..start + entry.row_count as usize];
        let encoded: Vec<_> = rows
            .iter()
            .enumerate()
            .map(|(i, v)| model.encode(v, entry.first_row + i as u64).unwrap())
            .collect();
        let block = ExhaustiveBlock::from_rows(
            CertificateBlockId::from_bytes([3; 32]),
            entry.row_count,
            rows.iter()
                .zip(&encoded)
                .enumerate()
                .map(|(i, (row, e))| CertificateRow::new(i as u32, row, &e.primary, &e.residual)),
        )
        .unwrap();
        let cert = build_exhaustive_certificate(
            &FixedPointScorer::new(),
            &model.plan,
            &model.quantizer,
            &model.codebook,
            &block,
        )
        .unwrap();
        assert_eq!(
            paired.primary().primary_certificate().unwrap(),
            stored(cert.primary().terms())
        );
        assert_eq!(
            paired.primary().refined_certificate().unwrap(),
            stored(cert.refined().terms())
        );
        for (i, e) in encoded.iter().enumerate() {
            let p = paired.primary().primary_code(i as u32).unwrap();
            let r = paired.residual_code(i as u32).unwrap();
            assert_eq!(p, *e.primary.as_bytes());
            assert_eq!(r, *e.residual.as_bytes());
            let row = paired.primary().row(i as u32).unwrap();
            assert_eq!(row.chunk_id.as_u128(), (start + i) as u128);
            assert_eq!(row.document_id.as_u128(), 0);
            let radius = crate::builder::validate(&rows[i], 0)
                .unwrap()
                .radius_f16_bits()
                .to_le_bytes();
            assert_eq!(
                paired.primary().radius_flags(i as u32).unwrap(),
                [radius[0], radius[1], 0, 0]
            );
            all.push((p, r));
        }
    }
    all
}
#[test]
#[ignore = "full Task 7 release qualification: 70k rows, multiple encodings and certificates"]
fn batching_and_segment_certificates() {
    let corpus = corpus(70000);
    let rows = corpus.indexed();
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let mut builder = IndexBuilder::create(a.path(), corpus.calibration(), options()).unwrap();
    for (r, v) in rows.iter().enumerate() {
        assert_eq!(builder.push(v).unwrap().get(), r as u64)
    }
    assert_eq!(builder.commit().unwrap().rows_added(), 70000);
    let mut offset = 0;
    for size in [1, 500, 69499] {
        let mut builder = if offset == 0 {
            IndexBuilder::create(b.path(), corpus.calibration(), options()).unwrap()
        } else {
            IndexBuilder::append(b.path()).unwrap()
        };
        for (r, v) in rows[offset..offset + size].iter().enumerate() {
            assert_eq!(builder.push(v).unwrap().get(), (offset + r) as u64)
        }
        assert_eq!(builder.commit().unwrap().rows_added(), size as u64);
        offset += size;
    }
    assert_eq!(inspect(a.path(), rows), inspect(b.path(), rows));
    eprintln!(
        "70,000 rows: identical codes across 1 versus 3 commits; every segment certificate equals direct construction"
    );
}
#[test]
#[ignore = "full Task 7 release qualification: archived SciFact and 32,768-row training cap"]
fn archived_training_split_and_maximum_training() {
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
    let corpus = descriptor.load(20260804, 200).unwrap();
    assert_eq!(corpus.calibration().len(), 1295);
    assert_eq!((1295 / 4, 1295 - 1295 / 4), (323, 972));
    let dir = tempfile::tempdir().unwrap();
    let builder = IndexBuilder::create(dir.path(), corpus.calibration(), options()).unwrap();
    drop(builder);
    let max = vec![[1.0; 768]; MAX_TRAINING_ROWS];
    let builder = IndexBuilder::create(dir.path(), &max, options()).unwrap();
    drop(builder);
    eprintln!("SciFact 972/323 split and exactly 32,768 training inputs accepted");
}
#[test]
#[ignore = "full Task 7 release qualification: 200k-row commit and distribution drift"]
fn full_reservoir_and_distribution_drift() {
    let corpus = corpus(200000);
    let dir = tempfile::tempdir().unwrap();
    let mut builder = IndexBuilder::create(dir.path(), corpus.calibration(), options()).unwrap();
    for v in corpus.indexed() {
        builder.push(v).unwrap();
    }
    let report = builder.commit().unwrap();
    assert_eq!(report.drift().sample_size(), 65536);
    assert!(!report.drift().insufficient_sample());
    assert!(!report.drift().warned(), "{:?}", report.drift());
    let (_, m, model) = storage::load(&RealFs, dir.path()).unwrap();
    assert_eq!(m.segments.len(), 4);
    // Independently collect the full-commit sample from row statistics in reverse order.
    let mut reservoir = crate::drift::DriftSample::new(m.index_id);
    for r in (0..corpus.indexed().len()).rev() {
        let e = model.encode(&corpus.indexed()[r], r as u64).unwrap();
        reservoir.add(r as u64, e.stats, &model.file.baseline);
    }
    assert_eq!(
        report.drift().stats(),
        reservoir.report(&model.file.baseline).stats()
    );
    // Generate a strongly shifted distribution: directions concentrated on
    // transform axes instead of the diffuse correlated training distribution.
    // Recover the transpose through public basis transforms, without bypassing
    // the codec's validated input contract or changing warning thresholds.
    let columns: Vec<_> = (0..768)
        .map(|c| {
            let mut basis = [0.0; 768];
            basis[c] = 1.0;
            let v = crate::builder::validate(&basis, 0).unwrap();
            *spherra_codec::transform(&model.plan, v.normalized_direction().unwrap()).as_array()
        })
        .collect();
    let mut builder = IndexBuilder::append(dir.path()).unwrap();
    for r in 0..1200 {
        let shifted = std::array::from_fn(|c| columns[c][r % 768]);
        builder.push(&shifted).unwrap();
    }
    let report = builder.commit().unwrap();
    assert!(report.drift().warned(), "{:?}", report.drift());
    eprintln!(
        "200,000 rows / 4 segments: reservoir capped at 65,536 and batch-independent; normal/shifted warnings pass"
    );
}
#[test]
fn training_split_is_deterministic_and_disjoint() {
    let corpus = corpus(400);
    let rows = corpus.calibration();
    let a = Model::train(rows, 17, rows.len() / 4).unwrap();
    let b = Model::train(rows, 17, rows.len() / 4).unwrap();
    assert_eq!(a.file, b.file);
    let mut order: Vec<_> = (0..rows.len()).collect();
    order.sort_by_key(|r| {
        let bytes: Vec<_> = 17_u64
            .to_le_bytes()
            .into_iter()
            .chain((*r as u64).to_le_bytes())
            .collect();
        *blake3::hash(&bytes).as_bytes()
    });
    let mut changed = rows.to_vec();
    for &r in &order[..rows.len() / 4] {
        changed[r] = [1.0; 768];
    }
    let c = Model::train(&changed, 17, rows.len() / 4).unwrap();
    assert_eq!(a.file.centers, c.file.centers);
    assert_eq!(a.file.centroids, c.file.centroids);
    assert_ne!(a.file.baseline, c.file.baseline);
}
