//! Exact FP64 top-k with bounded heaps, without retaining indexed originals or
//! full rankings. Row partitions run independently; their top-k sets merge with
//! the same score/row ordering as `ExactOracle`.
use crate::Neighbor;
use spherra_codec::{ScorerError, dot_f64, normalize_fp64};
use spherra_domain::DIMENSION;
use std::{
    cmp::{Ordering, Reverse},
    collections::BinaryHeap,
    fmt,
    sync::Arc,
};
#[derive(Debug)]
pub enum OracleStreamError {
    InvalidOptions,
    RowLimit,
    InvalidQuery { query: usize, source: ScorerError },
    InvalidRow { row: u64, source: ScorerError },
}
impl fmt::Display for OracleStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for OracleStreamError {}
/// Canonical little-endian FP32 row hashing with only one row-sized scratch buffer.
#[derive(Clone, Default)]
pub struct CanonicalRowHasher(blake3::Hasher);
impl CanonicalRowHasher {
    pub fn update(&mut self, rows: &[[f32; DIMENSION]]) {
        let mut bytes = [0; DIMENSION * 4];
        for row in rows {
            for (slot, value) in bytes.chunks_exact_mut(4).zip(row) {
                slot.copy_from_slice(&value.to_le_bytes());
            }
            self.0.update(&bytes);
        }
    }
    pub fn hash(&self) -> String {
        self.0.finalize().to_hex().to_string()
    }
}
#[derive(Clone, Copy, Debug)]
struct Ranked(Neighbor);
impl PartialEq for Ranked {
    fn eq(&self, o: &Self) -> bool {
        self.0.row == o.0.row && self.0.score.to_bits() == o.0.score.to_bits()
    }
}
impl Eq for Ranked {}
impl PartialOrd for Ranked {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Ranked {
    fn cmp(&self, o: &Self) -> Ordering {
        self.0
            .score
            .total_cmp(&o.0.score)
            .then_with(|| o.0.row.cmp(&self.0.row))
    }
}
type Heap = BinaryHeap<Reverse<Ranked>>;
fn admit(heap: &mut Heap, n: Neighbor, k: usize) {
    let n = Ranked(n);
    if heap.len() < k {
        heap.push(Reverse(n))
    } else if heap.peek().is_some_and(|worst| n > worst.0) {
        heap.pop();
        heap.push(Reverse(n));
    }
}
pub struct StreamingOracle {
    queries: Arc<Vec<[f64; DIMENSION]>>,
    heaps: Vec<Heap>,
    k: usize,
    rows: u64,
    hash: CanonicalRowHasher,
}
impl StreamingOracle {
    pub fn new(queries: &[[f32; DIMENSION]], k: usize) -> Result<Self, OracleStreamError> {
        if queries.is_empty() || k == 0 {
            return Err(OracleStreamError::InvalidOptions);
        }
        let queries = queries
            .iter()
            .enumerate()
            .map(|(query, q)| {
                normalize_fp64(q)
                    .map_err(|source| OracleStreamError::InvalidQuery { query, source })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            heaps: (0..queries.len()).map(|_| Heap::new()).collect(),
            queries: Arc::new(queries),
            k,
            rows: 0,
            hash: CanonicalRowHasher::default(),
        })
    }
    /// Append a dense chunk. An invalid row rejects the entire chunk without
    /// changing existing rankings, row count, or corpus hash.
    pub fn extend(&mut self, rows: &[[f32; DIMENSION]]) -> Result<(), OracleStreamError> {
        if rows.is_empty() {
            return Ok(());
        }
        let end = self
            .rows
            .checked_add(rows.len() as u64)
            .filter(|n| *n <= u64::from(u32::MAX))
            .ok_or(OracleStreamError::RowLimit)?;
        let width = rows.len().div_ceil(6);
        let first = self.rows;
        let k = self.k;
        let batches = std::thread::scope(|scope| {
            let handles: Vec<_> = rows
                .chunks(width)
                .enumerate()
                .map(|(batch, rows)| {
                    let queries = self.queries.clone();
                    scope.spawn(move || {
                        let mut heaps: Vec<_> = (0..queries.len()).map(|_| Heap::new()).collect();
                        for (r, raw) in rows.iter().enumerate() {
                            let row = first + (batch * width + r) as u64;
                            let normalized = normalize_fp64(raw)
                                .map_err(|source| OracleStreamError::InvalidRow { row, source })?;
                            for (q, heap) in queries.iter().zip(&mut heaps) {
                                admit(
                                    heap,
                                    Neighbor {
                                        row: row as u32,
                                        score: dot_f64(q, &normalized),
                                    },
                                    k,
                                );
                            }
                        }
                        Ok::<_, OracleStreamError>(heaps)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("oracle worker panicked"))
                .collect::<Result<Vec<_>, _>>()
        })?;
        for heaps in batches {
            for (target, source) in self.heaps.iter_mut().zip(heaps) {
                for Reverse(n) in source {
                    admit(target, n.0, self.k)
                }
            }
        }
        self.hash.update(rows);
        self.rows = end;
        Ok(())
    }
    pub fn row_count(&self) -> u64 {
        self.rows
    }
    pub fn corpus_hash(&self) -> String {
        self.hash.hash()
    }
    pub fn top_k(&self) -> Vec<Vec<Neighbor>> {
        self.heaps
            .iter()
            .map(|heap| {
                let mut rows: Vec<_> = heap.iter().map(|r| r.0.0).collect();
                crate::exact::sort_by_score_then_row(&mut rows);
                rows
            })
            .collect()
    }
    /// Deterministic reference bytes: magic, row count u64, query count u32,
    /// k u32, then each query's actual count u32 and (row u64, score f64) pairs.
    pub fn reference_bytes(&self) -> Vec<u8> {
        let mut bytes = b"SPHROR01".to_vec();
        bytes.extend_from_slice(&self.rows.to_le_bytes());
        bytes.extend_from_slice(&(self.heaps.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(self.k as u32).to_le_bytes());
        for query in self.top_k() {
            bytes.extend_from_slice(&(query.len() as u32).to_le_bytes());
            for n in query {
                bytes.extend_from_slice(&u64::from(n.row).to_le_bytes());
                bytes.extend_from_slice(&n.score.to_le_bytes());
            }
        }
        bytes
    }
}
