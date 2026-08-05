use core::array;

use spherra_domain::DIMENSION;

use crate::int4::{DirectCode, QuantizerTable};
use crate::transform::TransformedDirection;

const TILE_ROWS: usize = 32;
const BYTES_PER_COORDINATE: usize = TILE_ROWS / 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TiledSoa32 {
    row_count: usize,
    bytes: Vec<u8>,
}

impl TiledSoa32 {
    pub const fn direction_bytes_for_full_tile() -> usize {
        DIMENSION * BYTES_PER_COORDINATE
    }

    pub fn from_codes(codes: &[DirectCode]) -> Self {
        let tile_count = codes.len().div_ceil(TILE_ROWS);
        let tile_bytes = Self::direction_bytes_for_full_tile();
        let mut bytes = vec![0; tile_count * tile_bytes];

        for (row, code) in codes.iter().enumerate() {
            let tile = row / TILE_ROWS;
            let lane = row % TILE_ROWS;
            for coordinate in 0..DIMENSION {
                let offset = tile * tile_bytes + coordinate * BYTES_PER_COORDINATE + lane / 2;
                let shift = if lane.is_multiple_of(2) { 0 } else { 4 };
                bytes[offset] |= code.nibble_at(coordinate) << shift;
            }
        }

        Self {
            row_count: codes.len(),
            bytes,
        }
    }

    pub const fn row_count(&self) -> usize {
        self.row_count
    }

    pub fn logical_direction_bytes(&self) -> usize {
        self.row_count * DirectCode::BYTE_LEN
    }

    pub fn physical_direction_bytes(&self) -> usize {
        self.bytes.len()
    }

    pub fn padding_direction_bytes(&self) -> usize {
        self.physical_direction_bytes() - self.logical_direction_bytes()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn code_at(&self, row: usize) -> Option<DirectCode> {
        (row < self.row_count).then(|| {
            DirectCode::from_valid_nibbles(array::from_fn(|coordinate| {
                self.nibble_at(row, coordinate)
            }))
        })
    }

    pub fn scan_scores(&self, table: &QuantizerTable, query: &TransformedDirection) -> Vec<f32> {
        let mut scores = vec![0.0; self.row_count];

        for tile in 0..self.tile_count() {
            let first_row = tile * TILE_ROWS;
            let last_row = (first_row + TILE_ROWS).min(self.row_count);
            for (coordinate, query_value) in query.as_array().iter().enumerate() {
                for (row, score) in scores.iter_mut().enumerate().take(last_row).skip(first_row) {
                    *score += query_value
                        * table
                            .center(coordinate, self.nibble_at(row, coordinate) as usize)
                            .expect("tiled scan supplies valid coordinates and four-bit codes");
                }
            }
        }

        scores
    }

    fn tile_count(&self) -> usize {
        self.row_count.div_ceil(TILE_ROWS)
    }

    fn nibble_at(&self, row: usize, coordinate: usize) -> u8 {
        let tile = row / TILE_ROWS;
        let lane = row % TILE_ROWS;
        let offset = tile * Self::direction_bytes_for_full_tile()
            + coordinate * BYTES_PER_COORDINATE
            + lane / 2;
        let shift = if lane.is_multiple_of(2) { 0 } else { 4 };
        (self.bytes[offset] >> shift) & 0x0f
    }
}
