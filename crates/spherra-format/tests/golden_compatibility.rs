//! Pinned v1 byte compatibility for the minimal primary/residual segment pair.
//!
//! These fixtures are the durable contract. A format change must keep reading
//! them byte-for-byte or deliberately raise the major version and record a
//! migration decision. Run the ignored regenerator below only as part of such a
//! decision:
//!
//! ```text
//! cargo test -p spherra-format --test golden_compatibility -- --ignored
//! ```

mod common;

use common::*;
use spherra_format::{
    FileKind, FormatError, PairedSegmentReaders, PrimaryFileReader, ResidualFileReader,
    SectionKind, encode_primary_segment, encode_residual_segment,
};

const PRIMARY_FIXTURE: &[u8] = include_bytes!("fixtures/primary-v1-minimal.bin");
const RESIDUAL_FIXTURE: &[u8] = include_bytes!("fixtures/residual-v1-minimal.bin");

#[test]
fn regenerated_fixtures_match_the_checked_in_bytes() {
    assert_eq!(
        encode_primary_segment(&primary_segment()).expect("encode"),
        PRIMARY_FIXTURE,
        "the primary writer is no longer byte-deterministic against the pinned v1 fixture"
    );
    assert_eq!(
        encode_residual_segment(&residual_segment()).expect("encode"),
        RESIDUAL_FIXTURE,
        "the residual writer is no longer byte-deterministic against the pinned v1 fixture"
    );
}

#[test]
fn the_pinned_fixtures_open_through_their_own_capability_readers() {
    let primary = PrimaryFileReader::open_bytes(PRIMARY_FIXTURE.to_vec(), &expectations())
        .expect("the pinned primary fixture opens");
    let residual = ResidualFileReader::open_bytes(RESIDUAL_FIXTURE.to_vec(), &expectations())
        .expect("the pinned residual fixture opens");

    assert_eq!(primary.file_kind(), FileKind::Primary);
    assert_eq!(residual.file_kind(), FileKind::Residual);
    assert_eq!(primary.row_count(), ROW_COUNT);
    assert_eq!(residual.row_count(), ROW_COUNT);

    for row in 0..ROW_COUNT {
        assert_eq!(primary.row(row).expect("row entry"), row_entry(row));
        assert_eq!(
            primary.primary_code(row).expect("primary code"),
            primary_code(row)
        );
    }
    assert_eq!(primary.quantizer_table().expect("table"), quantizer_table());
    assert_eq!(residual.pq_codebook().expect("codebook"), pq_codebook());
}

#[test]
fn the_pinned_primary_fixture_carries_exactly_one_tiled_soa_32_tail_tile() {
    let entry = entry_at(
        PRIMARY_FIXTURE,
        index_of_kind(PRIMARY_FIXTURE, KIND_PRIMARY_DIRECT_INT4),
    );

    assert_eq!(
        read_u64(PRIMARY_FIXTURE, entry + ENTRY_OFFSET_LENGTH),
        spherra_format::tiled_soa32_len(ROW_COUNT).expect("two rows fit one tile"),
    );
    assert_eq!(
        read_u32(PRIMARY_FIXTURE, entry + ENTRY_OFFSET_LOGICAL_ROW_COUNT),
        ROW_COUNT,
        "the tail tile stores two logical rows inside a full 32-row tile"
    );
}

#[test]
fn swapping_the_pinned_file_kinds_is_rejected() {
    assert!(matches!(
        PrimaryFileReader::open_bytes(RESIDUAL_FIXTURE.to_vec(), &expectations()),
        Err(FormatError::UnexpectedFileKind {
            expected: FileKind::Primary,
            actual: FileKind::Residual,
        })
    ));
    assert!(matches!(
        ResidualFileReader::open_bytes(PRIMARY_FIXTURE.to_vec(), &expectations()),
        Err(FormatError::UnexpectedFileKind {
            expected: FileKind::Residual,
            actual: FileKind::Primary,
        })
    ));
}

#[test]
fn a_residual_section_embedded_in_the_pinned_primary_fixture_is_rejected() {
    let mut bytes = PRIMARY_FIXTURE.to_vec();
    let entry = entry_at(&bytes, index_of_kind(&bytes, KIND_REFINED_CERTIFICATE));
    write_u16(&mut bytes, entry + ENTRY_OFFSET_KIND, KIND_PQ96_RESIDUAL);

    assert!(matches!(
        PrimaryFileReader::open_bytes(bytes, &expectations()),
        Err(FormatError::ForbiddenSection {
            kind: SectionKind::Pq96Residual,
            ..
        })
    ));
}

#[test]
fn a_primary_section_embedded_in_the_pinned_residual_fixture_is_rejected() {
    let mut bytes = RESIDUAL_FIXTURE.to_vec();
    let entry = entry_at(&bytes, index_of_kind(&bytes, KIND_PQ_CODEBOOK));
    write_u16(
        &mut bytes,
        entry + ENTRY_OFFSET_KIND,
        KIND_INT4_QUANTIZER_TABLE,
    );

    assert!(matches!(
        ResidualFileReader::open_bytes(bytes, &expectations()),
        Err(FormatError::ForbiddenSection {
            kind: SectionKind::Int4QuantizerTable,
            ..
        })
    ));
}

#[test]
fn pairing_rejects_a_valid_residual_file_from_another_collection_or_segment() {
    let mut other_collection = residual_segment();
    other_collection.identity.collection_id = [0x99; 16];
    let other_collection = ResidualFileReader::open_bytes(
        encode_residual_segment(&other_collection).expect("encode"),
        &expectations(),
    )
    .expect("a residual file from another collection is individually valid");

    let mut other_segment = residual_segment();
    other_segment.identity.segment_id = [0x99; 16];
    let other_segment = ResidualFileReader::open_bytes(
        encode_residual_segment(&other_segment).expect("encode"),
        &expectations(),
    )
    .expect("a residual file from another segment is individually valid");

    assert!(matches!(
        PairedSegmentReaders::open(pinned_primary(), other_collection),
        Err(FormatError::CollectionMismatch)
    ));
    assert!(matches!(
        PairedSegmentReaders::open(pinned_primary(), other_segment),
        Err(FormatError::SegmentMismatch)
    ));
}

#[test]
fn the_pinned_pair_exposes_residual_rows_only_after_pairing_succeeds() {
    let residual = ResidualFileReader::open_bytes(RESIDUAL_FIXTURE.to_vec(), &expectations())
        .expect("the pinned residual fixture opens");
    let paired = PairedSegmentReaders::open(pinned_primary(), residual)
        .expect("the pinned pair shares every identity");

    for row in 0..ROW_COUNT {
        assert_eq!(paired.residual_code(row).expect("code"), residual_code(row));
    }
}

fn pinned_primary() -> PrimaryFileReader {
    PrimaryFileReader::open_bytes(PRIMARY_FIXTURE.to_vec(), &expectations())
        .expect("the pinned primary fixture opens")
}

/// Rewrites the checked-in fixtures. Deliberate major-version work only.
#[test]
#[ignore = "regenerating pinned fixtures is a deliberate format decision"]
fn regenerate_golden_fixtures() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::write(
        directory.join("primary-v1-minimal.bin"),
        encode_primary_segment(&primary_segment()).expect("encode"),
    )
    .expect("write the primary fixture");
    std::fs::write(
        directory.join("residual-v1-minimal.bin"),
        encode_residual_segment(&residual_segment()).expect("encode"),
    )
    .expect("write the residual fixture");
}
