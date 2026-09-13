use crate::{
    Error,
    container::Current,
    drift::{DriftReport, DriftSample},
    error::lock_error,
    fs::{FileSystem, RealFs, write_all},
    lock::IndexLock,
    manifest::{MAX_ROWS, MAX_SEGMENT_ROWS, MAX_SEGMENTS, Manifest, SegmentEntry},
    model::Model,
    storage,
};
use spherra_codec::{
    CertificateBlockId, CertificateRow, ExhaustiveBlock, FixedPointScorer,
    build_exhaustive_certificate,
};
use spherra_domain::{ChunkId, DocumentId, PutSeq, ValidatedVector};
use spherra_format::{
    PairedSegmentReaders, PrimaryFileReader, PrimarySegment, ResidualFileReader, ResidualSegment,
    RowEntry, SegmentIdentity, StoredErrorCertificate, encode_primary_segment,
    encode_residual_segment,
};
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};
pub type Vector = [f32; 768];
pub const MAX_TRAINING_ROWS: usize = 32768;
/// A dense index-assigned ordinal. Callers cannot mint an indexed row.
/// ```compile_fail
/// let row = spherra::RowId(0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RowId(pub(crate) u64);
impl RowId {
    pub fn get(self) -> u64 {
        self.0
    }
}
pub struct CreateOptions {
    pub seed: u64,
    pub validation_rows: Option<usize>,
}
/// A successfully published batch.
/// ```compile_fail
/// let report = spherra::CommitReport {};
/// ```
#[derive(Clone, Debug)]
pub struct CommitReport {
    generation: u64,
    first_row: RowId,
    rows_added: u64,
    drift: DriftReport,
    cleanup_complete: bool,
}
impl CommitReport {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn first_row(&self) -> RowId {
        self.first_row
    }
    pub fn rows_added(&self) -> u64 {
        self.rows_added
    }
    pub fn drift(&self) -> &DriftReport {
        &self.drift
    }
    pub fn cleanup_complete(&self) -> bool {
        self.cleanup_complete
    }
}
/// Offline exclusive builder. Dropping it publishes nothing; a later builder
/// removes its unreferenced files. At most one segment of originals is retained.
pub struct IndexBuilder {
    dir: PathBuf,
    fs: Arc<dyn FileSystem>,
    _lock: IndexLock,
    model: Model,
    manifest: Manifest,
    first_row: u64,
    added: u64,
    originals: Vec<Vector>,
    drift: DriftSample,
    poison: Option<Error>,
}
pub(crate) fn validate(v: &Vector, position: u64) -> Result<ValidatedVector, Error> {
    let validated =
        ValidatedVector::new(v.to_vec()).map_err(|_| Error::InvalidVector { position })?;
    if validated.direction_unreliable() {
        return Err(Error::InvalidVector { position });
    }
    Ok(validated)
}
impl IndexBuilder {
    pub fn create(dir: &Path, training: &[Vector], options: CreateOptions) -> Result<Self, Error> {
        Self::create_with_fs(dir, training, options, Arc::new(RealFs))
    }
    pub(crate) fn create_with_fs(
        dir: &Path,
        training: &[Vector],
        options: CreateOptions,
        fs: Arc<dyn FileSystem>,
    ) -> Result<Self, Error> {
        // These checks deliberately precede allocation, vector validation, and I/O.
        if training.len() > MAX_TRAINING_ROWS {
            return Err(Error::InvalidTraining);
        }
        let validation = options
            .validation_rows
            .unwrap_or((training.len() / 4).min(4096));
        if validation == 0
            || training
                .len()
                .checked_sub(validation)
                .is_none_or(|n| n < 256)
        {
            return Err(Error::InvalidTraining);
        }
        for (r, row) in training.iter().enumerate() {
            validate(row, r as u64)?;
        }
        fs.create_dir(dir)?;
        let lock = IndexLock::acquire(dir, true).map_err(lock_error)?;
        match fs.read(&dir.join("CURRENT"), Current::BYTE_LEN) {
            Ok(_) => return Err(Error::AlreadyExists),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) if e.kind() == io::ErrorKind::InvalidData => return Err(Error::AlreadyExists),
            Err(e) => return Err(e.into()),
        }
        storage::cleanup(fs.as_ref(), dir, None)?;
        let model = Model::train(training, options.seed, validation)?;
        let bytes = model.file.encode()?;
        let model_hash = *blake3::hash(&bytes).as_bytes();
        storage::stage(
            fs.as_ref(),
            dir,
            &storage::named("model", &model_hash),
            &bytes,
            |bytes| {
                Model::restore(crate::model::ModelFile::decode(&bytes)?)?;
                Ok(())
            },
        )?;
        let index_id = storage::unique_id();
        Ok(Self {
            dir: dir.to_owned(),
            fs,
            _lock: lock,
            model,
            manifest: Manifest {
                index_id,
                generation: 1,
                previous: [0; 32],
                model_hash,
                total_rows: 0,
                segments: Vec::new(),
            },
            first_row: 0,
            added: 0,
            originals: Vec::new(),
            drift: DriftSample::new(index_id),
            poison: None,
        })
    }
    pub fn append(dir: &Path) -> Result<Self, Error> {
        Self::append_with_fs(dir, Arc::new(RealFs))
    }
    pub(crate) fn append_with_fs(dir: &Path, fs: Arc<dyn FileSystem>) -> Result<Self, Error> {
        let lock = IndexLock::acquire(dir, true).map_err(lock_error)?;
        let (current, mut manifest, model) = storage::load(fs.as_ref(), dir)?;
        // Appending must not turn committed corruption into a fresh generation.
        for entry in &manifest.segments {
            verify_segment(fs.as_ref(), dir, &model, manifest.index_id, entry)?;
        }
        storage::cleanup(fs.as_ref(), dir, Some(&current))?;
        manifest.generation = manifest.generation.checked_add(1).ok_or(Error::RowLimit)?;
        manifest.previous = current.manifest_hash;
        let first_row = manifest.total_rows;
        let index_id = manifest.index_id;
        Ok(Self {
            dir: dir.to_owned(),
            fs,
            _lock: lock,
            model,
            manifest,
            first_row,
            added: 0,
            originals: Vec::new(),
            drift: DriftSample::new(index_id),
            poison: None,
        })
    }
    pub fn push(&mut self, vector: &Vector) -> Result<RowId, Error> {
        if let Some(e) = &self.poison {
            return Err(e.clone());
        }
        let row = self
            .first_row
            .checked_add(self.added)
            .ok_or(Error::RowLimit)?;
        if row >= MAX_ROWS {
            return Err(Error::RowLimit);
        }
        if self.originals.is_empty() && self.manifest.segments.len() >= MAX_SEGMENTS {
            return Err(Error::SegmentLimit);
        }
        validate(vector, row)?;
        self.originals.push(*vector);
        self.added += 1;
        if self.originals.len() == MAX_SEGMENT_ROWS as usize {
            if let Err(e) = self.stage_segment() {
                self.poison = Some(e.clone());
                return Err(e);
            }
        }
        Ok(RowId(row))
    }
    fn stage_segment(&mut self) -> Result<(), Error> {
        if self.originals.is_empty() {
            return Ok(());
        }
        if self.manifest.segments.len() >= MAX_SEGMENTS {
            return Err(Error::SegmentLimit);
        }
        let count = self.originals.len();
        let first = self.first_row + self.added - count as u64;
        let model = &self.model;
        let encoded = std::thread::scope(|scope| {
            let chunk = count.div_ceil(6);
            let handles: Vec<_> = self
                .originals
                .chunks(chunk)
                .enumerate()
                .map(|(batch, rows)| {
                    scope.spawn(move || {
                        rows.iter()
                            .enumerate()
                            .map(|(i, v)| model.encode(v, first + (batch * chunk + i) as u64))
                            .collect::<Result<Vec<_>, Error>>()
                    })
                })
                .collect();
            let mut encoded = Vec::with_capacity(count);
            for h in handles {
                encoded.extend(h.join().expect("encoding worker panicked")?);
            }
            Ok::<_, Error>(encoded)
        })?;
        let id = storage::unique_id();
        let block = ExhaustiveBlock::from_rows(
            CertificateBlockId::from_bytes(*blake3::hash(&id).as_bytes()),
            count as u32,
            self.originals
                .iter()
                .zip(&encoded)
                .enumerate()
                .map(|(i, (original, e))| {
                    CertificateRow::new(i as u32, original, &e.primary, &e.residual)
                }),
        )?;
        let cert = build_exhaustive_certificate(
            &FixedPointScorer::new(),
            &model.plan,
            &model.quantizer,
            &model.codebook,
            &block,
        )?;
        let identity = SegmentIdentity {
            collection_id: self.manifest.index_id,
            segment_id: id,
            codec_id: model.file.codec_id,
            scorer_version: model.file.scorer_version,
            transform_id: model.file.transform_id,
            quantizer_id: model.file.quantizer_id,
            pq_codebook_id: model.file.codebook_id,
            layout: spherra_format::LayoutId::TiledSoa32,
        };
        let primary = PrimarySegment {
            identity,
            rows: (first..first + count as u64)
                .map(|r| RowEntry {
                    chunk_id: ChunkId::from_u128(u128::from(r)),
                    document_id: DocumentId::from_u128(0),
                    put_seq: PutSeq::new(0, r).expect("bounded row id"),
                })
                .collect(),
            radius_flags: encoded
                .iter()
                .map(|e| {
                    let b = e.radius.to_le_bytes();
                    [b[0], b[1], 0, 0]
                })
                .collect(),
            primary_codes: encoded.iter().map(|e| *e.primary.as_bytes()).collect(),
            quantizer_table: model.file.centers.clone(),
            primary_certificate: stored(cert.primary().terms()),
            refined_certificate: stored(cert.refined().terms()),
        };
        let residual = ResidualSegment {
            identity,
            row_count: count as u32,
            residual_codes: encoded.iter().map(|e| *e.residual.as_bytes()).collect(),
            pq_codebook: model.file.centroids.clone(),
        };
        let expected = model.expectations();
        let bytes = encode_primary_segment(&primary)?;
        drop(primary);
        let p = PrimaryFileReader::open_bytes(bytes.clone(), &expected)?;
        let primary_hash = p.file_identity();
        drop(p);
        let primary_len = bytes.len() as u64;
        storage::stage(
            self.fs.as_ref(),
            &self.dir,
            &format!("{}.primary", storage::hex(&id)),
            &bytes,
            |bytes| {
                PrimaryFileReader::open_bytes(bytes, &expected)?;
                Ok(())
            },
        )?;
        drop(bytes);
        let bytes = encode_residual_segment(&residual)?;
        drop(residual);
        let r = ResidualFileReader::open_bytes(bytes.clone(), &expected)?;
        let residual_hash = r.file_identity();
        drop(r);
        let residual_len = bytes.len() as u64;
        storage::stage(
            self.fs.as_ref(),
            &self.dir,
            &format!("{}.residual", storage::hex(&id)),
            &bytes,
            |bytes| {
                ResidualFileReader::open_bytes(bytes, &expected)?;
                Ok(())
            },
        )?;
        for (i, e) in encoded.iter().enumerate() {
            self.drift
                .add(first + i as u64, e.stats, &model.file.baseline);
        }
        self.manifest.segments.push(SegmentEntry {
            id,
            first_row: first,
            row_count: count as u32,
            primary_len,
            residual_len,
            primary_hash,
            residual_hash,
        });
        // Release the original allocation as well as its contents between segments.
        self.originals = Vec::new();
        Ok(())
    }
    pub fn commit(mut self) -> Result<CommitReport, Error> {
        if let Some(e) = self.poison.take() {
            return Err(e);
        }
        if self.added == 0 {
            return Err(Error::EmptyCommit);
        }
        self.stage_segment()?;
        self.manifest.total_rows = self.first_row + self.added;
        self.manifest
            .validate(self.manifest.generation)
            .map_err(|_| Error::Corrupt)?;
        let bytes = self.manifest.encode()?;
        let manifest_hash = *blake3::hash(&bytes).as_bytes();
        storage::stage(
            self.fs.as_ref(),
            &self.dir,
            &storage::named("manifest", &manifest_hash),
            &bytes,
            |bytes| {
                let m = Manifest::decode(&bytes)?;
                m.validate(self.manifest.generation)
                    .map_err(|_| Error::Corrupt)
            },
        )?;
        let current = Current {
            generation: self.manifest.generation,
            manifest_hash,
        };
        let path = self.dir.join("CURRENT.tmp");
        let mut file = self.fs.create(&path)?;
        write_all(self.fs.as_ref(), &mut file, &current.encode())?;
        self.fs.sync(&file)?;
        drop(file);
        self.fs.rename(&path, &self.dir.join("CURRENT"))?;
        if self.fs.sync_dir(&self.dir).is_err() {
            return Err(Error::CommitOutcomeUnknown {
                generation: current.generation,
            });
        }
        let cleanup_complete =
            storage::cleanup(self.fs.as_ref(), &self.dir, Some(&current)).is_ok();
        Ok(CommitReport {
            generation: current.generation,
            first_row: RowId(self.first_row),
            rows_added: self.added,
            drift: self.drift.report(&self.model.file.baseline),
            cleanup_complete,
        })
    }
}
pub(crate) fn stored(t: spherra_codec::CertificateTerms) -> StoredErrorCertificate {
    StoredErrorCertificate {
        max_reconstruction_l2_error: t.max_reconstruction_l2_error,
        eta_transform_dot: t.eta_transform_dot,
        query_norm_upper: t.query_norm_upper,
        eta_serving_score: t.eta_serving_score,
        epsilon: t.epsilon,
    }
}
pub(crate) fn terms(t: StoredErrorCertificate) -> spherra_codec::CertificateTerms {
    spherra_codec::CertificateTerms {
        max_reconstruction_l2_error: t.max_reconstruction_l2_error,
        eta_transform_dot: t.eta_transform_dot,
        query_norm_upper: t.query_norm_upper,
        eta_serving_score: t.eta_serving_score,
        epsilon: t.epsilon,
    }
}
fn verify_segment(
    fs: &dyn FileSystem,
    dir: &Path,
    model: &Model,
    index: [u8; 16],
    e: &SegmentEntry,
) -> Result<(), Error> {
    // Manifest sizes are untrusted; never allocate according to them.
    const MAX_FILE: usize = 40 * 1024 * 1024;
    let p = fs.read(
        &dir.join(format!("{}.primary", storage::hex(&e.id))),
        MAX_FILE,
    )?;
    let r = fs.read(
        &dir.join(format!("{}.residual", storage::hex(&e.id))),
        MAX_FILE,
    )?;
    if p.len() as u64 != e.primary_len || r.len() as u64 != e.residual_len {
        return Err(Error::IdentityMismatch);
    }
    let p = PrimaryFileReader::open_bytes(p, &model.expectations())?;
    let r = ResidualFileReader::open_bytes(r, &model.expectations())?;
    if p.file_identity() != e.primary_hash
        || r.file_identity() != e.residual_hash
        || p.identity().collection_id != index
        || p.identity().segment_id != e.id
        || p.row_count() != e.row_count
    {
        return Err(Error::IdentityMismatch);
    }
    spherra_codec::validate_certificate_terms(
        terms(p.primary_certificate()?),
        spherra_codec::ScoreKind::Primary,
    )?;
    spherra_codec::validate_certificate_terms(
        terms(p.refined_certificate()?),
        spherra_codec::ScoreKind::Refined,
    )?;
    PairedSegmentReaders::open(p, r)?;
    Ok(())
}
