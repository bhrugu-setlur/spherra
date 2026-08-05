use spherra_domain::{ChunkId, DIMENSION, DocumentId, PutSeq, ValidatedVector};

#[test]
fn dimension_and_sequence_contracts_are_stable() {
    assert_eq!(DIMENSION, 768);
    let seq = PutSeq::new(7, (1_u64 << 48) - 1).unwrap();
    assert_eq!(seq.epoch(), 7);
    assert_eq!(seq.index(), (1_u64 << 48) - 1);
    assert_eq!(seq.raw(), (7_u64 << 48) | ((1_u64 << 48) - 1));
    assert!(PutSeq::new(7, 1_u64 << 48).is_err());
    assert_ne!(ChunkId::from_u128(1), ChunkId::from_u128(2));
    assert_ne!(DocumentId::from_u128(1), DocumentId::from_u128(2));
}

#[test]
fn vector_validation_rejects_non_finite_and_marks_small_norms() {
    assert!(ValidatedVector::new(vec![0.0; 767]).is_err());

    let mut nan = vec![0.0; DIMENSION];
    nan[3] = f32::NAN;
    assert!(ValidatedVector::new(nan).is_err());

    let mut infinity = vec![0.0; DIMENSION];
    infinity[5] = f32::INFINITY;
    assert!(ValidatedVector::new(infinity).is_err());

    let zero = ValidatedVector::new(vec![0.0; DIMENSION]).unwrap();
    assert!(zero.direction_unreliable());
    assert!(zero.normalized_direction().is_none());

    let mut below_default_epsilon = vec![0.0; DIMENSION];
    below_default_epsilon[0] = 5.0e-13;
    assert!(
        ValidatedVector::new(below_default_epsilon)
            .unwrap()
            .direction_unreliable()
    );
}

#[test]
fn vector_validation_rejects_norms_that_cannot_fit_in_f16() {
    let mut at_limit = vec![0.0; DIMENSION];
    at_limit[0] = 65_504.0;
    let accepted = ValidatedVector::new(at_limit).unwrap();
    assert_eq!(accepted.radius_f32(), 65_504.0);

    let mut rounded_radius = vec![0.0; DIMENSION];
    rounded_radius[0] = 1.0001;
    assert_eq!(
        ValidatedVector::new(rounded_radius).unwrap().radius_f32(),
        1.0
    );

    let mut above_limit = vec![0.0; DIMENSION];
    above_limit[0] = 50_000.0;
    above_limit[1] = 50_000.0;
    assert!(ValidatedVector::new(above_limit).is_err());
}

#[test]
fn explicit_epsilon_controls_direction_reliability() {
    let mut below = vec![0.0; DIMENSION];
    below[0] = 0.5;
    let below = ValidatedVector::new_with_min_norm_epsilon(below, 1.0).unwrap();
    assert!(below.direction_unreliable());

    let mut boundary = vec![0.0; DIMENSION];
    boundary[0] = 1.0;
    let boundary = ValidatedVector::new_with_min_norm_epsilon(boundary, 1.0).unwrap();
    assert!(!boundary.direction_unreliable());
    assert_eq!(boundary.normalized_direction().unwrap().as_array()[0], 1.0);

    assert!(ValidatedVector::new_with_min_norm_epsilon(vec![0.0; DIMENSION], 0.0).is_err());
    assert!(ValidatedVector::new_with_min_norm_epsilon(vec![0.0; DIMENSION], f64::NAN).is_err());
}

#[test]
fn reliable_direction_is_normalized_from_the_fp64_norm() {
    let mut raw = vec![0.0; DIMENSION];
    raw[0] = 3.0;
    raw[1] = 4.0;

    let validated = ValidatedVector::new(raw).unwrap();
    let direction = validated.normalized_direction().unwrap().as_array();

    assert_eq!(validated.radius_f32(), 5.0);
    assert!((direction[0] - 0.6).abs() <= f32::EPSILON);
    assert!((direction[1] - 0.8).abs() <= f32::EPSILON);
}
