use core::fmt;

/// A malformed canonical model table. Indices refer to its flattened values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RestoreError {
    Length { expected: usize, actual: usize },
    NonFinite { index: usize },
    NegativeZero { index: usize },
    DecreasingCenters { coordinate: usize, code: usize },
}

impl fmt::Display for RestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length { expected, actual } => write!(
                f,
                "model table requires {expected} values, received {actual}"
            ),
            Self::NonFinite { index } => write!(f, "model table value {index} is not finite"),
            Self::NegativeZero { index } => write!(f, "model table value {index} is negative zero"),
            Self::DecreasingCenters { coordinate, code } => write!(
                f,
                "quantizer centers decrease at coordinate {coordinate}, code {code}"
            ),
        }
    }
}

impl std::error::Error for RestoreError {}

pub(crate) fn validate_values(values: &[f32], expected: usize) -> Result<(), RestoreError> {
    if values.len() != expected {
        return Err(RestoreError::Length {
            expected,
            actual: values.len(),
        });
    }
    for (index, value) in values.iter().enumerate() {
        if !value.is_finite() {
            return Err(RestoreError::NonFinite { index });
        }
        // Reject rather than normalize: accepted stored bits must round-trip exactly.
        if value.to_bits() == (-0.0_f32).to_bits() {
            return Err(RestoreError::NegativeZero { index });
        }
    }
    Ok(())
}
