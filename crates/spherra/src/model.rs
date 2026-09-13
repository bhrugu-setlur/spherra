use crate::container::{ContainerError, Decoder, pack, unpack};
use spherra_codec::{Pq96Codebook, QuantizerTable};
pub(crate) const CENTER_COUNT: usize = 768 * 16;
pub(crate) const CENTROID_COUNT: usize = 96 * 256 * 8;
const PAYLOAD_LEN: usize = 178 + (CENTER_COUNT + CENTROID_COUNT) * 4 + 56;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DriftBaseline {
    pub primary: [f64; 3],
    pub refined: [f64; 3],
    pub outside_fraction: f64,
}
impl DriftBaseline {
    pub fn valid(&self) -> bool {
        [self.primary, self.refined].iter().all(|p| {
            p.iter()
                .all(|v| v.is_finite() && *v >= 0.0 && v.to_bits() != (-0.0_f64).to_bits())
                && p.windows(2).all(|w| w[0] <= w[1])
        }) && self.outside_fraction.is_finite()
            && self.outside_fraction.to_bits() != (-0.0_f64).to_bits()
            && (0.0..=1.0).contains(&self.outside_fraction)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ModelFile {
    pub generator_version: u16,
    pub seed: u64,
    pub expanded_digest: [u8; 32],
    pub transform_id: [u8; 32],
    pub quantizer_id: [u8; 32],
    pub codebook_id: [u8; 32],
    pub codec_id: [u8; 32],
    pub scorer_version: u32,
    pub layout: u32,
    pub centers: Vec<f32>,
    pub centroids: Vec<f32>,
    pub baseline: DriftBaseline,
}
impl ModelFile {
    pub const BYTE_LEN: usize = PAYLOAD_LEN + 50;
    fn validate_values(&self) -> Result<(), ContainerError> {
        if self.centers.len() != CENTER_COUNT || self.centroids.len() != CENTROID_COUNT {
            return Err(ContainerError::Length);
        }
        if !self.baseline.valid() {
            return Err(ContainerError::ModelValues);
        }
        QuantizerTable::from_centers(&self.centers).map_err(|_| ContainerError::ModelValues)?;
        Pq96Codebook::from_centroids(&self.centroids).map_err(|_| ContainerError::ModelValues)?;
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>, ContainerError> {
        self.validate_values()?;
        let mut p = Vec::with_capacity(PAYLOAD_LEN);
        p.extend_from_slice(&self.generator_version.to_le_bytes());
        p.extend_from_slice(&self.seed.to_le_bytes());
        for id in [
            &self.expanded_digest,
            &self.transform_id,
            &self.quantizer_id,
            &self.codebook_id,
            &self.codec_id,
        ] {
            p.extend_from_slice(id)
        }
        p.extend_from_slice(&self.scorer_version.to_le_bytes());
        p.extend_from_slice(&self.layout.to_le_bytes());
        for v in self.centers.iter().chain(&self.centroids) {
            p.extend_from_slice(&v.to_le_bytes())
        }
        for v in self
            .baseline
            .primary
            .iter()
            .chain(&self.baseline.refined)
            .chain(std::iter::once(&self.baseline.outside_fraction))
        {
            p.extend_from_slice(&v.to_le_bytes())
        }
        Ok(pack(*b"SPHRMOD1", &p))
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, ContainerError> {
        let payload = unpack(bytes, b"SPHRMOD1")?;
        if payload.len() != PAYLOAD_LEN {
            return Err(ContainerError::Length);
        }
        let mut d = Decoder::new(payload);
        let generator_version = d.u16()?;
        let seed = d.u64()?;
        let expanded_digest = d.take()?;
        let transform_id = d.take()?;
        let quantizer_id = d.take()?;
        let codebook_id = d.take()?;
        let codec_id = d.take()?;
        let scorer_version = d.u32()?;
        let layout = d.u32()?;
        // The exact fixed payload length was checked before either allocation.
        let centers = (0..CENTER_COUNT)
            .map(|_| d.f32())
            .collect::<Result<Vec<_>, _>>()?;
        let centroids = (0..CENTROID_COUNT)
            .map(|_| d.f32())
            .collect::<Result<Vec<_>, _>>()?;
        let primary = [d.f64()?, d.f64()?, d.f64()?];
        let refined = [d.f64()?, d.f64()?, d.f64()?];
        let outside_fraction = d.f64()?;
        d.finish()?;
        let model = Self {
            generator_version,
            seed,
            expanded_digest,
            transform_id,
            quantizer_id,
            codebook_id,
            codec_id,
            scorer_version,
            layout,
            centers,
            centroids,
            baseline: DriftBaseline {
                primary,
                refined,
                outside_fraction,
            },
        };
        model.validate_values()?;
        Ok(model)
    }
}
