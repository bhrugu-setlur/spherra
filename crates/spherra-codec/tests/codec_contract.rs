use core::array;

use spherra_codec::{CodecError, Pq96Code, Pq96Codebook};
use spherra_domain::DIMENSION;

fn calibration_residuals() -> Vec<[f32; DIMENSION]> {
    (0..Pq96Code::CENTROIDS)
        .map(|row| {
            array::from_fn(|coordinate| {
                let subquantizer = coordinate / Pq96Code::SUBVECTOR_DIMENSION;
                let lane = coordinate % Pq96Code::SUBVECTOR_DIMENSION;
                row as f32 + (subquantizer as f32 * 0.01) + (lane as f32 * 0.001)
            })
        })
        .collect()
}

#[test]
fn pq96_constants_and_training_are_canonical_and_deterministic() {
    assert_eq!(Pq96Code::SUBQUANTIZERS, 96);
    assert_eq!(Pq96Code::SUBVECTOR_DIMENSION, 8);
    assert_eq!(Pq96Code::CENTROIDS, 256);
    assert_eq!(Pq96Code::BYTE_LEN, 96);

    let calibration = calibration_residuals();
    let first = Pq96Codebook::train(&calibration, 7).expect("calibration residuals are valid");
    let repeat = Pq96Codebook::train(&calibration, 7).expect("calibration residuals are valid");

    assert_eq!(first.canonical_bytes(), repeat.canonical_bytes());
    assert_eq!(first.codebook_id(), repeat.codebook_id());
    assert_eq!(
        first.canonical_bytes().len(),
        96 * 256 * 8 * size_of::<f32>()
    );
}

#[test]
fn pq96_code_decodes_each_subquantizer_from_its_selected_centroid() {
    let calibration = calibration_residuals();
    let codebook = Pq96Codebook::train(&calibration, 11).expect("calibration residuals are valid");
    let residual = calibration[113];

    let code = codebook
        .encode(&residual)
        .expect("known residual is finite");
    let decoded = codebook.decode(&code);

    for subquantizer in 0..Pq96Code::SUBQUANTIZERS {
        let start = subquantizer * Pq96Code::SUBVECTOR_DIMENSION;
        let end = start + Pq96Code::SUBVECTOR_DIMENSION;
        let centroid = codebook
            .centroid(subquantizer, code.as_bytes()[subquantizer])
            .expect("subquantizer index is in range");

        assert_eq!(&decoded[start..end], centroid);
    }
}

#[test]
fn pq96_training_rejects_incomplete_or_non_finite_residuals() {
    let insufficient = vec![[0.0; DIMENSION]; Pq96Code::CENTROIDS - 1];
    assert!(matches!(
        Pq96Codebook::train(&insufficient, 0),
        Err(CodecError::InsufficientCalibrationRows { .. })
    ));

    let mut non_finite = calibration_residuals();
    non_finite[5][17] = f32::NAN;
    assert!(matches!(
        Pq96Codebook::train(&non_finite, 0),
        Err(CodecError::NonFiniteResidual {
            row: 5,
            coordinate: 17
        })
    ));
}
