use core::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum DomainError {
    DimensionMismatch { expected: usize, actual: usize },
    NonFiniteComponent { index: usize },
    NormExceedsFp16 { norm: f64, max: f32 },
    InvalidMinNormEpsilon { value: f64 },
    SequenceOverflow,
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DimensionMismatch { expected, actual } => {
                write!(
                    formatter,
                    "expected {expected} vector components, received {actual}"
                )
            }
            Self::NonFiniteComponent { index } => {
                write!(formatter, "vector component {index} is not finite")
            }
            Self::NormExceedsFp16 { norm, max } => write!(
                formatter,
                "vector norm {norm} exceeds the largest finite FP16 value {max}"
            ),
            Self::InvalidMinNormEpsilon { value } => write!(
                formatter,
                "minimum norm epsilon must be finite and greater than zero, received {value}"
            ),
            Self::SequenceOverflow => {
                formatter.write_str("sequence index exceeds the 48-bit limit")
            }
        }
    }
}

impl std::error::Error for DomainError {}
