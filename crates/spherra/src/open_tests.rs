use crate::{container::Current, fs::RealFs, manifest::Manifest, model::ModelFile, storage, *};
use spherra_format::{
    PairedSegmentReaders, PrimaryFileReader, PrimarySegment, ResidualFileReader, ResidualSegment,
    encode_primary_segment, encode_residual_segment,
};
use std::{path::Path, sync::OnceLock};
struct Fixture {
    dir: tempfile::TempDir,
    rows: Vec<Vector>,
}
fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let corpus = spherra_testkit::CorpusDescriptor::resolve("generated-correlated-768x400")
            .unwrap()
            .load(20260804, 20)
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let rows = corpus.indexed()[..33].to_vec();
        let mut b = IndexBuilder::create(
            dir.path(),
            corpus.calibration(),
            CreateOptions {
                seed: 20260804,
                validation_rows: None,
            },
        )
        .unwrap();
        for row in &rows[..17] {
            b.push(row).unwrap();
        }
        b.commit().unwrap();
        let mut b = IndexBuilder::append(dir.path()).unwrap();
        for row in &rows[17..] {
            b.push(row).unwrap();
        }
        b.commit().unwrap();
        Fixture { dir, rows }
    })
}
fn copy() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(fixture().dir.path()).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, dir.path().join(path.file_name().unwrap())).unwrap();
    }
    dir
}
fn publish_manifest(dir: &Path, m: &Manifest) {
    let bytes = m.encode().unwrap();
    let hash = *blake3::hash(&bytes).as_bytes();
    std::fs::write(dir.join(storage::named("manifest", &hash)), bytes).unwrap();
    std::fs::write(
        dir.join("CURRENT"),
        Current {
            generation: m.generation,
            manifest_hash: hash,
        }
        .encode(),
    )
    .unwrap();
}
fn publish_model(dir: &Path, m: &mut Manifest, model: &ModelFile) {
    let bytes = model.encode().unwrap();
    m.model_hash = *blake3::hash(&bytes).as_bytes();
    std::fs::write(dir.join(storage::named("model", &m.model_hash)), bytes).unwrap();
    publish_manifest(dir, m);
}
fn rewrite_segment(
    dir: &Path,
    m: &mut Manifest,
    edit: impl FnOnce(&mut PrimarySegment, &mut ResidualSegment),
) {
    let (_, _, model) = storage::load(&RealFs, dir).unwrap();
    let entry = &mut m.segments[0];
    let pp = dir.join(format!("{}.primary", storage::hex(&entry.id)));
    let rp = dir.join(format!("{}.residual", storage::hex(&entry.id)));
    let p = PrimaryFileReader::open_path(&pp, &model.expectations()).unwrap();
    let r = ResidualFileReader::open_path(&rp, &model.expectations()).unwrap();
    let paired = PairedSegmentReaders::open(p, r).unwrap();
    let p = paired.primary();
    let mut primary = PrimarySegment {
        identity: *p.identity(),
        rows: (0..p.row_count()).map(|i| p.row(i).unwrap()).collect(),
        radius_flags: (0..p.row_count())
            .map(|i| p.radius_flags(i).unwrap())
            .collect(),
        primary_codes: (0..p.row_count())
            .map(|i| p.primary_code(i).unwrap())
            .collect(),
        quantizer_table: model.file.centers.clone(),
        primary_certificate: p.primary_certificate().unwrap(),
        refined_certificate: p.refined_certificate().unwrap(),
    };
    let mut residual = ResidualSegment {
        identity: *p.identity(),
        row_count: p.row_count(),
        residual_codes: (0..p.row_count())
            .map(|i| paired.residual_code(i).unwrap())
            .collect(),
        pq_codebook: model.file.centroids.clone(),
    };
    drop(paired);
    edit(&mut primary, &mut residual);
    let p = encode_primary_segment(&primary).unwrap();
    let r = encode_residual_segment(&residual).unwrap();
    entry.primary_len = p.len() as u64;
    entry.residual_len = r.len() as u64;
    entry.primary_hash = p[208..240].try_into().unwrap();
    entry.residual_hash = r[208..240].try_into().unwrap();
    std::fs::write(pp, p).unwrap();
    std::fs::write(rp, r).unwrap();
    publish_manifest(dir, m);
}
#[test]
fn missing_current_and_manifest_or_model_hash_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(Index::open(dir.path()), Err(Error::NotFound)));
    for kind in ["manifest", "model"] {
        let dir = copy();
        let (c, m, _) = storage::load(&RealFs, dir.path()).unwrap();
        let hash = if kind == "manifest" {
            c.manifest_hash
        } else {
            m.model_hash
        };
        let path = dir.path().join(storage::named(kind, &hash));
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[20] ^= 1;
        std::fs::write(path, bytes).unwrap();
        assert!(
            matches!(Index::open(dir.path()), Err(Error::IdentityMismatch)),
            "{kind}"
        );
    }
}
#[test]
fn model_support_rules_and_expanded_plan_are_independent_checks() {
    for case in 0..8 {
        let dir = copy();
        let (_, mut m, model) = storage::load(&RealFs, dir.path()).unwrap();
        m.segments.truncate(1);
        m.total_rows = u64::from(m.segments[0].row_count);
        publish_manifest(dir.path(), &m);
        let mut file = model.file;
        match case {
            0 => file.generator_version += 1,
            1 => file.codec_id[0] ^= 1,
            2 => file.scorer_version += 1,
            3 => file.layout += 1,
            4 => file.expanded_digest[0] ^= 1,
            5 => file.transform_id[0] ^= 1,
            6 => file.quantizer_id[0] ^= 1,
            7 => file.codebook_id[0] ^= 1,
            _ => unreachable!(),
        }
        // For representation support failures, make both segment headers agree
        // with the unsupported model, so agreement alone cannot admit it.
        if (1..=2).contains(&case) {
            let id = file.codec_id;
            let scorer = file.scorer_version;
            rewrite_segment(dir.path(), &mut m, |p, r| {
                p.identity.codec_id = id;
                r.identity.codec_id = id;
                p.identity.scorer_version = scorer;
                r.identity.scorer_version = scorer;
            });
        }
        if case == 3 {
            // Unknown layouts cannot be emitted by the v1 writer. Change only
            // header bytes and repair its defined whole-file identity.
            for primary in [true, false] {
                let entry = &mut m.segments[0];
                let suffix = if primary { "primary" } else { "residual" };
                let path = dir
                    .path()
                    .join(format!("{}.{suffix}", storage::hex(&entry.id)));
                let mut bytes = std::fs::read(&path).unwrap();
                bytes[188..190].copy_from_slice(&(file.layout as u16).to_le_bytes());
                bytes[208..240].fill(0);
                let hash = *blake3::hash(&bytes).as_bytes();
                bytes[208..240].copy_from_slice(&hash);
                if primary {
                    entry.primary_hash = hash
                } else {
                    entry.residual_hash = hash
                }
                std::fs::write(path, bytes).unwrap();
            }
        }
        publish_model(dir.path(), &mut m, &file);
        let e = Index::open(dir.path()).err().expect("reject model");
        assert!(
            if case < 4 {
                matches!(e, Error::Unsupported)
            } else {
                matches!(e, Error::IdentityMismatch)
            },
            "case {case}: {e}"
        );
    }
}
#[test]
fn unsupported_container_versions_are_rejected_even_after_rehashing() {
    fn bump(mut bytes: Vec<u8>) -> Vec<u8> {
        bytes[8..10].copy_from_slice(&2_u16.to_le_bytes());
        let end = bytes.len() - 32;
        let h = *blake3::hash(&bytes[..end]).as_bytes();
        bytes[end..].copy_from_slice(&h);
        bytes
    }
    for kind in ["CURRENT", "manifest", "model"] {
        let dir = copy();
        let (mut c, mut m, model) = storage::load(&RealFs, dir.path()).unwrap();
        match kind {
            "CURRENT" => {
                let mut bytes = bump(c.encode());
                let crc = crc32c::crc32c(&bytes[..58]);
                bytes[58..62].copy_from_slice(&crc.to_le_bytes());
                let h = *blake3::hash(&bytes[..62]).as_bytes();
                bytes[62..].copy_from_slice(&h);
                std::fs::write(dir.path().join("CURRENT"), bytes).unwrap();
            }
            "manifest" => {
                let bytes = bump(m.encode().unwrap());
                c.manifest_hash = *blake3::hash(&bytes).as_bytes();
                std::fs::write(
                    dir.path()
                        .join(storage::named("manifest", &c.manifest_hash)),
                    bytes,
                )
                .unwrap();
                std::fs::write(dir.path().join("CURRENT"), c.encode()).unwrap();
            }
            "model" => {
                let bytes = bump(model.file.encode().unwrap());
                m.model_hash = *blake3::hash(&bytes).as_bytes();
                std::fs::write(
                    dir.path().join(storage::named("model", &m.model_hash)),
                    bytes,
                )
                .unwrap();
                publish_manifest(dir.path(), &m);
            }
            _ => unreachable!(),
        }
        assert!(
            matches!(Index::open(dir.path()), Err(Error::Unsupported)),
            "{kind}"
        );
    }
}
#[test]
fn segment_entry_identity_lengths_hashes_and_certificates_are_checked() {
    for case in 0..14 {
        let dir = copy();
        let (_, mut m, _) = storage::load(&RealFs, dir.path()).unwrap();
        match case {
            0 => {
                m.segments[0].primary_len += 1;
                publish_manifest(dir.path(), &m)
            }
            1 => {
                m.segments[0].residual_hash[0] ^= 1;
                publish_manifest(dir.path(), &m)
            }
            2 => rewrite_segment(dir.path(), &mut m, |p, r| {
                p.identity.collection_id = [7; 16];
                r.identity.collection_id = [7; 16]
            }),
            3 => rewrite_segment(dir.path(), &mut m, |p, r| {
                p.identity.segment_id = [7; 16];
                r.identity.segment_id = [7; 16]
            }),
            4 => {
                m.segments[0].row_count -= 1;
                m.segments[1].first_row -= 1;
                m.total_rows -= 1;
                publish_manifest(dir.path(), &m)
            }
            5 => {
                let path = dir
                    .path()
                    .join(format!("{}.primary", storage::hex(&m.segments[0].id)));
                std::fs::write(path, [0; 10]).unwrap();
                m.segments[0].primary_len = 10;
                publish_manifest(dir.path(), &m);
            }
            6 => rewrite_segment(dir.path(), &mut m, |p, _| {
                p.primary_certificate.epsilon += 0.01
            }),
            7 => rewrite_segment(dir.path(), &mut m, |p, _| {
                p.refined_certificate.query_norm_upper += 0.01
            }),
            8 => rewrite_segment(dir.path(), &mut m, |_, r| {
                r.identity.collection_id = [7; 16]
            }),
            9..=13 => rewrite_segment(dir.path(), &mut m, |p, r| {
                match case {
                    9 => p.identity.codec_id[0] ^= 1,
                    10 => p.identity.scorer_version += 1,
                    11 => p.identity.transform_id[0] ^= 1,
                    12 => p.identity.quantizer_id[0] ^= 1,
                    13 => p.identity.pq_codebook_id[0] ^= 1,
                    _ => unreachable!(),
                }
                r.identity = p.identity;
            }),
            _ => unreachable!(),
        }
        let e = Index::open(dir.path()).err().expect("reject segment");
        assert!(
            if (6..=7).contains(&case) {
                matches!(e, Error::CertificateInvalid)
            } else {
                matches!(e, Error::IdentityMismatch | Error::Corrupt)
            },
            "case {case}: {e}"
        );
    }
}
#[test]
fn descriptor_child() {
    let Ok(path) = std::env::var("SPHERRA_DESCRIPTOR_CHILD") else {
        return;
    };
    let path = Path::new(&path);
    let (_, m, _) = storage::load(&RealFs, path).unwrap();
    let required = m.segments.len() as u64 + 65;
    use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
    let old = getrlimit(Resource::Nofile);
    setrlimit(
        Resource::Nofile,
        Rlimit {
            current: Some(required - 1),
            maximum: old.maximum,
        },
    )
    .unwrap();
    assert!(
        matches!(Index::open(path),Err(Error::DescriptorLimit{required:r,available:a}) if r==required && a==required-1)
    );
    setrlimit(Resource::Nofile, old).unwrap();
    let count = || std::fs::read_dir("/dev/fd").unwrap().count();
    let before = count();
    let index = Index::open(path).unwrap();
    assert_eq!(count() - before, m.segments.len() + 1);
    index
        .search(
            &[1.0; 768],
            SearchOptions {
                k: 10,
                candidate_budget: None,
            },
        )
        .unwrap();
    drop(index);
    assert_eq!(count(), before);
}
#[test]
fn descriptor_budget_and_retained_handles_are_exact() {
    let dir = copy();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "open_tests::descriptor_child", "--nocapture"])
        .env("SPHERRA_DESCRIPTOR_CHILD", dir.path())
        .status()
        .unwrap();
    assert!(status.success());
}
#[test]
fn worker_counts_match_on_distinct_rows() {
    let dir = copy();
    let mut expected = None;
    for workers in [1, 4, 6, 8] {
        let index = Index::open_with_workers(dir.path(), workers).unwrap();
        let results: Vec<_> = fixture()
            .rows
            .iter()
            .take(8)
            .map(|q| {
                let result = index
                    .search(
                        q,
                        SearchOptions {
                            k: 10,
                            candidate_budget: Some(20),
                        },
                    )
                    .unwrap();
                result
                    .hits()
                    .iter()
                    .map(|h| (h.row(), h.score(), h.interval()))
                    .collect::<Vec<_>>()
            })
            .collect();
        std::thread::scope(|scope| {
            for (query, expected) in fixture().rows.iter().zip(&results) {
                let index = &index;
                scope.spawn(move || {
                    let result = index
                        .search(
                            query,
                            SearchOptions {
                                k: 10,
                                candidate_budget: Some(20),
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
        if let Some(e) = &expected {
            assert_eq!(e, &results)
        } else {
            expected = Some(results)
        };
    }
}

#[test]
fn certificates_reject_every_mismatched_binding_and_row_range() {
    let dir = copy();
    let index = Index::open(dir.path()).unwrap();
    let binding = index.data.binding(0);
    let pair = &index.data.segments[0].certificate;
    for case in 0..6 {
        let mut changed = binding;
        match case {
            0 => changed.generation += 1,
            1 => changed.segment += 1,
            2 => changed.id[0] ^= 1,
            3 => changed.first_row += 1,
            4 => changed.row_count += 1,
            5 => changed.model_hash[0] ^= 1,
            _ => unreachable!(),
        }
        assert!(matches!(
            pair.interval(changed, 0, 0, 0),
            Err(Error::CertificateInvalid)
        ));
    }
    assert!(matches!(
        pair.interval(binding, 17, 0, 0),
        Err(Error::CertificateInvalid)
    ));
    assert!(matches!(
        pair.interval(binding, u64::MAX, 0, 0),
        Err(Error::CertificateInvalid)
    ));
}

#[test]
fn stored_magnitudes_roundtrip_append_and_do_not_affect_cosine_order() {
    let corpus = spherra_testkit::CorpusDescriptor::resolve("generated-correlated-768x400")
        .unwrap()
        .load(20260804, 20)
        .unwrap();
    let lengths = [1.0_f32, 1.0001, 14.0, 1e-10, 1e-7, 65504.0];
    let rows: Vec<Vector> = lengths
        .iter()
        .map(|&x| {
            let mut v = [0.0; 768];
            v[0] = x;
            v
        })
        .collect();
    let expected: Vec<_> = rows
        .iter()
        .map(|r| {
            spherra_domain::ValidatedVector::new(r.to_vec())
                .unwrap()
                .radius_f32()
        })
        .collect();
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
    for row in &rows[..3] {
        b.push(row).unwrap();
    }
    b.commit().unwrap();
    let index = Index::open(dir.path()).unwrap();
    let first = index
        .search(
            &rows[0],
            SearchOptions {
                k: 3,
                candidate_budget: Some(3),
            },
        )
        .unwrap();
    let kept = first.hits()[1].clone();
    drop(index);
    let mut b = IndexBuilder::append(dir.path()).unwrap();
    for row in &rows[3..] {
        b.push(row).unwrap();
    }
    b.commit().unwrap();
    for workers in [1, 6] {
        let index = Index::open_with_workers(dir.path(), workers).unwrap();
        let result = index
            .search(
                &rows[0],
                SearchOptions {
                    k: 6,
                    candidate_budget: Some(6),
                },
            )
            .unwrap();
        assert_eq!(result.hits().len(), 6);
        assert_eq!(
            index
                .data
                .segments
                .iter()
                .map(|s| s.magnitudes.len() * 2)
                .sum::<usize>(),
            12
        );
        for (i, h) in result.hits().iter().enumerate() {
            assert_eq!(h.row().get(), i as u64);
            assert_eq!(h.stored_magnitude().to_bits(), expected[i].to_bits());
            assert_eq!(h.raw(), result.hits()[0].raw());
            assert!(h.interval().0 <= 1.0 && h.interval().1 >= 1.0);
        }
        for (a, b) in first.hits().iter().zip(result.hits()) {
            assert_eq!(
                (a.row(), a.raw(), a.interval()),
                (b.row(), b.raw(), b.interval())
            );
        }
    }
    assert_eq!(kept.stored_magnitude(), 1.0);
    assert_eq!(expected[3], 0.0);
}

#[test]
fn opening_rejects_invalid_magnitudes_even_with_consistent_hashes() {
    for bits in [0x8000_u16, 0xbc00, 0x7c00, 0x7e00] {
        let dir = copy();
        let (_, mut m, _) = storage::load(&RealFs, dir.path()).unwrap();
        rewrite_segment(dir.path(), &mut m, |p, _| {
            p.radius_flags[0][..2].copy_from_slice(&bits.to_le_bytes())
        });
        assert!(
            matches!(Index::open(dir.path()), Err(Error::Corrupt)),
            "bits={bits:04x}"
        );
    }
}

#[test]
fn magnitude_only_changes_preserve_every_score_rank_and_interval() {
    let dir = copy();
    let index = Index::open(dir.path()).unwrap();
    let queries = &fixture().rows[..8];
    let before: Vec<_> = queries
        .iter()
        .map(|q| {
            index
                .search(
                    q,
                    SearchOptions {
                        k: 33,
                        candidate_budget: Some(33),
                    },
                )
                .unwrap()
                .hits()
                .iter()
                .map(|h| {
                    let (lo, hi) = h.interval();
                    (h.row(), h.raw(), lo.to_bits(), hi.to_bits())
                })
                .collect::<Vec<_>>()
        })
        .collect();
    drop(index);
    let (_, mut m, _) = storage::load(&RealFs, dir.path()).unwrap();
    rewrite_segment(dir.path(), &mut m, |p, _| {
        for (i, r) in p.radius_flags.iter_mut().enumerate() {
            r[..2]
                .copy_from_slice(&(if i % 2 == 0 { 0x0000_u16 } else { 0x7bff_u16 }).to_le_bytes());
        }
    });
    let index = Index::open(dir.path()).unwrap();
    for (q, expected) in queries.iter().zip(before) {
        let actual = index
            .search(
                q,
                SearchOptions {
                    k: 33,
                    candidate_budget: Some(33),
                },
            )
            .unwrap();
        assert_eq!(
            actual
                .hits()
                .iter()
                .map(|h| {
                    let (lo, hi) = h.interval();
                    (h.row(), h.raw(), lo.to_bits(), hi.to_bits())
                })
                .collect::<Vec<_>>(),
            expected
        );
    }
}
