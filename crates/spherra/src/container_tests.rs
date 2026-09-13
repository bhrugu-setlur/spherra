use crate::container::{ContainerError, Current, HEADER_LEN, pack};
use crate::manifest::{Manifest, ManifestError, SegmentEntry};
use crate::model::{DriftBaseline, ModelFile};
use spherra_codec::{CODEC_ID, GENERATOR_VERSION, Pq96Codebook, QuantizerTable, TransformPlan};

fn model() -> ModelFile {
    let plan = TransformPlan::from_seed(20260804);
    let centers = vec![0.0; 768 * 16];
    let centroids = vec![0.0; 96 * 256 * 8];
    ModelFile {
        generator_version: GENERATOR_VERSION,
        seed: 20260804,
        expanded_digest: plan.expanded_digest(),
        transform_id: *plan.identity(),
        quantizer_id: *QuantizerTable::from_centers(&centers).unwrap().identity(),
        codebook_id: *Pq96Codebook::from_centroids(&centroids)
            .unwrap()
            .codebook_id(),
        codec_id: *CODEC_ID,
        scorer_version: 1,
        layout: 1,
        centers,
        centroids,
        baseline: DriftBaseline {
            primary: [0.1, 0.2, 0.3],
            refined: [0.05, 0.1, 0.2],
            outside_fraction: 0.01,
        },
    }
}
fn manifest() -> Manifest {
    Manifest {
        index_id: [8; 16],
        generation: 1,
        previous: [0; 32],
        model_hash: *blake3::hash(&model().encode().unwrap()).as_bytes(),
        total_rows: 3,
        segments: vec![SegmentEntry {
            id: [9; 16],
            first_row: 0,
            row_count: 3,
            primary_len: 100,
            residual_len: 200,
            primary_hash: [11; 32],
            residual_hash: [12; 32],
        }],
    }
}
#[test]
fn container_roundtrips_match_independent_golden_bytes() {
    let m = model();
    let bytes = m.encode().unwrap();
    assert_eq!(bytes, include_bytes!("../tests/fixtures/model-v1.bin"));
    assert_eq!(ModelFile::decode(&bytes).unwrap(), m);
    let manifest = manifest();
    let bytes = manifest.encode().unwrap();
    assert_eq!(bytes, include_bytes!("../tests/fixtures/manifest-v1.bin"));
    assert_eq!(Manifest::decode(&bytes).unwrap(), manifest);
    let current = Current {
        generation: 1,
        manifest_hash: *blake3::hash(&bytes).as_bytes(),
    };
    let bytes = current.encode();
    assert_eq!(bytes, include_bytes!("../tests/fixtures/current-v1.bin"));
    assert_eq!(Current::decode(&bytes).unwrap(), current);
}
fn decode(kind: usize, bytes: &[u8]) -> Result<(), ContainerError> {
    match kind {
        0 => Current::decode(bytes).map(|_| ()),
        1 => ModelFile::decode(bytes).map(|_| ()),
        _ => Manifest::decode(bytes).map(|_| ()),
    }
}
#[test]
fn malformed_containers_are_rejected() {
    let fixtures: [&[u8]; 3] = [
        include_bytes!("../tests/fixtures/current-v1.bin"),
        include_bytes!("../tests/fixtures/model-v1.bin"),
        include_bytes!("../tests/fixtures/manifest-v1.bin"),
    ];
    for (kind, original) in fixtures.iter().enumerate() {
        for len in [0, 7, HEADER_LEN - 1, HEADER_LEN, original.len() - 1] {
            assert!(decode(kind, &original[..len]).is_err());
        }
        let mut bytes = original.to_vec();
        bytes[0] ^= 1;
        assert_eq!(decode(kind, &bytes), Err(ContainerError::Magic));
        let mut bytes = original.to_vec();
        bytes[8..10].copy_from_slice(&2_u16.to_le_bytes());
        assert_eq!(decode(kind, &bytes), Err(ContainerError::Version(2)));
        let mut bytes = original.to_vec();
        bytes[10..18].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(decode(kind, &bytes), Err(ContainerError::Length));
        let mut bytes = original.to_vec();
        *bytes.last_mut().unwrap() ^= 1;
        assert_eq!(decode(kind, &bytes), Err(ContainerError::Hash));
        let mut bytes = original.to_vec();
        bytes.push(0);
        assert_eq!(decode(kind, &bytes), Err(ContainerError::Length));
    }
}
#[test]
fn current_crc_is_checked_even_with_valid_trailing_hash() {
    let mut bytes = include_bytes!("../tests/fixtures/current-v1.bin").to_vec();
    bytes[HEADER_LEN] ^= 1;
    let end = bytes.len() - 32;
    let hash = *blake3::hash(&bytes[..end]).as_bytes();
    bytes[end..].copy_from_slice(&hash);
    assert_eq!(Current::decode(&bytes), Err(ContainerError::Crc));
}
#[test]
fn oversized_manifest_count_is_rejected_before_entries_are_allocated() {
    let bytes = manifest().encode().unwrap();
    let mut payload = bytes[HEADER_LEN..bytes.len() - 32].to_vec();
    payload[96..100].copy_from_slice(&u32::MAX.to_le_bytes());
    let encoded = pack(*b"SPHRMAN1", &payload);
    assert_eq!(
        Manifest::decode(&encoded),
        Err(ContainerError::SegmentCount)
    );
}
#[test]
fn checksummed_manifest_semantic_errors_are_distinct() {
    let base = manifest();
    let check = |m: Manifest, expected| {
        let decoded = Manifest::decode(&m.encode().unwrap()).unwrap();
        assert_eq!(decoded.validate(1), Err(expected));
    };
    let mut m = base.clone();
    m.generation = 2;
    check(m, ManifestError::Generation);
    let mut m = base.clone();
    let mut s = m.segments[0].clone();
    s.first_row = 3;
    m.segments.push(s);
    m.total_rows = 6;
    check(m, ManifestError::DuplicateSegment);
    let mut m = base.clone();
    m.segments[0].first_row = 1;
    check(m, ManifestError::FirstRow);
    for (start, error) in [(4, ManifestError::Gap), (2, ManifestError::Overlap)] {
        let mut m = base.clone();
        let mut s = m.segments[0].clone();
        s.id = [10; 16];
        s.first_row = start;
        m.segments.push(s);
        m.total_rows = 6;
        check(m, error);
    }
    let mut m = base.clone();
    m.total_rows = 4;
    check(m, ManifestError::RowSum);
    let mut m = base.clone();
    m.segments.clear();
    check(m, ManifestError::SegmentCount);
    // Encoder bounds also forbid emitting more entries than the format permits.
    let mut m = base.clone();
    m.segments.resize(4097, base.segments[0].clone());
    assert_eq!(m.validate(1), Err(ManifestError::SegmentCount));
    assert_eq!(m.encode(), Err(ContainerError::SegmentCount));
    let encoded = base.encode().unwrap();
    let mut payload = encoded[HEADER_LEN..HEADER_LEN + 100].to_vec();
    payload[96..100].copy_from_slice(&4097_u32.to_le_bytes());
    for _ in 0..4097 {
        payload.extend_from_slice(&encoded[HEADER_LEN + 100..encoded.len() - 32]);
    }
    assert_eq!(
        Manifest::decode(&pack(*b"SPHRMAN1", &payload)),
        Err(ContainerError::SegmentCount)
    );
    for count in [0, 65537] {
        let mut m = base.clone();
        m.segments[0].row_count = count;
        check(m, ManifestError::RowCount);
    }
    let mut m = base.clone();
    m.total_rows = 1_u64 << 48;
    check(m, ManifestError::TotalRows);
    let mut m = base;
    m.segments[0].first_row = u64::MAX;
    check(m, ManifestError::RowOverflow);
}

#[test]
fn model_rejects_noncanonical_or_invalid_drift_baselines_after_rehashing() {
    let original = include_bytes!("../tests/fixtures/model-v1.bin");
    for field in 0..7 {
        for bad in [-0.0_f64, -1.0, f64::INFINITY, f64::NAN] {
            let mut payload = original[HEADER_LEN..original.len() - 32].to_vec();
            let start = payload.len() - 56 + field * 8;
            payload[start..start + 8].copy_from_slice(&bad.to_le_bytes());
            assert!(matches!(
                ModelFile::decode(&pack(*b"SPHRMOD1", &payload)),
                Err(ContainerError::ModelValues)
            ));
        }
    }
    let mut invalid = model();
    invalid.baseline.primary = [0.3, 0.2, 0.1];
    assert_eq!(invalid.encode(), Err(ContainerError::ModelValues));
    let mut invalid = model();
    invalid.baseline.outside_fraction = 1.01;
    assert_eq!(invalid.encode(), Err(ContainerError::ModelValues));
}
