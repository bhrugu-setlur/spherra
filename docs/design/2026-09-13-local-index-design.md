# Spherra local index design

Status: **draft for user approval**
Date: 2026-09-13
Supersedes for local scope: the R7 distributed architecture and I4 implementation
specification. Those documents stay in the repository as the record of the
stopped database direction; they no longer describe what is being built.

## 1. Why this exists

The 2026-09-12 certified prune-rate experiment failed its pre-registered rule
([results](../../experiments/2026-09-12-certified-prune-rate-results.md)).
Certified bounds cannot replace a candidate budget, and survivor counts grow
linearly with corpus size. The distributed database direction is closed.

What remains is sound and tested: a 768-d codec that stores each vector in 480
bytes, a checked segment format, and error certificates. This design turns those
into a usable local library.

## 2. Product

A Rust library that builds a compressed vector index on disk, opens it, searches
it, and can have rows appended offline.

- 768 dimensions, cosine similarity (direction only).
- Rows are identified by their `u32` position. The caller maps positions to its
  own keys.
- Every search hit carries a certified interval that contains the true
  full-precision score.

### Explicitly out of scope

Deletes, updates, filters, metadata, caller-supplied IDs, concurrent append,
append while searching, on-disk residual mode, certificate-based pruning, SIMD,
HNSW, multiple segments, recall-drift detection, anything distributed.

## 3. Public API

One new crate, `spherra`, depending on `spherra-codec`, `spherra-format`, and
`spherra-domain`.

```rust
pub struct BuildOptions {
    pub seed: u64,
    pub calibration_rows: usize, // provisional default, see section 7
}

pub struct Index { /* resident codes, tables, certificates */ }

impl Index {
    pub fn build(vectors: &[[f32; 768]], options: BuildOptions) -> Result<Index, Error>;
    pub fn write(&self, directory: &Path) -> Result<(), Error>;
    pub fn open(directory: &Path) -> Result<Index, Error>;
    pub fn search(&self, query: &[f32; 768], k: usize) -> Result<Vec<Hit>, Error>;
    pub fn len(&self) -> u32;
}

pub fn append(directory: &Path, vectors: &[[f32; 768]]) -> Result<AppendReport, Error>;

pub struct Hit { pub row: u32, pub score: f64, pub lower: f64, pub upper: f64 }

pub struct AppendReport {
    pub rows_added: u32,
    pub epsilon_before: f64,
    pub epsilon_after: f64,
    pub drifted: bool,
}
```

`Index` is `Send + Sync`; any number of threads may search one index.

## 4. Search

There is one path. Residual codes are resident, so every row is refined.

1. Validate the query with `ValidatedVector`. A non-finite component, a norm
   above FP16 range, or a norm below the reliable-direction threshold is an error.
2. `FixedPointScorer::prepare_query` builds both lookup tables once.
3. Score every row with `FixedPointScorer::score_refined`: 768 primary lookups
   plus 96 residual lookups, 864 in total. Rows are split into contiguous chunks
   across `std::thread::available_parallelism()` scoped threads; each chunk keeps
   its own top-k; the chunks are merged.
4. Order by score descending, then row ascending, so results are deterministic
   for any thread count.
5. Each hit's interval is `score ± refined epsilon`, rounded outward.

### Why no candidate budget

The two-stage design assumed residuals live on SSD, where each refine is a disk
read. Held in memory, refining a row costs 96 lookups against a 768-lookup scan,
so refining everything costs 12.5% over a primary-only scan and removes the
budget guess. At 10M rows the resident codes are 3.84 GB primary plus 0.96 GB
residual, 4.8 GB total, inside the 20 GiB process cap.

### Why no certificate pruning

With no separate primary stage there is nothing to prune before refinement.
Measured against the stored block certificate, pruning would have skipped a
median 31% of the refine work, which is about 4% of total query compute, and
nothing at all for more than 10% of queries. It does not pay for the extra path.

### Expected speed (unmeasured projection)

The M1 benchmark measured the scalar primary scan at about 395,000 rows per
second per core. Refine-all is roughly 350,000 rows per second per core. On the
six performance cores that projects to about 0.5 s per query at 1M rows and
about 5 s at 10M rows. That is slow at the local target. SIMD is the main lever
and is deferred; this design records real numbers rather than promising any.

## 5. What a certificate means in this library

Hit intervals come from the refined certificate stored in the primary file. On
open, the library does not trust the stored `epsilon`: it recomputes it from the
stored terms and requires the transform and query-norm terms to equal the
current scorer's constants.

The reconstruction term cannot be recomputed without the original vectors,
which are not stored. It is trusted because the file passed its CRC checks, its
whole-file BLAKE3, and every codec, transform, quantizer, and codebook identity
check. A file that was deliberately edited and re-hashed could therefore report
a false bound. This is stated in the README.

The certificate is never used to claim that results are exact. Refined bounds
are about 0.107 wide on SciFact, wider than the gaps between close neighbours.

## 6. On-disk layout and crash safety

An index is a directory:

```
index/
  CURRENT                    names the live segment id
  <segment-id>.primary
  <segment-id>.residual
```

`write` and `append` write a new segment pair under a fresh segment id, sync
both files, then replace `CURRENT` by write-temp, sync, rename, and directory
sync. `open` reads `CURRENT` and pairs exactly those two files. A crash at any
point before the rename leaves the previous index intact. Superseded pairs are
removed after the rename succeeds; a crash during removal only leaves orphans,
which `open` ignores.

## 7. Build

1. Validate every input row with `ValidatedVector`. A row without a reliable
   direction is rejected with its position; it is not silently dropped.
2. Choose calibration rows deterministically from the seed. The default size is
   provisional and is set by measurement in the implementation plan.
3. `TransformPlan::from_seed`, `QuantizerTable::train`, then
   `Pq96Codebook::train` on the calibration residuals, as the harness does today.
4. Encode every row.
5. `build_exhaustive_certificate` over all rows while their originals are in
   memory.

The FP16 radius from `ValidatedVector` is stored in the first two bytes of each
row's radius/flags word, little endian; the flag bytes are zero. Search does not
read it.

## 8. Append

Append is offline. The caller must not have the index open for search in the
same process while it runs.

1. Open the current index.
2. Validate and encode the new rows with the **existing** transform, quantizer,
   and codebook. Nothing is retrained.
3. Build a certificate over the new rows alone, while their originals exist.
4. Combine it with the stored certificate:
   - reconstruction error: the maximum of the two, which is sound because it is
     a per-row maximum;
   - serving error: recomputed from the maximum primary and residual
     reconstruction norms over **all** rows, which are recomputable from the
     stored codes. Taking the larger of the two stored serving terms is not
     sound: the refined serving term depends jointly on both norms, and the
     maxima can come from different subsets;
   - epsilon: recomputed with outward rounding.
5. Write a new segment pair and swap `CURRENT`.

`drifted` is true when the refined epsilon after the append exceeds the one
before by more than 10%. That catches new rows that reconstruct worse than
anything already indexed, which widens every hit's interval. It does not detect
a gradual loss of recall; the README says so. The 10% threshold is a starting
value. When new data differs in kind — a different embedding model or domain —
the caller should rebuild.

## 9. Required changes to existing crates

**spherra-codec**
- `QuantizerTable::from_centers` and `Pq96Codebook::from_centroids`: rebuild
  tables from stored values and recompute their identities with the existing
  derivations. Today only `train` exists, so a written index cannot be opened.
- A constructor that turns stored certificate terms into an `ErrorCertificate`,
  recomputing `epsilon` and rejecting scorer-constant mismatches, plus a public
  outward-rounded `bounds` for a score.
- The append combination in section 8, implemented beside
  `build_exhaustive_certificate` so it reuses the same outward-rounding helpers.

**spherra-format**
- A new required `TransformSeed` section in the primary file. The transform is
  rebuilt only from its seed and the file stores only the transform identity, so
  a written index cannot be opened today. The seed is verified by rebuilding the
  plan and comparing identities. Adding a required section breaks existing
  files, so the major format version goes from 1 to 2. No segment files exist
  outside tests.
- A tile-level primary decode. `PrimaryFileReader::primary_code` reads a whole
  32-row tile to return one row, which would read each tile 32 times when
  loading an index.

**Project guide** (needs user approval)
- The frozen decision "residuals are candidate-only SSD/page-cache data; do not
  count them as resident scan memory" is amended for the local index: residuals
  are resident and counted.
- The project paragraph, product contract, frozen decisions, and status are
  rewritten for the local library.

## 10. Verification

- **Round trip.** build → write → open → search returns bit-identical hits to
  searching the built index.
- **Refine path equivalence.** For every row, `score_refined` equals
  `score_prepared_candidate`. The refined certificate was proven against the
  second path; this ties it to the one search uses.
- **Soundness.** Every hit's FP64 truth from `ExactOracle` lies inside its
  interval, on SciFact and on a generated corpus.
- **Recall.** recall@10 against `ExactOracle` is at least the best candidate-
  budget recall recorded on 2026-08-06 for the same corpus and seed. Refining
  every row must not do worse than refining some of them.
- **Append.** A codec-level test that the combined certificate equals a
  certificate built once over all rows under the same tables, bit for bit; and
  an end-to-end test that appended rows are found by search.
- **Crash safety.** Failure injected before the `CURRENT` rename leaves the old
  index openable and unchanged.
- **Rejection.** A wrong seed, a swapped residual file, a truncated file, and a
  tampered certificate term each fail `open` with a structured error.
- **Determinism.** Search results are identical at 1, 2, and 6 threads.
- **Speed and memory.** Query latency and resident memory recorded at 100k and
  1M generated rows. Recorded, not gated.
