use crate::container::{ContainerError, Decoder, pack, unpack};
use std::collections::HashSet;
pub(crate) const MAX_SEGMENTS: usize = 4096;
pub(crate) const MAX_SEGMENT_ROWS: u32 = 65536;
pub(crate) const MAX_ROWS: u64 = (1_u64 << 48) - 1;
const ENTRY_LEN: usize = 108;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SegmentEntry {
    pub id: [u8; 16],
    pub first_row: u64,
    pub row_count: u32,
    pub primary_len: u64,
    pub residual_len: u64,
    pub primary_hash: [u8; 32],
    pub residual_hash: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Manifest {
    pub index_id: [u8; 16],
    pub generation: u64,
    pub previous: [u8; 32],
    pub model_hash: [u8; 32],
    pub total_rows: u64,
    pub segments: Vec<SegmentEntry>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ManifestError {
    Generation,
    DuplicateSegment,
    FirstRow,
    Gap,
    Overlap,
    RowSum,
    SegmentCount,
    RowCount,
    TotalRows,
    RowOverflow,
}
impl Manifest {
    pub const MAX_BYTE_LEN: usize = 100 + MAX_SEGMENTS * ENTRY_LEN + 50;
    pub fn validate(&self, current_generation: u64) -> Result<(), ManifestError> {
        if self.generation == 0 || self.generation != current_generation {
            return Err(ManifestError::Generation);
        }
        if self.segments.is_empty() || self.segments.len() > MAX_SEGMENTS {
            return Err(ManifestError::SegmentCount);
        }
        if self.total_rows > MAX_ROWS {
            return Err(ManifestError::TotalRows);
        }
        let mut ids = HashSet::new();
        let mut next = 0_u64;
        for (i, s) in self.segments.iter().enumerate() {
            if !ids.insert(s.id) {
                return Err(ManifestError::DuplicateSegment);
            }
            if s.row_count == 0 || s.row_count > MAX_SEGMENT_ROWS {
                return Err(ManifestError::RowCount);
            }
            let end = s
                .first_row
                .checked_add(u64::from(s.row_count))
                .ok_or(ManifestError::RowOverflow)?;
            if i == 0 && s.first_row != 0 {
                return Err(ManifestError::FirstRow);
            }
            if s.first_row < next {
                return Err(ManifestError::Overlap);
            }
            if s.first_row > next {
                return Err(ManifestError::Gap);
            }
            next = end;
        }
        if next != self.total_rows {
            return Err(ManifestError::RowSum);
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>, ContainerError> {
        if self.segments.len() > MAX_SEGMENTS {
            return Err(ContainerError::SegmentCount);
        }
        let mut p = Vec::with_capacity(100 + self.segments.len() * ENTRY_LEN);
        p.extend_from_slice(&self.index_id);
        p.extend_from_slice(&self.generation.to_le_bytes());
        p.extend_from_slice(&self.previous);
        p.extend_from_slice(&self.model_hash);
        p.extend_from_slice(&self.total_rows.to_le_bytes());
        p.extend_from_slice(&(self.segments.len() as u32).to_le_bytes());
        for s in &self.segments {
            p.extend_from_slice(&s.id);
            p.extend_from_slice(&s.first_row.to_le_bytes());
            p.extend_from_slice(&s.row_count.to_le_bytes());
            p.extend_from_slice(&s.primary_len.to_le_bytes());
            p.extend_from_slice(&s.residual_len.to_le_bytes());
            p.extend_from_slice(&s.primary_hash);
            p.extend_from_slice(&s.residual_hash);
        }
        Ok(pack(*b"SPHRMAN1", &p))
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, ContainerError> {
        let p = unpack(bytes, b"SPHRMAN1")?;
        let mut d = Decoder::new(p);
        let index_id = d.take()?;
        let generation = d.u64()?;
        let previous = d.take()?;
        let model_hash = d.take()?;
        let total_rows = d.u64()?;
        let count = d.u32()? as usize;
        if count > MAX_SEGMENTS {
            return Err(ContainerError::SegmentCount);
        }
        if p.len() != 100 + count * ENTRY_LEN {
            return Err(ContainerError::Length);
        }
        let mut segments = Vec::with_capacity(count);
        for _ in 0..count {
            segments.push(SegmentEntry {
                id: d.take()?,
                first_row: d.u64()?,
                row_count: d.u32()?,
                primary_len: d.u64()?,
                residual_len: d.u64()?,
                primary_hash: d.take()?,
                residual_hash: d.take()?,
            })
        }
        d.finish()?;
        Ok(Self {
            index_id,
            generation,
            previous,
            model_hash,
            total_rows,
            segments,
        })
    }
}
