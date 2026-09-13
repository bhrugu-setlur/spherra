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

pub(crate) struct Model {
    pub file: ModelFile,
    pub plan: Box<spherra_codec::TransformPlan>,
    pub quantizer: Box<QuantizerTable>,
    pub codebook: Pq96Codebook,
}
impl Model {
    pub fn restore(file: ModelFile) -> Result<Self, crate::Error> {
        use spherra_codec::{CODEC_ID, FixedPointScorer, GENERATOR_VERSION, TransformPlan};
        if file.generator_version != GENERATOR_VERSION
            || file.codec_id != *CODEC_ID
            || file.scorer_version != FixedPointScorer::new().metadata().scorer_version()
            || file.layout != 1
        {
            return Err(crate::Error::Unsupported);
        }
        let plan = Box::new(TransformPlan::from_seed(file.seed));
        let quantizer =
            QuantizerTable::from_centers(&file.centers).map_err(|_| crate::Error::Corrupt)?;
        let codebook =
            Pq96Codebook::from_centroids(&file.centroids).map_err(|_| crate::Error::Corrupt)?;
        if plan.expanded_digest() != file.expanded_digest
            || *plan.identity() != file.transform_id
            || *quantizer.identity() != file.quantizer_id
            || *codebook.codebook_id() != file.codebook_id
        {
            return Err(crate::Error::IdentityMismatch);
        }
        Ok(Self {
            file,
            plan,
            quantizer: Box::new(quantizer),
            codebook,
        })
    }
    pub fn train(
        rows: &[crate::Vector],
        seed: u64,
        validation_rows: usize,
    ) -> Result<Self, crate::Error> {
        use spherra_codec::{
            CODEC_ID, FixedPointScorer, GENERATOR_VERSION, TransformPlan, transform,
        };
        let mut order: Vec<_> = (0..rows.len()).collect();
        // Seeded hash order defines a reproducible disjoint split without retaining copies of originals.
        order.sort_by_cached_key(|r| {
            let mut b = [0; 16];
            b[..8].copy_from_slice(&seed.to_le_bytes());
            b[8..].copy_from_slice(&(*r as u64).to_le_bytes());
            *blake3::hash(&b).as_bytes()
        });
        let plan = Box::new(TransformPlan::from_seed(seed));
        let transformed: Vec<_> = order[validation_rows..]
            .iter()
            .map(|&r| {
                let validated = crate::builder::validate(&rows[r], r as u64)?;
                Ok(transform(
                    &plan,
                    validated
                        .normalized_direction()
                        .expect("validated reliable"),
                ))
            })
            .collect::<Result<_, crate::Error>>()?;
        let quantizer = QuantizerTable::train(
            &transformed
                .iter()
                .map(|v| *v.as_array())
                .collect::<Vec<_>>(),
        )
        .map_err(|_| crate::Error::InvalidTraining)?;
        let residuals: Vec<_> = transformed
            .iter()
            .map(|v| {
                let decoded = quantizer.decode(&quantizer.encode(v));
                std::array::from_fn(|c| v.as_array()[c] - decoded[c])
            })
            .collect();
        let codebook =
            Pq96Codebook::train(&residuals, seed).map_err(|_| crate::Error::InvalidTraining)?;
        drop(residuals);
        drop(transformed);
        let centroids = codebook
            .canonical_bytes()
            .chunks_exact(4)
            .map(|v| f32::from_le_bytes(v.try_into().expect("four bytes")))
            .collect();
        let file = ModelFile {
            generator_version: GENERATOR_VERSION,
            seed,
            expanded_digest: plan.expanded_digest(),
            transform_id: *plan.identity(),
            quantizer_id: *quantizer.identity(),
            codebook_id: *codebook.codebook_id(),
            codec_id: *CODEC_ID,
            scorer_version: FixedPointScorer::new().metadata().scorer_version(),
            layout: 1,
            centers: quantizer.centers().to_vec(),
            centroids,
            baseline: DriftBaseline {
                primary: [0.0; 3],
                refined: [0.0; 3],
                outside_fraction: 0.0,
            },
        };
        let mut model = Self {
            file,
            plan,
            quantizer: Box::new(quantizer),
            codebook,
        };
        let samples = order[..validation_rows]
            .iter()
            .map(|&r| model.encode(&rows[r], r as u64).map(|e| e.stats))
            .collect::<Result<Vec<_>, _>>()?;
        model.file.baseline = crate::drift::summarize(&samples).baseline();
        Ok(model)
    }
    pub fn expectations(&self) -> spherra_format::SegmentExpectations {
        spherra_format::SegmentExpectations {
            codec_id: self.file.codec_id,
            scorer_version: self.file.scorer_version,
            transform_id: self.file.transform_id,
            quantizer_id: self.file.quantizer_id,
            pq_codebook_id: self.file.codebook_id,
            layout: spherra_format::LayoutId::TiledSoa32,
        }
    }
    pub fn encode(&self, row: &crate::Vector, position: u64) -> Result<EncodedRow, crate::Error> {
        let validated = crate::builder::validate(row, position)?;
        let transformed = spherra_codec::transform(
            &self.plan,
            validated
                .normalized_direction()
                .expect("validated reliable"),
        );
        let primary = self.quantizer.encode(&transformed);
        let p = self.quantizer.decode(&primary);
        let residual = self
            .codebook
            .encode(&std::array::from_fn(|c| transformed.as_array()[c] - p[c]))
            .map_err(|_| crate::Error::Corrupt)?;
        let e = self.codebook.decode(&residual);
        let mut primary_error = 0.0;
        let mut refined_error = 0.0;
        let mut outside = 0;
        for c in 0..768 {
            let t = f64::from(transformed.as_array()[c]);
            let d = t - f64::from(p[c]);
            primary_error += d * d;
            let d = d - f64::from(e[c]);
            refined_error += d * d;
            outside += usize::from(
                transformed.as_array()[c] < self.file.centers[c * 16]
                    || transformed.as_array()[c] > self.file.centers[c * 16 + 15],
            );
        }
        Ok(EncodedRow {
            primary,
            residual,
            radius: validated.radius_f16_bits(),
            stats: [
                primary_error.sqrt() as f32,
                refined_error.sqrt() as f32,
                outside as f32 / 768.0,
            ],
        })
    }
}
pub(crate) struct EncodedRow {
    pub primary: spherra_codec::DirectCode,
    pub residual: spherra_codec::Pq96Code,
    pub radius: u16,
    pub stats: [f32; 3],
}
