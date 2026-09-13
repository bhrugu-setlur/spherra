use crate::{
    Error, SegmentCertificates,
    builder::terms,
    certified::{Binding, BoundPair},
    error::lock_error,
    fs::RealFs,
    lock::IndexLock,
    manifest::SegmentEntry,
    model::Model,
    search::WorkerPool,
    storage,
};
use spherra_format::{
    PairedResidualReader, PairedSegmentReaders, PrimaryFileReader, ResidualFileReader,
};
use std::{fs::File, path::Path, sync::Arc};
pub(crate) const TILE_BYTES: usize = 768 * 16;
pub(crate) struct Segment {
    pub entry: SegmentEntry,
    pub tiles: Vec<u8>,
    pub tile_start: usize,
    pub residual: PairedResidualReader,
    pub certificate: BoundPair,
}
pub(crate) struct Data {
    pub model: Model,
    pub model_hash: [u8; 32],
    pub generation: u64,
    pub total_rows: u64,
    pub tiles: usize,
    pub segments: Vec<Segment>,
}
impl Data {
    pub fn binding(&self, segment: usize) -> Binding {
        let s = &self.segments[segment];
        Binding {
            generation: self.generation,
            segment: segment as u32,
            id: s.entry.id,
            first_row: s.entry.first_row,
            row_count: s.entry.row_count,
            model_hash: self.model_hash,
        }
    }
}
/// Immutable opened generation. The shared lock and residual descriptors remain
/// owned until the index is dropped. Files must not be modified externally.
pub struct Index {
    pub(crate) data: Arc<Data>,
    pub(crate) pool: WorkerPool,
    _lock: IndexLock,
}
impl Index {
    pub fn open(dir: &Path) -> Result<Self, Error> {
        Self::open_with_workers(dir, 6)
    }
    pub(crate) fn open_with_workers(dir: &Path, workers: usize) -> Result<Self, Error> {
        if workers == 0 {
            return Err(Error::InvalidOptions);
        }
        let lock = IndexLock::acquire(dir, false).map_err(lock_error)?;
        let (current, manifest, model) = storage::load(&RealFs, dir)?;
        let required = manifest.segments.len() as u64 + 65;
        let available = rustix::process::getrlimit(rustix::process::Resource::Nofile)
            .current
            .unwrap_or(u64::MAX);
        if required > available {
            return Err(Error::DescriptorLimit {
                required,
                available,
            });
        }
        let mut loaded = Vec::with_capacity(manifest.segments.len());
        let mut tiles = 0;
        for entry in manifest.segments {
            let primary = File::open(dir.join(format!("{}.primary", storage::hex(&entry.id))))?;
            let residual = File::open(dir.join(format!("{}.residual", storage::hex(&entry.id))))?;
            if primary.metadata()?.len() != entry.primary_len
                || residual.metadata()?.len() != entry.residual_len
            {
                return Err(Error::IdentityMismatch);
            }
            let primary = PrimaryFileReader::open(Box::new(primary), &model.expectations())?;
            let residual = ResidualFileReader::open(Box::new(residual), &model.expectations())?;
            if primary.file_identity() != entry.primary_hash
                || residual.file_identity() != entry.residual_hash
                || primary.identity().collection_id != manifest.index_id
                || residual.identity().collection_id != manifest.index_id
                || primary.identity().segment_id != entry.id
                || residual.identity().segment_id != entry.id
                || primary.row_count() != entry.row_count
                || residual.row_count() != entry.row_count
            {
                return Err(Error::IdentityMismatch);
            }
            let pair = PairedSegmentReaders::open(primary, residual)?;
            let count = entry.row_count.div_ceil(32) as usize;
            let mut bytes = Vec::with_capacity(count * TILE_BYTES);
            for tile in 0..count {
                bytes.extend_from_slice(&pair.primary().primary_tile(tile as u32)?);
            }
            let primary = terms(pair.primary().primary_certificate()?);
            let refined = terms(pair.primary().refined_certificate()?);
            loaded.push((entry, bytes, tiles, pair.into_residual(), primary, refined));
            tiles += count;
        }
        // No bound certificate exists until every segment's physical checks passed.
        let mut segments = Vec::with_capacity(loaded.len());
        for (i, (entry, bytes, tile_start, residual, primary, refined)) in
            loaded.into_iter().enumerate()
        {
            let binding = Binding {
                generation: current.generation,
                segment: i as u32,
                id: entry.id,
                first_row: entry.first_row,
                row_count: entry.row_count,
                model_hash: manifest.model_hash,
            };
            segments.push(Segment {
                entry,
                tiles: bytes,
                tile_start,
                residual,
                certificate: BoundPair::bind(binding, primary, refined)?,
            });
        }
        let data = Arc::new(Data {
            model,
            model_hash: manifest.model_hash,
            generation: current.generation,
            total_rows: manifest.total_rows,
            tiles,
            segments,
        });
        Ok(Self {
            data,
            pool: WorkerPool::new(workers)?,
            _lock: lock,
        })
    }
    // The approved API admits only nonempty committed generations.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> u64 {
        self.data.total_rows
    }
    pub fn generation(&self) -> u64 {
        self.data.generation
    }
    pub fn segment_count(&self) -> u32 {
        self.data.segments.len() as u32
    }
    pub fn certificates(&self, segment: u32) -> Option<SegmentCertificates> {
        self.data
            .segments
            .get(segment as usize)
            .map(|s| s.certificate.metadata())
    }
}
