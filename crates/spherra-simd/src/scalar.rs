pub const HADAMARD_BLOCK_LEN: usize = 128;

pub fn hadamard_128(input: &[f32; HADAMARD_BLOCK_LEN]) -> [f32; HADAMARD_BLOCK_LEN] {
    let mut output = *input;
    let mut half_width = 1;

    while half_width < HADAMARD_BLOCK_LEN {
        let full_width = half_width * 2;
        for start in (0..HADAMARD_BLOCK_LEN).step_by(full_width) {
            for offset in 0..half_width {
                let left_index = start + offset;
                let right_index = left_index + half_width;
                let left = output[left_index];
                let right = output[right_index];
                output[left_index] = left + right;
                output[right_index] = left - right;
            }
        }
        half_width = full_width;
    }

    let normalization = 1.0 / (HADAMARD_BLOCK_LEN as f32).sqrt();
    for value in &mut output {
        *value *= normalization;
    }

    output
}
