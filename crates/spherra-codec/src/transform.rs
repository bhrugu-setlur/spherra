use core::array;

use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;
use spherra_domain::{DIMENSION, ReliableDirection};
use spherra_simd::scalar::{HADAMARD_BLOCK_LEN, hadamard_128};

use crate::spec::TransformSpec;

const ROUND_SEED_DERIVATION_CONTEXT: &str = "spherra.transform.round-seed.v1";
const IDENTITY_DERIVATION_CONTEXT: &str = "spherra.transform.identity.v1";
const ROUND_COUNT: usize = 2;
const BLOCKS_PER_ROUND: usize = DIMENSION / HADAMARD_BLOCK_LEN;
const TRANSFORM_IDENTITY_LEN: usize = 32;
const IDENTITY_MATERIAL_LEN: usize = 8 + (4 * 2) + (ROUND_COUNT * TRANSFORM_IDENTITY_LEN);

#[derive(Clone, Debug, PartialEq)]
pub struct TransformedDirection([f32; DIMENSION]);

impl TransformedDirection {
    pub const fn as_array(&self) -> &[f32; DIMENSION] {
        &self.0
    }
}

/// A finite, non-zero FP32 kernel input derived directly from one normalized
/// FP64 vector. It is crate-private so callers cannot bypass the public domain
/// validation route for arbitrary raw vectors.
///
/// The scorer's certificate proof accounts for the one FP64-to-FP32 conversion
/// performed here. This type deliberately does not compute another norm or
/// divide components again: doing so would be a distinct numerical path that
/// needs its own constructive error terms.
#[derive(Clone, Debug)]
pub(crate) struct KernelInputDirection([f32; DIMENSION]);

impl KernelInputDirection {
    pub(crate) fn from_normalized_fp64(normalized: &[f64; DIMENSION]) -> Option<Self> {
        let mut values = [0.0; DIMENSION];
        let mut non_zero = false;
        for (coordinate, value) in normalized.iter().copied().enumerate() {
            if !value.is_finite() {
                return None;
            }
            let rounded = value as f32;
            if !rounded.is_finite() {
                return None;
            }
            non_zero |= rounded != 0.0;
            values[coordinate] = rounded;
        }
        non_zero.then_some(Self(values))
    }
}

#[derive(Clone, Debug)]
pub struct TransformPlan {
    spec: TransformSpec,
    identity: [u8; TRANSFORM_IDENTITY_LEN],
    rounds: [TransformRound; ROUND_COUNT],
}

impl TransformPlan {
    /// Builds the provisional Task 3 identity from canonical little-endian fields.
    ///
    /// Round seeds are BLAKE3-derived from the collection seed and round index. The
    /// identity is a separate BLAKE3 derivation over that seed, the fixed transform
    /// shape, and both round seeds. The domain-separation contexts above make this
    /// initial identity format explicit until a later benchmark-selected format freezes it.
    pub fn from_seed(collection_seed: u64) -> Self {
        let spec = TransformSpec::current();
        let round_seeds = array::from_fn(|round| derive_round_seed(collection_seed, round));
        let rounds = array::from_fn(|round| TransformRound::from_seed(round_seeds[round]));

        Self {
            spec,
            identity: derive_identity(collection_seed, spec, &round_seeds),
            rounds,
        }
    }

    pub const fn identity(&self) -> &[u8; TRANSFORM_IDENTITY_LEN] {
        &self.identity
    }

    pub const fn spec(&self) -> TransformSpec {
        self.spec
    }
}

#[derive(Clone, Debug)]
struct TransformRound {
    signs: [f32; DIMENSION],
    permutation: [usize; DIMENSION],
    inverse_permutation: [usize; DIMENSION],
}

impl TransformRound {
    fn from_seed(seed: [u8; TRANSFORM_IDENTITY_LEN]) -> Self {
        let mut rng = ChaCha20Rng::from_seed(seed);
        let signs = array::from_fn(|_| if rng.next_u32() & 1 == 0 { 1.0 } else { -1.0 });
        let mut permutation = array::from_fn(|index| index);

        for index in (1..DIMENSION).rev() {
            let swap_index = uniform_index(&mut rng, index);
            permutation.swap(index, swap_index);
        }

        let mut inverse_permutation = [0; DIMENSION];
        for (destination, source) in permutation.iter().enumerate() {
            inverse_permutation[*source] = destination;
        }

        Self {
            signs,
            permutation,
            inverse_permutation,
        }
    }
}

fn uniform_index(rng: &mut ChaCha20Rng, upper_inclusive: usize) -> usize {
    let bound = u32::try_from(upper_inclusive + 1)
        .expect("the fixed 768-dimensional permutation fits in u32");
    let accepted = u32::MAX - (u32::MAX % bound);

    loop {
        let candidate = rng.next_u32();
        if candidate < accepted {
            return (candidate % bound) as usize;
        }
    }
}

pub fn transform(plan: &TransformPlan, direction: &ReliableDirection) -> TransformedDirection {
    transform_values(plan, *direction.as_array())
}

/// Executes the frozen transform on a kernel input that has already passed the
/// scorer's FP64-normalization and one-rounding validation. It never performs
/// a second normalization.
pub(crate) fn transform_kernel_input(
    plan: &TransformPlan,
    direction: &KernelInputDirection,
) -> TransformedDirection {
    transform_values(plan, direction.0)
}

fn transform_values(plan: &TransformPlan, mut values: [f32; DIMENSION]) -> TransformedDirection {
    for round in &plan.rounds {
        values = apply_forward_round(values, round);
    }

    TransformedDirection(values)
}

pub fn inverse_transform(
    plan: &TransformPlan,
    transformed: &TransformedDirection,
) -> [f32; DIMENSION] {
    let mut values = *transformed.as_array();

    for round in plan.rounds.iter().rev() {
        values = apply_inverse_round(values, round);
    }

    values
}

fn derive_round_seed(collection_seed: u64, round: usize) -> [u8; TRANSFORM_IDENTITY_LEN] {
    let mut material = [0_u8; 10];
    material[..8].copy_from_slice(&collection_seed.to_le_bytes());
    material[8..].copy_from_slice(&(round as u16).to_le_bytes());
    blake3::derive_key(ROUND_SEED_DERIVATION_CONTEXT, &material)
}

fn derive_identity(
    collection_seed: u64,
    spec: TransformSpec,
    round_seeds: &[[u8; TRANSFORM_IDENTITY_LEN]; ROUND_COUNT],
) -> [u8; TRANSFORM_IDENTITY_LEN] {
    let mut material = [0_u8; IDENTITY_MATERIAL_LEN];
    material[..8].copy_from_slice(&collection_seed.to_le_bytes());
    material[8..10].copy_from_slice(&(spec.dimension() as u16).to_le_bytes());
    material[10..12].copy_from_slice(&(spec.rounds() as u16).to_le_bytes());
    material[12..14].copy_from_slice(&(spec.blocks_per_round() as u16).to_le_bytes());
    material[14..16].copy_from_slice(&(spec.block_len() as u16).to_le_bytes());
    material[16..48].copy_from_slice(&round_seeds[0]);
    material[48..80].copy_from_slice(&round_seeds[1]);
    blake3::derive_key(IDENTITY_DERIVATION_CONTEXT, &material)
}

fn apply_forward_round(values: [f32; DIMENSION], round: &TransformRound) -> [f32; DIMENSION] {
    let signed: [f32; DIMENSION] = array::from_fn(|index| values[index] * round.signs[index]);
    let permuted = array::from_fn(|index| signed[round.permutation[index]]);
    apply_hadamard_blocks(permuted)
}

fn apply_inverse_round(values: [f32; DIMENSION], round: &TransformRound) -> [f32; DIMENSION] {
    let after_hadamard = apply_hadamard_blocks(values);
    let unpermuted: [f32; DIMENSION] =
        array::from_fn(|index| after_hadamard[round.inverse_permutation[index]]);
    array::from_fn(|index| unpermuted[index] * round.signs[index])
}

fn apply_hadamard_blocks(values: [f32; DIMENSION]) -> [f32; DIMENSION] {
    let mut transformed = [0.0; DIMENSION];

    for block in 0..BLOCKS_PER_ROUND {
        let start = block * HADAMARD_BLOCK_LEN;
        let end = start + HADAMARD_BLOCK_LEN;
        let mut input = [0.0; HADAMARD_BLOCK_LEN];
        input.copy_from_slice(&values[start..end]);
        let output = hadamard_128(&input);
        transformed[start..end].copy_from_slice(&output);
    }

    transformed
}

#[cfg(test)]
pub(crate) fn transform_f64_reference(
    plan: &TransformPlan,
    mut values: [f64; DIMENSION],
) -> [f64; DIMENSION] {
    for round in &plan.rounds {
        let signed: [f64; DIMENSION] =
            array::from_fn(|index| values[index] * f64::from(round.signs[index]));
        let permuted = array::from_fn(|index| signed[round.permutation[index]]);
        values = apply_hadamard_blocks_f64(permuted);
    }
    values
}

#[cfg(test)]
fn apply_hadamard_blocks_f64(values: [f64; DIMENSION]) -> [f64; DIMENSION] {
    let mut transformed = [0.0; DIMENSION];

    for block in 0..BLOCKS_PER_ROUND {
        let start = block * HADAMARD_BLOCK_LEN;
        let end = start + HADAMARD_BLOCK_LEN;
        let mut output = [0.0_f64; HADAMARD_BLOCK_LEN];
        output.copy_from_slice(&values[start..end]);
        let mut half_width = 1;
        while half_width < HADAMARD_BLOCK_LEN {
            let full_width = half_width * 2;
            for group_start in (0..HADAMARD_BLOCK_LEN).step_by(full_width) {
                for offset in 0..half_width {
                    let left_index = group_start + offset;
                    let right_index = left_index + half_width;
                    let left = output[left_index];
                    let right = output[right_index];
                    output[left_index] = left + right;
                    output[right_index] = left - right;
                }
            }
            half_width = full_width;
        }
        let normalization = 1.0 / (HADAMARD_BLOCK_LEN as f64).sqrt();
        for value in &mut output {
            *value *= normalization;
        }
        transformed[start..end].copy_from_slice(&output);
    }

    transformed
}

#[cfg(test)]
mod tests {
    use core::array;

    use spherra_domain::{DIMENSION, ValidatedVector};

    use crate::scorer::transform_normalized_fp64;

    use super::{TransformPlan, apply_forward_round};

    #[test]
    fn fp64_normalized_kernel_input_is_not_normalized_a_second_time() {
        let mut state = 0x1234_5678_u64;
        let (normalized, direct_kernel_input) = (0..128)
            .find_map(|seed| {
                let raw: [f64; DIMENSION] = array::from_fn(|coordinate| {
                    state = state
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407);
                    let fraction = ((state >> 40) as u32) as f64 / (1_u32 << 24) as f64;
                    let exponent = ((coordinate * 37 + seed * 19) % 151) as i32 - 75;
                    fraction.mul_add(2.0, -1.0) * 2.0_f64.powi(exponent)
                });
                let norm = raw
                    .iter()
                    .fold(0.0, |sum, value| value.mul_add(*value, sum))
                    .sqrt();
                let normalized = array::from_fn(|coordinate| raw[coordinate] / norm);
                let direct = array::from_fn(|coordinate| normalized[coordinate] as f32);
                let validated = ValidatedVector::new(direct.to_vec())
                    .expect("the finite rounded fixture validates");
                let revalidated = validated
                    .normalized_direction()
                    .expect("the rounded fixture remains direction-reliable");
                (direct != *revalidated.as_array()).then_some((normalized, direct))
            })
            .expect("the deterministic fixture search finds a rounded kernel input with drift");

        let plan = TransformPlan::from_seed(0x6b_65_72_6e_65_6c);
        let mut direct = direct_kernel_input;
        for round in &plan.rounds {
            direct = apply_forward_round(direct, round);
        }
        let actual = transform_normalized_fp64(&plan, &normalized)
            .expect("the FP64-normalized fixture can enter the kernel");

        assert_eq!(
            actual, direct,
            "the scorer kernel path must not perform an unmodeled second normalization"
        );
    }
}
