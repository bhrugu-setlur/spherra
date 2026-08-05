use spherra_simd::scalar::{HADAMARD_BLOCK_LEN, hadamard_128};

const TOLERANCE: f32 = 2.0e-5;

fn l2(values: &[f32; HADAMARD_BLOCK_LEN]) -> f32 {
    values
        .iter()
        .map(|value| {
            let value = f64::from(*value);
            value * value
        })
        .sum::<f64>()
        .sqrt() as f32
}

fn dot(left: &[f32; HADAMARD_BLOCK_LEN], right: &[f32; HADAMARD_BLOCK_LEN]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(left, right)| f64::from(*left) * f64::from(*right))
        .sum::<f64>() as f32
}

fn max_abs_diff(left: &[f32; HADAMARD_BLOCK_LEN], right: &[f32; HADAMARD_BLOCK_LEN]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(left, right)| (*left - *right).abs())
        .fold(0.0_f32, f32::max)
}

#[test]
fn normalized_hadamard_128_preserves_norm_and_is_its_own_inverse() {
    let input = core::array::from_fn(|index| {
        let index = index as f32;
        index.mul_add(0.25, -13.0)
    });

    let transformed = hadamard_128(&input);
    let recovered = hadamard_128(&transformed);

    assert!((l2(&input) - l2(&transformed)).abs() <= TOLERANCE * l2(&input).max(1.0));
    assert!(max_abs_diff(&input, &recovered) <= TOLERANCE);
}

#[test]
fn normalized_hadamard_128_preserves_dot_product() {
    let left = core::array::from_fn(|index| (index as f32).mul_add(0.125, -4.0));
    let right = core::array::from_fn(|index| ((index * 17 % 31) as f32).mul_add(0.5, -7.0));

    let transformed_left = hadamard_128(&left);
    let transformed_right = hadamard_128(&right);

    assert!((dot(&left, &right) - dot(&transformed_left, &transformed_right)).abs() <= TOLERANCE);
}
