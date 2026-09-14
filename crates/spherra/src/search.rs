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
#[derive(Clone, Copy)]
struct Candidate {
    score: i64,
    row: u64,
    segment: usize,
    local: u32,
}
impl PartialEq for Candidate {
    fn eq(&self, o: &Self) -> bool {
        (self.score, self.row) == (o.score, o.row)
    }
}
impl Eq for Candidate {}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Candidate {
    fn cmp(&self, o: &Self) -> Ordering {
        (self.score, Reverse(self.row)).cmp(&(o.score, Reverse(o.row)))
    }
}
fn admit(heap: &mut BinaryHeap<Reverse<Candidate>>, candidate: Candidate, budget: usize) {
    if heap.len() < budget {
        heap.push(Reverse(candidate))
    } else if heap.peek().is_some_and(|worst| candidate > worst.0) {
        heap.pop();
        heap.push(Reverse(candidate));
    }
}
fn scan(
    data: &Data,
    query: &PreparedScorerQuery,
    begin: usize,
    end: usize,
    budget: usize,
) -> Vec<Candidate> {
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
                        score,
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
    pub(crate) fn worker_count(&self) -> usize {
        self.threads.len()
    }
    pub(crate) fn submit(&self, job: Job) -> Result<(), Error> {
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
    pub fn search(&self, query: &Vector, options: SearchOptions) -> Result<SearchResult, Error> {
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
                let _ = sender.send(scan(&data, &query, begin, end, budget));
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
            let residual = Pq96Code::from_bytes(
                self.data.segments[candidate.segment]
                    .residual
                    .residual_code(candidate.local)?,
            );
            let raw = scorer.refine_from_primary(&query, candidate.score, &residual);
            refined.push((raw, candidate));
        }
        refined.sort_unstable_by(|(a, ca), (b, cb)| b.cmp(a).then_with(|| ca.row.cmp(&cb.row)));
        let mut hits = Vec::with_capacity(options.k.min(refined.len()));
        for (raw, candidate) in refined.into_iter().take(options.k) {
            let interval = self.data.segments[candidate.segment].certificate.interval(
                self.data.binding(candidate.segment),
                candidate.row,
                candidate.score,
                raw,
            )?;
            hits.push(Hit {
                row: RowId(candidate.row),
                segment: candidate.segment as u32,
                raw,
                interval,
                magnitude_bits: self.data.segments[candidate.segment].magnitudes
                    [candidate.local as usize],
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
