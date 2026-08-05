use spherra_domain::DIMENSION;
use spherra_simd::scalar::HADAMARD_BLOCK_LEN;

const TRANSFORM_ROUNDS: usize = 2;
const BLOCKS_PER_ROUND: usize = DIMENSION / HADAMARD_BLOCK_LEN;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransformSpec {
    dimension: usize,
    rounds: usize,
    blocks_per_round: usize,
    block_len: usize,
}

impl TransformSpec {
    pub const fn current() -> Self {
        Self {
            dimension: DIMENSION,
            rounds: TRANSFORM_ROUNDS,
            blocks_per_round: BLOCKS_PER_ROUND,
            block_len: HADAMARD_BLOCK_LEN,
        }
    }

    pub const fn dimension(self) -> usize {
        self.dimension
    }

    pub const fn rounds(self) -> usize {
        self.rounds
    }

    pub const fn blocks_per_round(self) -> usize {
        self.blocks_per_round
    }

    pub const fn block_len(self) -> usize {
        self.block_len
    }
}
