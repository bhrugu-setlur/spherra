use core::{array, fmt};

use spherra_domain::DIMENSION;

use crate::transform::TransformedDirection;

const CENTERS_PER_COORDINATE: usize = 16;
const DIRECT_CODE_BYTE_LEN: usize = DIMENSION / 2;
const QUANTIZER_TABLE_LEN: usize = DIMENSION * CENTERS_PER_COORDINATE;
const QUANTIZER_ID_LEN: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectCode([u8; DIRECT_CODE_BYTE_LEN]);

impl DirectCode {
    pub const BYTE_LEN: usize = DIRECT_CODE_BYTE_LEN;

    pub fn from_nibbles(nibbles: [u8; DIMENSION]) -> Result<Self, DirectCodeError> {
        if let Some((coordinate, value)) = nibbles
            .iter()
            .copied()
            .enumerate()
            .find(|(_, value)| *value > 0x0f)
        {
            return Err(DirectCodeError { coordinate, value });
        }

        Ok(Self::from_valid_nibbles(nibbles))
    }

    pub const fn as_bytes(&self) -> &[u8; DIRECT_CODE_BYTE_LEN] {
        &self.0
    }

    pub fn to_nibbles(&self) -> [u8; DIMENSION] {
        array::from_fn(|coordinate| self.nibble_at(coordinate))
    }

    pub fn nibble_at(&self, coordinate: usize) -> u8 {
        let byte = self.0[coordinate / 2];
        let shift = if coordinate.is_multiple_of(2) { 0 } else { 4 };
        (byte >> shift) & 0x0f
    }

    pub(crate) fn from_valid_nibbles(nibbles: [u8; DIMENSION]) -> Self {
        let mut bytes = [0; DIRECT_CODE_BYTE_LEN];

        for (coordinate, nibble) in nibbles.into_iter().enumerate() {
            let shift = if coordinate.is_multiple_of(2) { 0 } else { 4 };
            bytes[coordinate / 2] |= nibble << shift;
        }

        Self(bytes)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectCodeError {
    coordinate: usize,
    value: u8,
}

impl fmt::Display for DirectCodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "direct-int4 code at coordinate {} exceeds 15: {}",
            self.coordinate, self.value
        )
    }
}

impl std::error::Error for DirectCodeError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RadiusFlags([u8; Self::BYTE_LEN]);

impl RadiusFlags {
    pub const BYTE_LEN: usize = 4;

    pub const fn from_bytes(bytes: [u8; Self::BYTE_LEN]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; Self::BYTE_LEN] {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct QuantizerTable {
    centers: [f32; QUANTIZER_TABLE_LEN],
    identity: [u8; QUANTIZER_ID_LEN],
}

impl QuantizerTable {
    pub fn train(calibration: &[[f32; DIMENSION]]) -> Result<Self, TrainError> {
        if calibration.is_empty() {
            return Err(TrainError::EmptyCalibration);
        }

        for (row, values) in calibration.iter().enumerate() {
            if let Some(coordinate) = values.iter().position(|value| !value.is_finite()) {
                return Err(TrainError::NonFiniteValue { row, coordinate });
            }
        }

        let mut centers = [0.0; QUANTIZER_TABLE_LEN];
        let mut coordinate_values = Vec::with_capacity(calibration.len());
        for coordinate in 0..DIMENSION {
            coordinate_values.clear();
            coordinate_values.extend(
                calibration
                    .iter()
                    .map(|values| canonicalize_zero(values[coordinate])),
            );
            coordinate_values.sort_by(f32::total_cmp);

            for center in 0..CENTERS_PER_COORDINATE {
                centers[center_index(coordinate, center)] =
                    coordinate_values[quantile_rank(center, calibration.len())];
            }
        }

        let identity = derive_identity(&centers);
        Ok(Self { centers, identity })
    }

    pub const fn centers(&self) -> &[f32; QUANTIZER_TABLE_LEN] {
        &self.centers
    }

    pub const fn identity(&self) -> &[u8; QUANTIZER_ID_LEN] {
        &self.identity
    }

    /// Returns a center only when both the coordinate and four-bit code are in range.
    pub fn center(&self, coordinate: usize, code: usize) -> Option<f32> {
        if coordinate >= DIMENSION || code >= CENTERS_PER_COORDINATE {
            return None;
        }

        Some(self.center_for_valid_indices(coordinate, code))
    }

    pub fn encode(&self, values: &TransformedDirection) -> DirectCode {
        let nibbles = array::from_fn(|coordinate| {
            self.nearest_code(coordinate, values.as_array()[coordinate])
        });
        DirectCode::from_valid_nibbles(nibbles)
    }

    pub fn decode(&self, code: &DirectCode) -> [f32; DIMENSION] {
        array::from_fn(|coordinate| {
            self.center_for_valid_indices(coordinate, code.nibble_at(coordinate) as usize)
        })
    }

    pub fn score(&self, query: &TransformedDirection, code: &DirectCode) -> f32 {
        query
            .as_array()
            .iter()
            .enumerate()
            .fold(0.0, |score, (coordinate, value)| {
                score
                    + value
                        * self.center_for_valid_indices(
                            coordinate,
                            code.nibble_at(coordinate) as usize,
                        )
            })
    }

    fn nearest_code(&self, coordinate: usize, value: f32) -> u8 {
        let mut selected = 0;
        let mut smallest_distance =
            (value - self.center_for_valid_indices(coordinate, selected)).abs();

        for code in 1..CENTERS_PER_COORDINATE {
            let distance = (value - self.center_for_valid_indices(coordinate, code)).abs();
            if distance < smallest_distance {
                selected = code;
                smallest_distance = distance;
            }
        }

        selected as u8
    }

    fn center_for_valid_indices(&self, coordinate: usize, code: usize) -> f32 {
        self.centers[center_index(coordinate, code)]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrainError {
    EmptyCalibration,
    NonFiniteValue { row: usize, coordinate: usize },
}

impl fmt::Display for TrainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCalibration => formatter.write_str("direct-int4 calibration is empty"),
            Self::NonFiniteValue { row, coordinate } => write!(
                formatter,
                "direct-int4 calibration row {row}, coordinate {coordinate} is not finite"
            ),
        }
    }
}

impl std::error::Error for TrainError {}

fn canonicalize_zero(value: f32) -> f32 {
    if value == 0.0 { 0.0 } else { value }
}

fn quantile_rank(center: usize, sample_count: usize) -> usize {
    let numerator = (center as u128) * ((sample_count - 1) as u128);
    usize::try_from(numerator / (CENTERS_PER_COORDINATE - 1) as u128)
        .expect("a quantile rank is bounded by the usize calibration length")
}

fn center_index(coordinate: usize, center: usize) -> usize {
    coordinate * CENTERS_PER_COORDINATE + center
}

fn derive_identity(centers: &[f32; QUANTIZER_TABLE_LEN]) -> [u8; QUANTIZER_ID_LEN] {
    let mut hasher = blake3::Hasher::new();
    for center in centers {
        hasher.update(&center.to_le_bytes());
    }
    *hasher.finalize().as_bytes()
}
