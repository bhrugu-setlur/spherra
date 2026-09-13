use crate::{Error, RowId};
use spherra_codec::{CertificateTerms, ScoreKind, ValidatedTerms, validate_certificate_terms};
/// Read-only certificate metadata from an opened segment. These terms cannot be
/// supplied back to the library to mint a hit or authenticate another index.
#[derive(Clone, Debug)]
pub struct Certificate {
    terms: CertificateTerms,
}
impl Certificate {
    pub fn max_reconstruction_l2_error(&self) -> f64 {
        self.terms.max_reconstruction_l2_error
    }
    pub fn eta_transform_dot(&self) -> f64 {
        self.terms.eta_transform_dot
    }
    pub fn query_norm_upper(&self) -> f64 {
        self.terms.query_norm_upper
    }
    pub fn eta_serving_score(&self) -> f64 {
        self.terms.eta_serving_score
    }
    pub fn epsilon(&self) -> f64 {
        self.terms.epsilon
    }
}
/// Certificates for one verified segment of the opened generation.
/// ```compile_fail
/// let certificates = spherra::SegmentCertificates {};
/// ```
#[derive(Clone, Debug)]
pub struct SegmentCertificates {
    first_row: RowId,
    row_count: u32,
    primary: Certificate,
    refined: Certificate,
}
impl SegmentCertificates {
    pub fn first_row(&self) -> RowId {
        self.first_row
    }
    pub fn row_count(&self) -> u32 {
        self.row_count
    }
    pub fn primary(&self) -> &Certificate {
        &self.primary
    }
    pub fn refined(&self) -> &Certificate {
        &self.refined
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    pub generation: u64,
    pub segment: u32,
    pub id: [u8; 16],
    pub first_row: u64,
    pub row_count: u32,
    pub model_hash: [u8; 32],
}
struct BoundCertificate {
    terms: ValidatedTerms,
    binding: Binding,
    kind: ScoreKind,
}
pub(crate) struct BoundPair {
    primary: BoundCertificate,
    refined: BoundCertificate,
    metadata: SegmentCertificates,
}
impl BoundPair {
    /// Called only by open after file identities, pairing and every row range
    /// have passed. Arithmetic validity alone is insufficient to call this.
    pub fn bind(
        binding: Binding,
        primary: CertificateTerms,
        refined: CertificateTerms,
    ) -> Result<Self, Error> {
        Ok(Self {
            primary: BoundCertificate {
                terms: validate_certificate_terms(primary, ScoreKind::Primary)?,
                binding,
                kind: ScoreKind::Primary,
            },
            refined: BoundCertificate {
                terms: validate_certificate_terms(refined, ScoreKind::Refined)?,
                binding,
                kind: ScoreKind::Refined,
            },
            metadata: SegmentCertificates {
                first_row: RowId(binding.first_row),
                row_count: binding.row_count,
                primary: Certificate { terms: primary },
                refined: Certificate { terms: refined },
            },
        })
    }
    pub fn metadata(&self) -> SegmentCertificates {
        self.metadata.clone()
    }
    pub fn interval(
        &self,
        binding: Binding,
        row: u64,
        primary: i64,
        refined: i64,
    ) -> Result<(f64, f64), Error> {
        if self.primary.binding != binding
            || self.refined.binding != binding
            || self.primary.kind != ScoreKind::Primary
            || self.refined.kind != ScoreKind::Refined
            || row < binding.first_row
            || row - binding.first_row >= u64::from(binding.row_count)
        {
            return Err(Error::CertificateInvalid);
        }
        let interval = self
            .primary
            .terms
            .arithmetic_interval(primary)
            .intersection(self.refined.terms.arithmetic_interval(refined))?;
        Ok((interval.lower(), interval.upper()))
    }
}
