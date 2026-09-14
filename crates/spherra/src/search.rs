use crate::{
    Error, Index, RowId, Vector,
    builder::validate,
    open::{Data, TILE_BYTES},
};
#[cfg(test)]
use spherra_codec::DirectCode;
use spherra_codec::{FixedPointScorer, Pq96Code, PreparedScorerQuery, score_tile_primary};
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
    raw: i64,
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
    pub fn score(&self) -> f64 {
        self.raw as f64 / FixedPointScorer::new().metadata().comparison_scale() as f64
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
/// How a search method orders rows from their i64 Q24 primary or refined score.
/// Keys sort descending, then by ascending row ID.
pub(crate) trait Ranking: 'static {
    type Key: Copy + Ord + Send + 'static;
    /// `magnitudes` is the row's segment cache. Cosine never reads it, keeping
    /// its scan free of magnitude access.
    fn key(score: i64, magnitudes: &[u16], local: usize) -> Self::Key;
}
pub(crate) struct Cosine;
impl Ranking for Cosine {
    type Key = i64;
    #[inline(always)]
    fn key(score: i64, _: &[u16], _: usize) -> i64 {
        score
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
pub(crate) struct Selected<K> {
    pub key: K,
    pub primary: i64,
    pub raw: i64,
    pub row: u64,
    pub segment: usize,
    pub local: u32,
}
/// The effective candidate budget and at most k refined candidates.
pub(crate) type Selection<K> = (usize, Vec<Selected<K>>);
#[cfg(test)]
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
    ) -> Result<Selection<R::Key>, Error> {
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
            refined.push(Selected {
                key: R::key(raw, &s.magnitudes, candidate.local as usize),
                primary: candidate.primary,
                raw,
                row: candidate.row,
                segment: candidate.segment,
                local: candidate.local,
            });
        }
        refined.sort_unstable_by(|a, b| b.key.cmp(&a.key).then_with(|| a.row.cmp(&b.row)));
        refined.truncate(options.k);
        Ok((budget, refined))
    }
    pub fn search(&self, query: &Vector, options: SearchOptions) -> Result<SearchResult, Error> {
        let (budget, selected) = self.select::<Cosine>(query, options)?;
        let mut hits = Vec::with_capacity(selected.len());
        for c in selected {
            let s = &self.data.segments[c.segment];
            let interval =
                s.certificate
                    .interval(self.data.binding(c.segment), c.row, c.primary, c.raw)?;
            hits.push(Hit {
                row: RowId(c.row),
                segment: c.segment as u32,
                raw: c.raw,
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
