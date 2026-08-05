use proptest::prelude::*;
use spherra_codec::{TransformPlan, TransformSpec, inverse_transform, transform};
use spherra_domain::{DIMENSION, ReliableDirection, ValidatedVector};

const TOLERANCE: f32 = 2.0e-4;

fn reliable_direction(mut raw: Vec<f32>) -> ReliableDirection {
    if raw.iter().all(|value| *value == 0.0) {
        raw[0] = 1.0;
    }

    ValidatedVector::new(raw)
        .expect("property inputs are finite and 768-dimensional")
        .normalized_direction()
        .expect("non-zero property inputs are direction-reliable")
        .clone()
}

fn l2(values: &[f32; DIMENSION]) -> f32 {
    values
        .iter()
        .map(|value| {
            let value = f64::from(*value);
            value * value
        })
        .sum::<f64>()
        .sqrt() as f32
}

fn dot(left: &[f32; DIMENSION], right: &[f32; DIMENSION]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(left, right)| f64::from(*left) * f64::from(*right))
        .sum::<f64>() as f32
}

fn max_abs_diff(left: &[f32; DIMENSION], right: &[f32; DIMENSION]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(left, right)| (*left - *right).abs())
        .fold(0.0_f32, f32::max)
}

#[test]
fn transform_spec_records_the_unpadded_two_round_layout() {
    let spec = TransformSpec::current();

    assert_eq!(spec.dimension(), DIMENSION);
    assert_eq!(spec.rounds(), 2);
    assert_eq!(spec.blocks_per_round(), 6);
    assert_eq!(spec.block_len(), 128);
    assert_eq!(spec.blocks_per_round() * spec.block_len(), spec.dimension());
}

#[test]
fn transform_plan_identity_is_seed_bound_and_deterministic() {
    let first = TransformPlan::from_seed(42);
    let repeat = TransformPlan::from_seed(42);
    let distinct = TransformPlan::from_seed(43);

    assert_eq!(first.identity(), repeat.identity());
    assert_ne!(first.identity(), distinct.identity());
    assert_eq!(first.identity().len(), 32);
}

proptest! {
    #[test]
    fn transform_preserves_norm_and_inverts(
        seed in any::<u64>(),
        raw in prop::collection::vec(-4.0f32..4.0, DIMENSION),
    ) {
        let plan = TransformPlan::from_seed(seed);
        let direction = reliable_direction(raw);
        let transformed = transform(&plan, &direction);
        let recovered = inverse_transform(&plan, &transformed);

        prop_assert!(
            (l2(direction.as_array()) - l2(transformed.as_array())).abs()
                <= TOLERANCE * l2(direction.as_array()).max(1.0)
        );
        prop_assert!(max_abs_diff(direction.as_array(), &recovered) <= TOLERANCE);
    }

    #[test]
    fn transform_preserves_dot_product(
        seed in any::<u64>(),
        left_raw in prop::collection::vec(-4.0f32..4.0, DIMENSION),
        right_raw in prop::collection::vec(-4.0f32..4.0, DIMENSION),
    ) {
        let plan = TransformPlan::from_seed(seed);
        let left = reliable_direction(left_raw);
        let right = reliable_direction(right_raw);
        let transformed_left = transform(&plan, &left);
        let transformed_right = transform(&plan, &right);

        prop_assert!(
            (dot(left.as_array(), right.as_array())
                - dot(transformed_left.as_array(), transformed_right.as_array()))
                .abs()
                <= TOLERANCE
        );
    }
}
