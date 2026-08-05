use core::array;

use spherra_codec::int4::{DirectCode, QuantizerTable, RadiusFlags, TrainError};
use spherra_codec::{TiledSoa32, TransformPlan, TransformedDirection, transform};
use spherra_domain::{DIMENSION, DomainError, ValidatedVector};

fn direct_code(nibbles: [u8; DIMENSION]) -> DirectCode {
    DirectCode::from_nibbles(nibbles).expect("test codes are four-bit values")
}

fn deterministic_nibbles(seed: u32) -> [u8; DIMENSION] {
    let mut state = seed;
    array::from_fn(|_| {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((state >> 28) & 0x0f) as u8
    })
}

fn coordinate_rank_calibration(rows: usize) -> Vec<[f32; DIMENSION]> {
    (0..rows)
        .map(|row| array::from_fn(|coordinate| (row * 1_000 + coordinate) as f32))
        .collect()
}

fn affine_calibration() -> Vec<[f32; DIMENSION]> {
    (0..16)
        .map(|row| array::from_fn(|coordinate| (row * 10 + coordinate) as f32))
        .collect()
}

fn affine_table() -> QuantizerTable {
    QuantizerTable::train(&affine_calibration()).expect("finite non-empty calibration")
}

fn transformed_query() -> TransformedDirection {
    let components = (0..DIMENSION)
        .map(|coordinate| (coordinate as f32 - 384.0) * 0.25)
        .collect();
    let vector = ValidatedVector::new(components).expect("finite test vector");
    let direction = vector
        .normalized_direction()
        .expect("non-zero test vector has a reliable direction");

    transform(&TransformPlan::from_seed(0x5eed_fade), direction)
}

#[test]
fn direct_code_and_layout_use_the_approved_byte_counts() {
    assert_eq!(DirectCode::BYTE_LEN, 384);
    assert_eq!(RadiusFlags::BYTE_LEN, 4);
    assert_eq!(TiledSoa32::direction_bytes_for_full_tile(), 12_288);

    let radius_flags = RadiusFlags::from_bytes([0x12, 0x34, 0x56, 0x78]);
    assert_eq!(radius_flags.as_bytes(), &[0x12, 0x34, 0x56, 0x78]);
}

#[test]
fn direct_code_packs_endpoints_and_deterministic_random_nibbles() {
    let endpoints = array::from_fn(|coordinate| match coordinate % 4 {
        0 => 0,
        1 => 15,
        2 => 7,
        _ => 8,
    });
    let endpoint_code = direct_code(endpoints);

    assert_eq!(endpoint_code.as_bytes()[0], 0xf0);
    assert_eq!(endpoint_code.as_bytes()[1], 0x87);
    assert_eq!(endpoint_code.to_nibbles(), endpoints);

    let random_nibbles = deterministic_nibbles(0x8bad_f00d);
    let random_code = direct_code(random_nibbles);
    assert_eq!(random_code.to_nibbles(), random_nibbles);
}

#[test]
fn quantizer_table_rejects_out_of_bounds_center_lookups() {
    let table = affine_table();

    assert_eq!(table.center(0, 0), Some(0.0));
    assert_eq!(table.center(0, 16), None);
    assert_eq!(table.center(DIMENSION, 0), None);
}

#[test]
fn directional_codec_paths_start_with_validated_transformed_direction() {
    let mut non_finite = vec![0.0; DIMENSION];
    non_finite[23] = f32::NAN;
    assert_eq!(
        ValidatedVector::new(non_finite),
        Err(DomainError::NonFiniteComponent { index: 23 })
    );

    let unreliable = ValidatedVector::new(vec![0.0; DIMENSION])
        .expect("a zero vector is finite but direction-unreliable");
    assert!(unreliable.normalized_direction().is_none());

    let query = transformed_query();
    let table = QuantizerTable::train(&[*query.as_array()])
        .expect("a reliable transformed direction is valid calibration");
    let code = table.encode(&query);
    let score = table.score(&query, &code);
    let tiled = TiledSoa32::from_codes(std::slice::from_ref(&code));

    assert_eq!(tiled.scan_scores(&table, &query), vec![score]);
}

#[test]
fn trainer_uses_the_user_approved_endpoint_inclusive_quantile_ranks() {
    let cases: &[(usize, [f32; 16])] = &[
        (1, [7.0; 16]),
        (
            2,
            [
                7.0, 7.0, 7.0, 7.0, 7.0, 7.0, 7.0, 7.0, 7.0, 7.0, 7.0, 7.0, 7.0, 7.0, 7.0, 1_007.0,
            ],
        ),
        (
            16,
            [
                7.0, 1_007.0, 2_007.0, 3_007.0, 4_007.0, 5_007.0, 6_007.0, 7_007.0, 8_007.0,
                9_007.0, 10_007.0, 11_007.0, 12_007.0, 13_007.0, 14_007.0, 15_007.0,
            ],
        ),
        (
            17,
            [
                7.0, 1_007.0, 2_007.0, 3_007.0, 4_007.0, 5_007.0, 6_007.0, 7_007.0, 8_007.0,
                9_007.0, 10_007.0, 11_007.0, 12_007.0, 13_007.0, 14_007.0, 16_007.0,
            ],
        ),
        (
            31,
            [
                7.0, 2_007.0, 4_007.0, 6_007.0, 8_007.0, 10_007.0, 12_007.0, 14_007.0, 16_007.0,
                18_007.0, 20_007.0, 22_007.0, 24_007.0, 26_007.0, 28_007.0, 30_007.0,
            ],
        ),
    ];

    for (row_count, expected_centers) in cases {
        let table = QuantizerTable::train(&coordinate_rank_calibration(*row_count))
            .expect("finite non-empty calibration");
        let actual_centers = array::from_fn(|center| {
            table
                .center(7, center)
                .expect("rank test requests valid coordinates and four-bit codes")
        });
        assert_eq!(actual_centers, *expected_centers, "row count {row_count}");
    }
}

#[test]
fn trainer_rejects_empty_and_non_finite_raw_calibration_with_context() {
    assert_eq!(
        QuantizerTable::train(&[]),
        Err(TrainError::EmptyCalibration)
    );

    let mut calibration = vec![[0.0; DIMENSION]; 2];
    calibration[1][23] = f32::NEG_INFINITY;
    assert_eq!(
        QuantizerTable::train(&calibration),
        Err(TrainError::NonFiniteValue {
            row: 1,
            coordinate: 23,
        })
    );
}

#[test]
fn trainer_canonicalizes_negative_zero_and_hashes_coordinate_major_little_endian_bytes() {
    let negative_zero = QuantizerTable::train(&[[-0.0; DIMENSION]])
        .expect("negative zero is a finite calibration value");
    let positive_zero = QuantizerTable::train(&[[0.0; DIMENSION]])
        .expect("positive zero is a finite calibration value");

    assert_eq!(negative_zero, positive_zero);
    assert!(
        negative_zero
            .centers()
            .iter()
            .all(|center| center.to_bits() == 0)
    );
    assert_eq!(
        negative_zero.identity(),
        &[
            0x82, 0x96, 0xdc, 0x1c, 0xd9, 0x1e, 0x5c, 0x14, 0xbf, 0xa7, 0x36, 0xd9, 0x67, 0xd0,
            0x99, 0xc5, 0x15, 0xc3, 0xa9, 0x54, 0x4e, 0xc4, 0xce, 0x03, 0x02, 0xb8, 0x11, 0xc1,
            0xfd, 0xe4, 0xc1, 0xf8,
        ]
    );

    let affine = affine_table();
    assert_eq!(affine.centers()[0].to_le_bytes(), [0x00, 0x00, 0x00, 0x00]);
    assert_eq!(affine.centers()[1].to_le_bytes(), [0x00, 0x00, 0x20, 0x41]);
    assert_eq!(affine.centers()[16].to_le_bytes(), [0x00, 0x00, 0x80, 0x3f]);
    assert_eq!(
        affine.identity(),
        &[
            0xf9, 0xfe, 0x0f, 0x43, 0xb7, 0x19, 0x4a, 0x29, 0x9c, 0x21, 0xf0, 0xa0, 0x82, 0xe5,
            0xb0, 0x35, 0xca, 0xc1, 0x72, 0x37, 0x63, 0xc9, 0x57, 0xd2, 0x5c, 0xb1, 0x9f, 0x96,
            0x52, 0xb5, 0xe8, 0x54,
        ]
    );
}

#[test]
fn quantizer_encodes_decodes_and_scores_using_its_trained_table() {
    let query = transformed_query();
    let table = QuantizerTable::train(&[*query.as_array()])
        .expect("a reliable transformed direction is valid calibration");
    let code = table.encode(&query);

    assert_eq!(code.as_bytes(), &[0; DirectCode::BYTE_LEN]);
    assert_eq!(table.decode(&code), *query.as_array());
    let expected_score = query
        .as_array()
        .iter()
        .fold(0.0, |score, value| score + value * value);
    assert_eq!(table.score(&query, &code), expected_score);

    let duplicate_table =
        QuantizerTable::train(&[[6.0; DIMENSION]]).expect("a singleton calibration is valid");
    assert_eq!(duplicate_table.encode(&query).nibble_at(0), 0);
}

#[test]
fn tiled_soa_full_tile_scan_matches_independent_per_row_scalar_scores() {
    let table = affine_table();
    let rows: Vec<DirectCode> = (0..32)
        .map(|row| {
            direct_code(array::from_fn(|coordinate| {
                ((row * 3 + coordinate * 5) % 16) as u8
            }))
        })
        .collect();
    let query = transformed_query();
    let tiled = TiledSoa32::from_codes(&rows);

    assert_eq!(tiled.row_count(), 32);
    assert_eq!(tiled.logical_direction_bytes(), 12_288);
    assert_eq!(tiled.physical_direction_bytes(), 12_288);
    assert_eq!(tiled.padding_direction_bytes(), 0);

    let tiled_scores = tiled.scan_scores(&table, &query);
    let scalar_scores: Vec<f32> = rows.iter().map(|code| table.score(&query, code)).collect();
    assert_eq!(tiled_scores, scalar_scores);
}

#[test]
fn tiled_soa_tail_rows_keep_logical_payload_and_physical_zero_padding_separate() {
    let all_fifteens = direct_code([15; DIMENSION]);
    let one_row = TiledSoa32::from_codes(std::slice::from_ref(&all_fifteens));

    assert_eq!(one_row.row_count(), 1);
    assert_eq!(one_row.logical_direction_bytes(), 384);
    assert_eq!(one_row.physical_direction_bytes(), 12_288);
    assert_eq!(one_row.padding_direction_bytes(), 11_904);
    assert_eq!(one_row.code_at(0), Some(all_fifteens.clone()));
    assert_eq!(one_row.code_at(1), None);
    for coordinate in 0..DIMENSION {
        let start = coordinate * 16;
        assert_eq!(one_row.as_bytes()[start], 0x0f);
        assert!(
            one_row.as_bytes()[start + 1..start + 16]
                .iter()
                .all(|byte| *byte == 0)
        );
    }

    let thirty_one_rows = vec![all_fifteens.clone(); 31];
    let thirty_one = TiledSoa32::from_codes(&thirty_one_rows);
    assert_eq!(thirty_one.row_count(), 31);
    assert_eq!(thirty_one.logical_direction_bytes(), 11_904);
    assert_eq!(thirty_one.physical_direction_bytes(), 12_288);
    assert_eq!(thirty_one.padding_direction_bytes(), 384);
    assert_eq!(thirty_one.code_at(30), Some(all_fifteens));
    assert_eq!(thirty_one.code_at(31), None);
    for coordinate in 0..DIMENSION {
        let start = coordinate * 16;
        assert!(
            thirty_one.as_bytes()[start..start + 15]
                .iter()
                .all(|byte| *byte == 0xff)
        );
        assert_eq!(thirty_one.as_bytes()[start + 15], 0x0f);
    }
}
