use crate::{
    Error, Index, RowId, Vector,
    builder::validate,
    open::{Data, TILE_BYTES},
};
use spherra_codec::{
    DirectCode, FixedPointScorer, Pq96Code, PreparedScorerQuery, score_tile_primary,
};
use std::{
    cmp::{Ordering, Reverse},
    collections::BinaryHeap,
    sync::{Arc, Mutex, mpsc},
    thread,
};
pub struct SearchOptions {
    pub k: usize,
    pub candidate_budget: Option<usize>,
}
/// An approximate ranked hit with an interval for its true cosine score.
/// ```compile_fail
/// let hit = spherra::Hit {};
/// ```
#[derive(Clone, Debug)]
pub struct Hit {
    row: RowId,
    segment: u32,
    #[cfg(test)]
    raw: i64,
    corrected: f64,
    interval: (f64, f64),
    magnitude_bits: u16,
}
impl Hit {
    pub fn row(&self) -> RowId {
        self.row
    }
    pub fn segment(&self) -> u32 {
        self.segment
    }
    /// Approximate cosine: the refined score divided by the length of its
    /// reconstruction. `interval()` is the certified range for the true cosine;
    /// the estimate itself is not certified to lie inside it.
    pub fn score(&self) -> f64 {
        self.corrected / FixedPointScorer::new().metadata().comparison_scale() as f64
    }
    pub fn interval(&self) -> (f64, f64) {
        self.interval
    }
    /// The original input length rounded to FP16, promoted exactly to FP32.
    /// A tiny positive input length may round to zero. This is metadata, not
    /// a confidence score, and does not participate in ranking or intervals.
    pub fn stored_magnitude(&self) -> f32 {
        half::f16::from_bits(self.magnitude_bits).to_f32()
    }
    #[cfg(test)]
    pub(crate) fn raw(&self) -> i64 {
        self.raw
    }
}
#[derive(Clone, Debug)]
pub struct SearchResult {
    generation: u64,
    candidate_budget: usize,
    rows_scanned: u64,
    rows_refined: u64,
    hits: Vec<Hit>,
}
impl SearchResult {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn candidate_budget(&self) -> usize {
        self.candidate_budget
    }
    pub fn rows_scanned(&self) -> u64 {
        self.rows_scanned
    }
    pub fn rows_refined(&self) -> u64 {
        self.rows_refined
    }
    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }
}
/// How a search method orders rows. Primary candidates use an exact integer key
/// from the i64 Q24 primary score; refined candidates use an FP64 key from the
/// length-corrected refined score. Keys sort descending, then by ascending row.
pub(crate) trait Ranking: 'static {
    type Key: Copy + Ord + Send + 'static;
    /// `magnitudes` is the row's segment cache. Cosine never reads it, keeping
    /// its scan free of magnitude access.
    fn key(score: i64, magnitudes: &[u16], local: usize) -> Self::Key;
    fn refined_key(corrected: f64, magnitudes: &[u16], local: usize) -> f64;
}
pub(crate) struct Cosine;
impl Ranking for Cosine {
    type Key = i64;
    #[inline(always)]
    fn key(score: i64, _: &[u16], _: usize) -> i64 {
        score
    }
    #[inline(always)]
    fn refined_key(corrected: f64, _: &[u16], _: usize) -> f64 {
        corrected
    }
}
/// The refined Q24 score divided by the FP64 length of its reconstruction p + e
/// (user-approved correction amendment). Squares are summed in coordinate order
/// without fused multiply-add, so the value is bit-reproducible. A non-normal
/// length leaves the raw score unchanged.
pub(crate) fn corrected_score(raw: i64, p: &[f32; 768], e: &[f32; 768]) -> f64 {
    let mut squared = 0.0_f64;
    for (&p, &e) in p.iter().zip(e) {
        let v = f64::from(p) + f64::from(e);
        squared += v * v;
    }
    let length = squared.sqrt();
    if length.is_normal() {
        raw as f64 / length
    } else {
        raw as f64
    }
}
#[derive(Clone, Copy)]
struct Candidate<K> {
    key: K,
    primary: i64,
    row: u64,
    segment: usize,
    local: u32,
}
impl<K: Ord> PartialEq for Candidate<K> {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}
impl<K: Ord> Eq for Candidate<K> {}
impl<K: Ord> PartialOrd for Candidate<K> {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl<K: Ord> Ord for Candidate<K> {
    fn cmp(&self, o: &Self) -> Ordering {
        self.key
            .cmp(&o.key)
            .then_with(|| Reverse(self.row).cmp(&Reverse(o.row)))
    }
}
fn admit<K: Ord>(
    heap: &mut BinaryHeap<Reverse<Candidate<K>>>,
    candidate: Candidate<K>,
    budget: usize,
) {
    if heap.len() < budget {
        heap.push(Reverse(candidate))
    } else if heap.peek().is_some_and(|worst| candidate > worst.0) {
        heap.pop();
        heap.push(Reverse(candidate));
    }
}
fn scan<R: Ranking>(
    data: &Data,
    query: &PreparedScorerQuery,
    begin: usize,
    end: usize,
    budget: usize,
) -> Vec<Candidate<R::Key>> {
    let mut heap = BinaryHeap::new();
    for (segment, s) in data.segments.iter().enumerate() {
        let count = s.tiles.len() / TILE_BYTES;
        let first = begin.max(s.tile_start);
        let last = end.min(s.tile_start + count);
        for global in first..last {
            let ordinal = global - s.tile_start;
            let tile = &s.tiles[ordinal * TILE_BYTES..(ordinal + 1) * TILE_BYTES];
            let lanes = (s.entry.row_count as usize - ordinal * 32).min(32);
            let mut scores = [0; 32];
            score_tile_primary(query, tile, lanes, &mut scores).expect("opened tile geometry");
            for (lane, &score) in scores[..lanes].iter().enumerate() {
                let local = (ordinal * 32 + lane) as u32;
                admit(
                    &mut heap,
                    Candidate {
                        key: R::key(score, &s.magnitudes, local as usize),
                        primary: score,
                        row: s.entry.first_row + u64::from(local),
                        segment,
                        local,
                    },
                    budget,
                );
            }
        }
    }
    heap.into_iter().map(|c| c.0).collect()
}
/// A refined candidate in final order, before its certificate interval.
pub(crate) struct Selected {
    pub key: f64,
    pub corrected: f64,
    pub primary: i64,
    pub raw: i64,
    pub row: u64,
    pub segment: usize,
    pub local: u32,
}
/// The effective candidate budget and at most k refined candidates.
pub(crate) type Selection = (usize, Vec<Selected>);
pub(crate) fn decode_lane(tile: &[u8], lane: usize) -> DirectCode {
    let nibbles = std::array::from_fn(|c| (tile[c * 16 + lane / 2] >> ((lane % 2) * 4)) & 15);
    DirectCode::from_nibbles(nibbles).expect("masked four-bit codes")
}
type Job = Box<dyn FnOnce() + Send + 'static>;
pub(crate) struct WorkerPool {
    sender: Option<mpsc::SyncSender<Job>>,
    threads: Vec<thread::JoinHandle<()>>,
}
impl WorkerPool {
    pub fn new(workers: usize) -> Result<Self, Error> {
        let (sender, receiver) = mpsc::sync_channel::<Job>(workers);
        let receiver = Arc::new(Mutex::new(receiver));
        let mut pool = Self {
            sender: Some(sender),
            threads: Vec::with_capacity(workers),
        };
        for worker in 0..workers {
            let receiver = receiver.clone();
            pool.threads.push(
                thread::Builder::new()
                    .name(format!("spherra-scan-{worker}"))
                    .spawn(move || {
                        loop {
                            let job = receiver.lock().ok().and_then(|r| r.recv().ok());
                            match job {
                                Some(job) => job(),
                                None => break,
                            }
                        }
                    })?,
            );
        }
        Ok(pool)
    }
    fn submit(&self, job: Job) -> Result<(), Error> {
        self.sender
            .as_ref()
            .ok_or(Error::Corrupt)?
            .send(job)
            .map_err(|_| Error::Corrupt)
    }
}
impl Drop for WorkerPool {
    fn drop(&mut self) {
        self.sender.take();
        for worker in self.threads.drain(..) {
            let _ = worker.join();
        }
    }
}
impl Index {
    /// Shared option validation, full primary scan, candidate refinement and
    /// final ordering. Returns the effective budget and at most k candidates.
    pub(crate) fn select<R: Ranking>(
        &self,
        query: &Vector,
        options: SearchOptions,
    ) -> Result<Selection, Error> {
        if options.k == 0 {
            return Err(Error::InvalidOptions);
        }
        let budget = match options.candidate_budget {
            Some(b) => b,
            None => options
                .k
                .checked_mul(2)
                .ok_or(Error::InvalidOptions)?
                .max(200),
        };
        if budget < options.k {
            return Err(Error::InvalidOptions);
        }
        let budget = budget.min(self.len() as usize);
        validate(query, 0)?;
        let scorer = FixedPointScorer::new();
        let model = &self.data.model;
        let query = Arc::new(
            scorer
                .prepare_query(&model.plan, query, &model.quantizer, &model.codebook)
                .map_err(|_| Error::Corrupt)?,
        );
        let jobs = self.pool.threads.len().min(self.data.tiles);
        let width = self.data.tiles.div_ceil(jobs);
        let (sender, receiver) = mpsc::channel();
        let mut submitted = 0;
        for begin in (0..self.data.tiles).step_by(width) {
            let data = self.data.clone();
            let query = query.clone();
            let sender = sender.clone();
            let end = (begin + width).min(data.tiles);
            self.pool.submit(Box::new(move || {
                let _ = sender.send(scan::<R>(&data, &query, begin, end, budget));
            }))?;
            submitted += 1;
        }
        drop(sender);
        let mut heap = BinaryHeap::new();
        for _ in 0..submitted {
            for candidate in receiver.recv().map_err(|_| Error::Corrupt)? {
                admit(&mut heap, candidate, budget);
            }
        }
        let mut refined = Vec::with_capacity(heap.len());
        for Reverse(candidate) in heap {
            let s = &self.data.segments[candidate.segment];
            let residual = Pq96Code::from_bytes(s.residual.residual_code(candidate.local)?);
            let raw = scorer.refine_from_primary(&query, candidate.primary, &residual);
            let ordinal = candidate.local as usize / 32;
            let tile = &s.tiles[ordinal * TILE_BYTES..(ordinal + 1) * TILE_BYTES];
            let p = model
                .quantizer
                .decode(&decode_lane(tile, candidate.local as usize % 32));
            let corrected = corrected_score(raw, &p, &model.codebook.decode(&residual));
            refined.push(Selected {
                key: R::refined_key(corrected, &s.magnitudes, candidate.local as usize),
                corrected,
                primary: candidate.primary,
                raw,
                row: candidate.row,
                segment: candidate.segment,
                local: candidate.local,
            });
        }
        refined.sort_unstable_by(|a, b| b.key.total_cmp(&a.key).then_with(|| a.row.cmp(&b.row)));
        refined.truncate(options.k);
        Ok((budget, refined))
    }
    pub fn search(&self, query: &Vector, options: SearchOptions) -> Result<SearchResult, Error> {
        let (budget, selected) = self.select::<Cosine>(query, options)?;
        let mut hits = Vec::with_capacity(selected.len());
        for c in selected {
            let s = &self.data.segments[c.segment];
            // The unchanged raw scores authenticate the original-space truth
            // interval. Rescaling this interval would certify a different target.
            let interval =
                s.certificate
                    .interval(self.data.binding(c.segment), c.row, c.primary, c.raw)?;
            hits.push(Hit {
                row: RowId(c.row),
                segment: c.segment as u32,
                #[cfg(test)]
                raw: c.raw,
                corrected: c.corrected,
                interval,
                magnitude_bits: s.magnitudes[c.local as usize],
            });
        }
        Ok(SearchResult {
            generation: self.generation(),
            candidate_budget: budget,
            rows_scanned: self.len(),
            rows_refined: budget as u64,
            hits,
        })
    }
}

#[cfg(test)]
mod correction_tests {
    use super::*;

    #[test]
    fn reconstruction_length_removes_shrinkage_and_expansion() {
        let mut p = [0.0; 768];
        let mut e = [0.0; 768];
        p[0] = 0.25;
        e[0] = 0.25;
        assert_eq!(corrected_score(8, &p, &e), 16.0);
        assert_eq!(corrected_score(-8, &p, &e), -16.0);
        p[0] = 1.5;
        assert_eq!(corrected_score(8, &p, &e), 8.0 / 1.75);
        p[0] = 0.75;
        assert_eq!(corrected_score(8, &p, &e), 8.0);
    }

    #[test]
    fn reconstruction_length_uses_all_coordinates_and_fp64_addition() {
        let mut p = [0.0; 768];
        let mut e = [0.0; 768];
        p[0] = 3.0;
        e[767] = 4.0;
        assert_eq!(corrected_score(10, &p, &e), 2.0);
        p.fill(0.0);
        e.fill(0.0);
        p[0] = 1.0;
        e[0] = 2.0_f32.powi(-25);
        assert_eq!(corrected_score(1, &p, &e), 1.0 / (1.0 + 2.0_f64.powi(-25)));
    }

    #[test]
    fn degenerate_reconstruction_falls_back_and_extreme_finite_values_stay_finite() {
        let mut p = [0.0; 768];
        let mut e = [0.0; 768];
        assert_eq!(corrected_score(7, &p, &e), 7.0);
        p[0] = 1.0;
        e[0] = -1.0;
        assert_eq!(corrected_score(-7, &p, &e), -7.0);
        p.fill(f32::MAX);
        e.fill(f32::MAX);
        assert!(corrected_score(i64::MAX, &p, &e).is_normal());
        p.fill(f32::from_bits(1));
        e.fill(0.0);
        assert!(corrected_score(i64::MIN, &p, &e).is_normal());
    }
}
