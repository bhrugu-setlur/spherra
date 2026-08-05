use core::array;

use spherra_codec::{CodecError, Pq96Code, Pq96Codebook, PreparedQuery};
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

fn movement_calibration_residuals() -> Vec<[f32; DIMENSION]> {
    let mut residuals = Vec::with_capacity(Pq96Code::CENTROIDS + 1);
    residuals.push([-1.0; DIMENSION]);
    residuals.push([1.0; DIMENSION]);
    for group in 1..Pq96Code::CENTROIDS {
        residuals.push([group as f32 * 1_000_000.0; DIMENSION]);
    }
    residuals
}

fn empty_cluster_calibration_residuals() -> Vec<[f32; DIMENSION]> {
    (0..Pq96Code::CENTROIDS)
        .map(|row| {
            if row < Pq96Code::CENTROIDS / 2 {
                [0.0; DIMENSION]
            } else {
                [10.0; DIMENSION]
            }
        })
        .collect()
}

fn squared_reconstruction_error(codebook: &Pq96Codebook, residuals: &[[f32; DIMENSION]]) -> f64 {
    let mut total_error = 0.0;
    for residual in residuals {
        let decoded = codebook.decode(
            &codebook
                .encode(residual)
                .expect("calibration residuals are finite"),
        );
        for (actual, reconstructed) in residual.iter().zip(decoded) {
            let difference = f64::from(*actual) - f64::from(reconstructed);
            total_error += difference * difference;
        }
    }
    total_error
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
    assert_eq!(first.training_diagnostics(), repeat.training_diagnostics());
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

#[test]
fn pq96_public_codec_boundaries_reject_invalid_inputs() {
    let calibration = calibration_residuals();
    let codebook = Pq96Codebook::train(&calibration, 13).expect("calibration residuals are valid");

    let mut residual = calibration[0];
    residual[23] = f32::INFINITY;
    assert!(matches!(
        codebook.encode(&residual),
        Err(CodecError::NonFiniteEncodedResidual { coordinate: 23 })
    ));

    let mut query = [0.0; DIMENSION];
    query[31] = f32::NAN;
    assert!(matches!(
        PreparedQuery::from_transformed(query),
        Err(CodecError::NonFinitePreparedQuery { coordinate: 31 })
    ));

    assert!(matches!(
        codebook.centroid(Pq96Code::SUBQUANTIZERS, 0),
        Err(CodecError::InvalidSubquantizer { index })
            if index == Pq96Code::SUBQUANTIZERS
    ));
}

#[test]
fn public_training_moves_centroids_and_recovers_empty_clusters() {
    let movement_calibration = movement_calibration_residuals();
    let moved = Pq96Codebook::train(&movement_calibration, 42)
        .expect("movement calibration residuals are valid");

    assert_eq!(moved.training_diagnostics().empty_cluster_reseeds(), 0);
    assert!(moved.training_diagnostics().centroid_moves() > 0);
    assert!(
        squared_reconstruction_error(&moved, &movement_calibration) < 2_000.0,
        "Lloyd updates should reconstruct the two near-origin rows through their mean"
    );

    let recovered = Pq96Codebook::train(&empty_cluster_calibration_residuals(), 43)
        .expect("empty-cluster calibration residuals are valid");
    let diagnostics = recovered.training_diagnostics();
    assert_eq!(
        diagnostics.lloyd_iterations(),
        (Pq96Code::SUBQUANTIZERS * 2) as u32,
        "a real reseed receives one follow-up pass, then no-op reseeding permits convergence"
    );
    assert_eq!(
        diagnostics.empty_cluster_reseeds(),
        (Pq96Code::SUBQUANTIZERS * (Pq96Code::CENTROIDS - 2) * 2) as u32,
        "two distinct rows leave 254 empty centroids in each of the two Lloyd passes"
    );
    assert!(
        diagnostics.centroid_moves() > 0,
        "the public diagnostics must prove that the first pass performed real reseeds"
    );
}
