//! Task 7 rejection contract for the versioned segment foundation.
//!
//! Every case corrupts exactly one aspect of an otherwise valid minimal file.
//! The reader checks blocks before sections and sections before the whole-file
//! identity, so a single corruption always surfaces as its most specific error.

mod common;

use common::*;
use spherra_format::{
    FileKind, FormatError, PairedSegmentReaders, PrimaryFileReader, ResidualFileReader,
    SectionKind, encode_primary_segment, encode_residual_segment,
};

fn open_primary(bytes: Vec<u8>) -> Result<PrimaryFileReader, FormatError> {
    PrimaryFileReader::open_bytes(bytes, &expectations())
}

fn open_residual(bytes: Vec<u8>) -> Result<ResidualFileReader, FormatError> {
    ResidualFileReader::open_bytes(bytes, &expectations())
}

#[test]
fn minimal_primary_and_residual_files_open_and_expose_their_rows() {
    let primary = open_primary(primary_bytes()).expect("the minimal primary file is valid");
    let residual = open_residual(residual_bytes()).expect("the minimal residual file is valid");

    assert_eq!(primary.row_count(), ROW_COUNT);
    assert_eq!(primary.file_kind(), FileKind::Primary);
    assert_eq!(residual.file_kind(), FileKind::Residual);

    for row in 0..ROW_COUNT {
        assert_eq!(primary.row(row).expect("row entry"), row_entry(row));
        assert_eq!(
            primary.radius_flags(row).expect("radius flags"),
            radius_flags(row)
        );
        assert_eq!(
            primary.primary_code(row).expect("primary code"),
            primary_code(row)
        );
    }

    assert_eq!(primary.quantizer_table().expect("table"), quantizer_table());
    assert_eq!(
        primary.primary_certificate().expect("primary certificate"),
        primary_segment().primary_certificate
    );
    assert_eq!(
        primary.refined_certificate().expect("refined certificate"),
        primary_segment().refined_certificate
    );
    assert_eq!(residual.pq_codebook().expect("codebook"), pq_codebook());
}

#[test]
fn the_v1_header_places_every_field_at_its_documented_offset() {
    let bytes = primary_bytes();

    assert_eq!(HEADER_LEN, spherra_format::HEADER_LEN);
    assert_eq!(&bytes[OFFSET_MAGIC..OFFSET_MAGIC + 8], b"SPHERRA1");
    assert_eq!(
        read_u16(&bytes, OFFSET_MAJOR),
        spherra_format::MAJOR_VERSION
    );
    assert_eq!(
        read_u16(&bytes, OFFSET_MINOR),
        spherra_format::MINOR_VERSION
    );
    assert_eq!(read_u16(&bytes, OFFSET_FILE_KIND), 1);
    assert_eq!(read_u32(&bytes, OFFSET_HEADER_LEN), HEADER_LEN as u32);
    assert_eq!(
        &bytes[OFFSET_COLLECTION_ID..OFFSET_COLLECTION_ID + 16],
        COLLECTION_ID
    );
    assert_eq!(
        &bytes[OFFSET_SEGMENT_ID..OFFSET_SEGMENT_ID + 16],
        SEGMENT_ID
    );
    assert_eq!(read_u16(&bytes, OFFSET_DIMENSION), 768);
    assert_eq!(read_u32(&bytes, OFFSET_ROW_COUNT), ROW_COUNT);
    assert_eq!(&bytes[OFFSET_CODEC_ID..OFFSET_CODEC_ID + 32], CODEC_ID);
    assert_eq!(read_u32(&bytes, OFFSET_SCORER_VERSION), SCORER_VERSION);
    assert_eq!(
        &bytes[OFFSET_TRANSFORM_ID..OFFSET_TRANSFORM_ID + 32],
        TRANSFORM_ID
    );
    assert_eq!(
        &bytes[OFFSET_QUANTIZER_ID..OFFSET_QUANTIZER_ID + 32],
        QUANTIZER_ID
    );
    assert_eq!(
        &bytes[OFFSET_PQ_CODEBOOK_ID..OFFSET_PQ_CODEBOOK_ID + 32],
        PQ_CODEBOOK_ID
    );
    assert_eq!(read_u16(&bytes, OFFSET_LAYOUT_ID), 1);
    assert_eq!(
        read_u64(&bytes, OFFSET_DIRECTORY_OFFSET),
        HEADER_LEN as u64,
        "the section directory follows the header"
    );
    assert_eq!(
        read_u64(&bytes, OFFSET_PAYLOAD_LEN) + HEADER_LEN as u64,
        bytes.len() as u64
    );
    assert_eq!(
        DIRECTORY_ENTRY_LEN,
        spherra_format::DIRECTORY_ENTRY_LEN,
        "the pinned directory entry width is part of the durable contract"
    );
}

#[test]
fn row_accessors_reject_rows_outside_the_declared_count() {
    let primary = open_primary(primary_bytes()).expect("valid primary file");

    for row in [ROW_COUNT, ROW_COUNT + 1, u32::MAX] {
        assert!(matches!(
            primary.row(row),
            Err(FormatError::RowOutOfRange { .. })
        ));
        assert!(matches!(
            primary.radius_flags(row),
            Err(FormatError::RowOutOfRange { .. })
        ));
        assert!(matches!(
            primary.primary_code(row),
            Err(FormatError::RowOutOfRange { .. })
        ));
    }
}

#[test]
fn wrong_magic_is_rejected() {
    let mut bytes = primary_bytes();
    bytes[OFFSET_MAGIC] ^= 0xff;

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::WrongMagic { .. })
    ));
}

#[test]
fn wrong_major_version_is_rejected() {
    let mut bytes = primary_bytes();
    write_u16(&mut bytes, OFFSET_MAJOR, 2);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::UnsupportedMajorVersion { actual: 2 })
    ));
}

#[test]
fn unknown_minor_version_is_rejected() {
    let mut bytes = primary_bytes();
    write_u16(&mut bytes, OFFSET_MINOR, 9);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::UnsupportedMinorVersion { actual: 9 })
    ));
}

#[test]
fn unknown_file_kind_is_rejected() {
    let mut bytes = primary_bytes();
    write_u16(&mut bytes, OFFSET_FILE_KIND, 7);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::UnknownFileKind { actual: 7 })
    ));
}

#[test]
fn a_residual_file_cannot_be_opened_as_a_primary_file() {
    assert!(matches!(
        open_primary(residual_bytes()),
        Err(FormatError::UnexpectedFileKind {
            expected: FileKind::Primary,
            actual: FileKind::Residual,
        })
    ));
    assert!(matches!(
        open_residual(primary_bytes()),
        Err(FormatError::UnexpectedFileKind {
            expected: FileKind::Residual,
            actual: FileKind::Primary,
        })
    ));
}

#[test]
fn wrong_declared_header_length_is_rejected() {
    for declared in [0_u32, 239, 241, u32::MAX] {
        let mut bytes = primary_bytes();
        write_u32(&mut bytes, OFFSET_HEADER_LEN, declared);

        assert!(matches!(
            open_primary(bytes),
            Err(FormatError::WrongHeaderLength { .. })
        ));
    }
}

#[test]
fn wrong_dimension_is_rejected() {
    let mut bytes = primary_bytes();
    write_u16(&mut bytes, OFFSET_DIMENSION, 512);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::WrongDimension { actual: 512 })
    ));
}

#[test]
fn a_zero_row_segment_is_rejected() {
    let mut bytes = primary_bytes();
    write_u32(&mut bytes, OFFSET_ROW_COUNT, 0);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::EmptySegment)
    ));
}

#[test]
fn wrong_codec_scorer_transform_quantizer_or_codebook_identity_is_rejected() {
    let cases: [(usize, FormatError); 4] = [
        (OFFSET_CODEC_ID, FormatError::CodecMismatch),
        (OFFSET_TRANSFORM_ID, FormatError::TransformMismatch),
        (OFFSET_QUANTIZER_ID, FormatError::QuantizerMismatch),
        (OFFSET_PQ_CODEBOOK_ID, FormatError::CodebookMismatch),
    ];

    for (offset, expected) in cases {
        let mut bytes = primary_bytes();
        bytes[offset] ^= 0xff;
        assert_eq!(open_primary(bytes).err(), Some(expected));
    }

    let mut bytes = primary_bytes();
    write_u32(&mut bytes, OFFSET_SCORER_VERSION, SCORER_VERSION + 1);
    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::ScorerVersionMismatch { .. })
    ));
}

#[test]
fn unknown_or_unexpected_layout_is_rejected() {
    let mut bytes = primary_bytes();
    write_u16(&mut bytes, OFFSET_LAYOUT_ID, 0);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::UnknownLayout { actual: 0 })
    ));
}

#[test]
fn a_declared_payload_length_that_overflows_is_rejected() {
    let mut bytes = primary_bytes();
    write_u64(&mut bytes, OFFSET_PAYLOAD_LEN, u64::MAX);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::LengthOverflow)
    ));
}

#[test]
fn a_declared_payload_length_that_disagrees_with_the_file_is_rejected() {
    let mut bytes = primary_bytes();
    let declared = read_u64(&bytes, OFFSET_PAYLOAD_LEN);
    write_u64(&mut bytes, OFFSET_PAYLOAD_LEN, declared - 1);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::PayloadLengthMismatch { .. })
    ));
}

#[test]
fn a_section_directory_outside_the_file_is_rejected() {
    let mut bytes = primary_bytes();
    let length = bytes.len() as u64;
    write_u64(&mut bytes, OFFSET_DIRECTORY_OFFSET, length - 8);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::DirectoryOutOfFile)
    ));
}

#[test]
fn a_section_directory_overlapping_the_header_is_rejected() {
    let mut bytes = primary_bytes();
    write_u64(&mut bytes, OFFSET_DIRECTORY_OFFSET, 8);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::DirectoryOutOfFile)
    ));
}

#[test]
fn an_out_of_range_section_count_is_rejected() {
    for section_count in [0_u16, 1024, u16::MAX] {
        let mut bytes = primary_bytes();
        write_u16(&mut bytes, OFFSET_SECTION_COUNT, section_count);

        assert!(matches!(
            open_primary(bytes),
            Err(FormatError::SectionCountOutOfRange { .. } | FormatError::DirectoryOutOfFile)
        ));
    }
}

#[test]
fn a_section_length_that_overflows_the_file_offset_is_rejected() {
    let mut bytes = primary_bytes();
    let entry = entry_at(&bytes, 0);
    write_u64(&mut bytes, entry + ENTRY_OFFSET_LENGTH, u64::MAX);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::LengthOverflow | FormatError::SectionOutsideFile { .. })
    ));
}

#[test]
fn a_section_that_ends_outside_the_file_is_rejected() {
    let mut bytes = primary_bytes();
    let entry = entry_at(&bytes, 0);
    // Still aligned, so the offset check cannot mask the out-of-file fault.
    let past_end = (bytes.len() as u64).next_multiple_of(64);
    write_u64(&mut bytes, entry + ENTRY_OFFSET_OFFSET, past_end);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::SectionOutsideFile { .. })
    ));
}

#[test]
fn overlapping_sections_are_rejected() {
    let mut bytes = primary_bytes();
    let first = entry_at(&bytes, 0);
    let second = entry_at(&bytes, 1);
    let first_offset = read_u64(&bytes, first + ENTRY_OFFSET_OFFSET);
    let first_length = read_u64(&bytes, first + ENTRY_OFFSET_LENGTH);
    assert!(first_length > 8, "the first fixture section is non-trivial");
    // Aligned and still monotonic, but it restarts inside the previous section.
    write_u64(&mut bytes, second + ENTRY_OFFSET_OFFSET, first_offset);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::OverlappingSections { .. })
    ));
}

#[test]
fn a_non_monotonic_section_directory_is_rejected() {
    let mut bytes = primary_bytes();
    let first_offset = read_u64(&bytes, entry_at(&bytes, 0) + ENTRY_OFFSET_OFFSET);
    let third = entry_at(&bytes, 2);
    // A valid, aligned, in-file offset that simply appears out of order.
    write_u64(&mut bytes, third + ENTRY_OFFSET_OFFSET, first_offset);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::NonMonotonicDirectory { .. })
    ));
}

#[test]
fn a_section_alignment_that_is_not_a_supported_power_of_two_is_rejected() {
    for alignment in [0_u32, 3, 24, 8192, u32::MAX] {
        let mut bytes = primary_bytes();
        let entry = entry_at(&bytes, 0);
        write_u32(&mut bytes, entry + ENTRY_OFFSET_ALIGNMENT, alignment);

        assert!(
            matches!(
                open_primary(bytes),
                Err(FormatError::InvalidAlignment { .. })
            ),
            "alignment {alignment} must be rejected"
        );
    }
}

#[test]
fn a_section_offset_that_violates_its_declared_alignment_is_rejected() {
    let mut bytes = primary_bytes();
    let entry = entry_at(&bytes, 0);
    let offset = read_u64(&bytes, entry + ENTRY_OFFSET_OFFSET);
    write_u64(&mut bytes, entry + ENTRY_OFFSET_OFFSET, offset + 1);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::MisalignedSection { .. })
    ));
}

#[test]
fn a_section_row_count_that_disagrees_with_the_header_is_rejected() {
    let mut bytes = primary_bytes();
    let entry = entry_at(&bytes, index_of_kind(&bytes, KIND_IDS_SEQUENCES));
    write_u32(&mut bytes, entry + ENTRY_OFFSET_LOGICAL_ROW_COUNT, 1);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::SectionRowCountMismatch { .. })
    ));
}

#[test]
fn a_section_length_that_disagrees_with_its_kind_is_rejected() {
    let mut bytes = primary_bytes();
    let entry = entry_at(&bytes, index_of_kind(&bytes, KIND_RADIUS_FLAGS));
    let length = read_u64(&bytes, entry + ENTRY_OFFSET_LENGTH);
    write_u64(&mut bytes, entry + ENTRY_OFFSET_LENGTH, length - 4);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::SectionLengthMismatch { .. })
    ));
}

#[test]
fn a_reserved_directory_field_that_is_not_zero_is_rejected() {
    for offset in [ENTRY_OFFSET_RESERVED_U16, ENTRY_OFFSET_RESERVED_U32] {
        let mut bytes = primary_bytes();
        let entry = entry_at(&bytes, 0);
        bytes[entry + offset] = 1;

        assert!(matches!(
            open_primary(bytes),
            Err(FormatError::ReservedFieldNotZero { .. })
        ));
    }
}

#[test]
fn an_unknown_section_flag_is_rejected() {
    let mut bytes = primary_bytes();
    let entry = entry_at(&bytes, 0);
    write_u16(&mut bytes, entry + ENTRY_OFFSET_FLAGS, 0x8000);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::UnknownSectionFlags { .. })
    ));
}

#[test]
fn an_unknown_section_kind_is_rejected() {
    let mut bytes = primary_bytes();
    let entry = entry_at(&bytes, 0);
    write_u16(&mut bytes, entry + ENTRY_OFFSET_KIND, 4242);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::UnknownSectionKind { .. })
    ));
}

#[test]
fn a_duplicated_section_kind_is_rejected() {
    let mut bytes = primary_bytes();
    let entry = entry_at(&bytes, index_of_kind(&bytes, KIND_REFINED_CERTIFICATE));
    write_u16(
        &mut bytes,
        entry + ENTRY_OFFSET_KIND,
        KIND_PRIMARY_CERTIFICATE,
    );

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::DuplicateSection { .. })
    ));
}

#[test]
fn a_missing_required_section_is_rejected() {
    let mut bytes = primary_bytes();
    let index = index_of_kind(&bytes, KIND_PRIMARY_CERTIFICATE);
    let entry = entry_at(&bytes, index);
    write_u16(&mut bytes, entry + ENTRY_OFFSET_KIND, KIND_CRC_TABLE);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::MissingSection { .. } | FormatError::SectionLengthMismatch { .. })
    ));
}

#[test]
fn a_residual_section_inside_a_primary_file_is_rejected() {
    let mut bytes = primary_bytes();
    let entry = entry_at(&bytes, index_of_kind(&bytes, KIND_PRIMARY_CERTIFICATE));
    write_u16(&mut bytes, entry + ENTRY_OFFSET_KIND, KIND_PQ96_RESIDUAL);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::ForbiddenSection {
            kind: SectionKind::Pq96Residual,
            ..
        })
    ));
}

#[test]
fn a_primary_section_inside_a_residual_file_is_rejected() {
    let mut bytes = residual_bytes();
    let entry = entry_at(&bytes, index_of_kind(&bytes, KIND_PQ_CODEBOOK));
    write_u16(
        &mut bytes,
        entry + ENTRY_OFFSET_KIND,
        KIND_PRIMARY_DIRECT_INT4,
    );

    assert!(matches!(
        open_residual(bytes),
        Err(FormatError::ForbiddenSection {
            kind: SectionKind::PrimaryDirectInt4,
            ..
        })
    ));
}

#[test]
fn a_crc_table_reference_outside_the_directory_is_rejected() {
    let mut bytes = primary_bytes();
    let entry = entry_at(&bytes, index_of_kind(&bytes, KIND_IDS_SEQUENCES));
    write_u16(&mut bytes, entry + ENTRY_OFFSET_CRC_TABLE_INDEX, 900);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::InvalidCrcTableReference { .. })
    ));
}

#[test]
fn a_data_section_without_a_crc_table_is_rejected() {
    let mut bytes = primary_bytes();
    let entry = entry_at(&bytes, index_of_kind(&bytes, KIND_IDS_SEQUENCES));
    write_u16(&mut bytes, entry + ENTRY_OFFSET_CRC_TABLE_INDEX, u16::MAX);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::MissingCrcTable { .. })
    ));
}

#[test]
fn any_block_size_other_than_the_single_v1_block_size_is_rejected() {
    // A large declared block size still sizes its CRC table consistently — one
    // block, four bytes — so nothing downstream catches it. Found by fuzzing:
    // the verifier allocated the declared size before checksumming.
    for block_size in [0, 1, 2048, 8192, 0x9c10_0c41, u32::MAX] {
        let mut bytes = primary_bytes();
        let entry = entry_at(&bytes, index_of_kind(&bytes, KIND_IDS_SEQUENCES));
        write_u32(&mut bytes, entry + ENTRY_OFFSET_BLOCK_SIZE, block_size);

        assert!(
            matches!(
                open_primary(bytes),
                Err(FormatError::InvalidBlockSize { .. })
            ),
            "block size {block_size} must be rejected"
        );
    }

    assert_eq!(
        read_u32(
            &primary_bytes(),
            entry_at(&primary_bytes(), 0) + ENTRY_OFFSET_BLOCK_SIZE
        ),
        spherra_format::BLOCK_SIZE,
    );
}

#[test]
fn a_crc_table_shared_by_two_data_sections_is_rejected() {
    let mut bytes = primary_bytes();
    let borrowed = read_u16(
        &bytes,
        entry_at(&bytes, index_of_kind(&bytes, KIND_RADIUS_FLAGS)) + ENTRY_OFFSET_CRC_TABLE_INDEX,
    );
    let entry = entry_at(&bytes, index_of_kind(&bytes, KIND_IDS_SEQUENCES));
    write_u16(&mut bytes, entry + ENTRY_OFFSET_CRC_TABLE_INDEX, borrowed);

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::CrcTableAliased { .. } | FormatError::SectionLengthMismatch { .. })
    ));
}

#[test]
fn a_corrupted_data_block_is_rejected_by_its_block_checksum() {
    let mut bytes = primary_bytes();
    let entry = entry_at(&bytes, index_of_kind(&bytes, KIND_PRIMARY_DIRECT_INT4));
    let offset = read_u64(&bytes, entry + ENTRY_OFFSET_OFFSET) as usize;
    bytes[offset] ^= 0x01;

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::BlockChecksumMismatch { .. })
    ));
}

#[test]
fn a_corrupted_section_identity_is_rejected() {
    let mut bytes = primary_bytes();
    let entry = entry_at(&bytes, index_of_kind(&bytes, KIND_INT4_QUANTIZER_TABLE));
    bytes[entry + ENTRY_OFFSET_IDENTITY] ^= 0xff;

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::SectionIdentityMismatch { .. })
    ));
}

#[test]
fn a_mismatched_whole_file_blake3_is_rejected() {
    let mut bytes = primary_bytes();
    bytes[OFFSET_WHOLE_FILE_BLAKE3] ^= 0xff;

    assert!(matches!(
        open_primary(bytes),
        Err(FormatError::FileIdentityMismatch)
    ));
}

#[test]
fn truncation_at_every_byte_boundary_is_rejected_without_panicking() {
    for bytes in [primary_bytes(), residual_bytes()] {
        let is_primary = read_u16(&bytes, OFFSET_FILE_KIND) == 1;
        for length in 0..bytes.len() {
            let truncated = bytes[..length].to_vec();
            let outcome = if is_primary {
                open_primary(truncated).map(|_| ())
            } else {
                open_residual(truncated).map(|_| ())
            };
            assert!(
                outcome.is_err(),
                "a file truncated to {length} bytes must be rejected"
            );
        }
    }
}

#[test]
fn every_single_byte_corruption_of_the_minimal_files_is_rejected() {
    // Exhaustive over the header and section directory, where a silent
    // acceptance would misdirect every later positional read.
    for bytes in [primary_bytes(), residual_bytes()] {
        let is_primary = read_u16(&bytes, OFFSET_FILE_KIND) == 1;
        let section_count = read_u16(&bytes, OFFSET_SECTION_COUNT) as usize;
        let directory_end = entry_at(&bytes, section_count);

        for index in 0..directory_end {
            let mut corrupted = bytes.clone();
            corrupted[index] ^= 0xff;
            let outcome = if is_primary {
                open_primary(corrupted).map(|_| ())
            } else {
                open_residual(corrupted).map(|_| ())
            };
            assert!(
                outcome.is_err(),
                "byte {index} of the header or directory must be authenticated"
            );
        }
    }
}

#[test]
fn pairing_rejects_a_residual_file_with_a_different_row_count() {
    let mut short = residual_segment();
    short.row_count = 1;
    short.residual_codes.truncate(1);
    let short = open_residual(encode_residual_segment(&short).expect("a valid residual segment"))
        .expect("a one-row residual file is individually valid");

    assert!(matches!(
        PairedSegmentReaders::open(open_primary(primary_bytes()).expect("valid"), short),
        Err(FormatError::RowCountMismatch { .. })
    ));
}

#[test]
fn pairing_exposes_the_only_public_residual_row_accessor() {
    let paired = PairedSegmentReaders::open(
        open_primary(primary_bytes()).expect("valid primary file"),
        open_residual(residual_bytes()).expect("valid residual file"),
    )
    .expect("matching identities pair");

    for row in 0..ROW_COUNT {
        assert_eq!(paired.residual_code(row).expect("code"), residual_code(row));
    }
    assert!(matches!(
        paired.residual_code(ROW_COUNT),
        Err(FormatError::RowOutOfRange { .. })
    ));
}

#[test]
fn encoding_is_deterministic_and_staging_reproduces_it() {
    let expected = primary_bytes();
    assert_eq!(
        encode_primary_segment(&primary_segment()).expect("re-encode"),
        expected
    );

    let directory = tempfile::tempdir().expect("a temporary directory");
    let staged = spherra_format::stage_primary_segment(directory.path(), &primary_segment())
        .expect("staging the primary segment");

    assert_eq!(staged.len(), expected.len() as u64);
    assert_eq!(
        std::fs::read(staged.path()).expect("the staged file is readable"),
        expected
    );
    assert_eq!(
        staged.identity(),
        read_whole_file_identity(&expected),
        "the staged descriptor reports the file's BLAKE3 identity"
    );

    PrimaryFileReader::open_path(staged.path(), &expectations()).expect("the staged file opens");
}

fn read_whole_file_identity(bytes: &[u8]) -> [u8; 32] {
    bytes[OFFSET_WHOLE_FILE_BLAKE3..OFFSET_WHOLE_FILE_BLAKE3 + 32]
        .try_into()
        .expect("thirty-two identity bytes")
}
