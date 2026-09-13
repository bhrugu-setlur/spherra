use half::f16;

use crate::DomainError;

pub const DIMENSION: usize = 768;
pub const DEFAULT_MIN_NORM_EPSILON: f64 = 1.0e-12;

#[derive(Clone, Debug, PartialEq)]
pub struct ReliableDirection([f32; DIMENSION]);

impl ReliableDirection {
    pub const fn as_array(&self) -> &[f32; DIMENSION] {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedVector {
    radius: f16,
    normalized_direction: Option<ReliableDirection>,
}

impl ValidatedVector {
    pub fn new(components: Vec<f32>) -> Result<Self, DomainError> {
        Self::new_with_min_norm_epsilon(components, DEFAULT_MIN_NORM_EPSILON)
    }

    pub fn new_with_min_norm_epsilon(
        components: Vec<f32>,
        min_norm_epsilon: f64,
    ) -> Result<Self, DomainError> {
        if !min_norm_epsilon.is_finite() || min_norm_epsilon <= 0.0 {
            return Err(DomainError::InvalidMinNormEpsilon {
                value: min_norm_epsilon,
            });
        }

        let actual = components.len();
        let components: [f32; DIMENSION] =
            components
                .try_into()
                .map_err(|_| DomainError::DimensionMismatch {
                    expected: DIMENSION,
                    actual,
                })?;

        if let Some(index) = components
            .iter()
            .position(|component| !component.is_finite())
        {
            return Err(DomainError::NonFiniteComponent { index });
        }

        let squared_norm = components.iter().fold(0.0_f64, |sum, component| {
            let component = f64::from(*component);
            component.mul_add(component, sum)
        });
        let norm = squared_norm.sqrt();
        let max_f16 = f16::MAX.to_f32();
        if norm > f64::from(max_f16) {
            return Err(DomainError::NormExceedsFp16 { norm, max: max_f16 });
        }

        let radius = f16::from_f64(norm);
        let normalized_direction = if norm < min_norm_epsilon {
            None
        } else {
            let mut normalized = [0.0_f32; DIMENSION];
            for (output, component) in normalized.iter_mut().zip(components) {
                *output = (f64::from(component) / norm) as f32;
            }
            Some(ReliableDirection(normalized))
        };

        Ok(Self {
            radius,
            normalized_direction,
        })
    }

    pub fn direction_unreliable(&self) -> bool {
        self.normalized_direction.is_none()
    }

    pub fn normalized_direction(&self) -> Option<&ReliableDirection> {
        self.normalized_direction.as_ref()
    }

    pub fn radius_f32(&self) -> f32 {
        self.radius.to_f32()
    }

    /// The already-rounded stored FP16 radius, for explicit durable encoding.
    pub fn radius_f16_bits(&self) -> u16 {
        self.radius.to_bits()
    }
}
