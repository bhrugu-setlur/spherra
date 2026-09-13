use crate::model::DriftBaseline;
use std::{cmp::Ordering, collections::BinaryHeap};
const CAPACITY: usize = 65536;
/// Reconstruction change statistics, not a recall estimate.
#[derive(Clone, Debug, PartialEq)]
pub struct DriftStatistics {
    primary: [f64; 3],
    refined: [f64; 3],
    outside_fraction: f64,
}
impl DriftStatistics {
    /// Nearest-rank p50, p95 and p99 over the selected sample.
    pub fn primary(&self) -> [f64; 3] {
        self.primary
    }
    pub fn refined(&self) -> [f64; 3] {
        self.refined
    }
    pub fn outside_fraction(&self) -> f64 {
        self.outside_fraction
    }
    pub(crate) fn baseline(&self) -> DriftBaseline {
        DriftBaseline {
            primary: self.primary,
            refined: self.refined,
            outside_fraction: self.outside_fraction,
        }
    }
}
#[derive(Clone, Debug)]
pub struct DriftReport {
    stats: DriftStatistics,
    sample_size: usize,
    warned: bool,
    insufficient_sample: bool,
}
impl DriftReport {
    pub fn stats(&self) -> &DriftStatistics {
        &self.stats
    }
    pub fn sample_size(&self) -> usize {
        self.sample_size
    }
    pub fn warned(&self) -> bool {
        self.warned
    }
    pub fn insufficient_sample(&self) -> bool {
        self.insufficient_sample
    }
}
#[derive(Clone, Debug)]
struct Sample {
    priority: [u8; 32],
    row: u64,
    values: [f32; 3],
}
impl PartialEq for Sample {
    fn eq(&self, o: &Self) -> bool {
        (self.priority, self.row) == (o.priority, o.row)
    }
}
impl Eq for Sample {}
impl PartialOrd for Sample {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Sample {
    fn cmp(&self, o: &Self) -> Ordering {
        (self.priority, self.row).cmp(&(o.priority, o.row))
    }
}
pub(crate) struct DriftSample {
    id: [u8; 16],
    heap: BinaryHeap<Sample>,
    count: u64,
    exceeded: u64,
}
impl DriftSample {
    pub fn new(id: [u8; 16]) -> Self {
        Self {
            id,
            heap: BinaryHeap::new(),
            count: 0,
            exceeded: 0,
        }
    }
    pub fn add(&mut self, row: u64, values: [f32; 3], baseline: &DriftBaseline) {
        self.count += 1;
        self.exceeded += u64::from(f64::from(values[1]) > baseline.refined[2]);
        let mut key = [0; 24];
        key[..8].copy_from_slice(&row.to_le_bytes());
        key[8..].copy_from_slice(&self.id);
        let sample = Sample {
            priority: *blake3::hash(&key).as_bytes(),
            row,
            values,
        };
        if self.heap.len() < CAPACITY {
            self.heap.push(sample)
        } else if self.heap.peek().is_some_and(|worst| sample < *worst) {
            self.heap.pop();
            self.heap.push(sample);
        }
    }
    pub fn report(&self, baseline: &DriftBaseline) -> DriftReport {
        let stats = summarize(&self.heap.iter().map(|s| s.values).collect::<Vec<_>>());
        let insufficient_sample = self.count < 1000;
        let warned = !insufficient_sample
            && (stats.refined[1] > 1.25 * baseline.refined[1]
                || self.exceeded as f64 > 0.05 * self.count as f64);
        DriftReport {
            stats,
            sample_size: self.heap.len(),
            warned,
            insufficient_sample,
        }
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.heap.len()
    }
    #[cfg(test)]
    pub fn selected(&self) -> Vec<(u64, [f32; 3])> {
        let mut v: Vec<_> = self.heap.iter().map(|s| (s.row, s.values)).collect();
        v.sort_by_key(|s| s.0);
        v
    }
}
pub(crate) fn summarize(values: &[[f32; 3]]) -> DriftStatistics {
    assert!(!values.is_empty());
    let percentiles = |column: usize| {
        let mut v: Vec<_> = values.iter().map(|row| row[column]).collect();
        v.sort_by(f32::total_cmp);
        [50, 95, 99].map(|p| f64::from(v[(v.len() * p).div_ceil(100) - 1]))
    };
    // Sorted summation makes the statistic independent of reservoir iteration order.
    let mut outside: Vec<_> = values.iter().map(|v| v[2]).collect();
    outside.sort_by(f32::total_cmp);
    DriftStatistics {
        primary: percentiles(0),
        refined: percentiles(1),
        outside_fraction: outside.iter().map(|v| f64::from(*v)).sum::<f64>() / values.len() as f64,
    }
}
