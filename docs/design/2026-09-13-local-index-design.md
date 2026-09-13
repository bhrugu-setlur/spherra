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
it quickly, and can have rows appended offline.

- 768 dimensions, cosine similarity (direction only).
- Rows are identified by their `u32` position. The caller maps positions to its
  own keys.
- Every search hit carries a certified interval that contains the true
  full-precision score.

### Explicitly out of scope

Deletes, updates, filters, metadata, caller-supplied IDs, concurrent append,
append while searching, on-disk residual mode, certificate-based pruning, x86
SIMD, memory mapping, HNSW, multiple segments, recall-drift detection, anything
distributed.

## 3. Public API

One new crate, `spherra`, depending on `spherra-codec`, `spherra-format`, and
`spherra-domain`.

```rust
pub struct BuildOptions {
    pub seed: u64,
    pub calibration_rows: usize, // provisional default, see section 8
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

`build` holds its input in memory: 10M FP32 rows are about 30 GB, which does not
fit on the development machine. A corpus that large is loaded by building from
a first chunk and appending the rest in chunks.

## 4. Search

There is one path. Residual codes are resident, so every row is refined.

1. Validate the query with `ValidatedVector`. A non-finite component, a norm
   above FP16 range, or a norm below the reliable-direction threshold is an error.
2. `FixedPointScorer::prepare_query` builds both lookup tables once.
3. Score every row with the fastest kernel from section 5 that is available on
   this CPU. Every kernel returns the same `i64` score as
   `FixedPointScorer::score_refined`, bit for bit. Rows are split into contiguous
   whole-tile chunks across `std::thread::available_parallelism()` scoped
   threads; each chunk keeps its own top-k; the chunks are merged.
4. Order by score descending, then row ascending, so results are deterministic
   for any thread count and any kernel.
5. Each hit's interval is `score ± refined epsilon`, rounded outward.

The resident index keeps primary codes in the existing `TiledSoa32` layout and
residual codes as one contiguous 96-byte-per-row buffer.

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

## 5. Speed

### Target, fixed before measuring

On the Apple M1 Pro, using every core, at k=10:

- p50 query latency **at most 1.0 s at 10M rows**, and
- p50 query latency **at most 0.1 s at 1M rows**.

Speed work proceeds in stages. After each stage the target is measured, and
**work stops at the first stage that meets it**. Later stages are not built.

The M1 benchmark measured the scalar primary scan at about 395,000 rows per
second per core, including per-row code unpacking. Unmodified refine-all
projects to about 5 s per query at 10M rows. Meeting the target needs roughly
5× more per-core speed on top of threads.

### Why faster kernels cannot change results

Scores are sums of `i64` table entries. Integer addition gives the same answer
whatever order or width it is done in, so a faster kernel either returns exactly
the scalar score or is wrong — there is no rounding drift to hide in. Every
kernel is tested for exact equality against `score_refined`. The certificates,
`codec_id`, and `scorer_version` do not change.

### Stage 1: baseline

Threaded search calling `score_refined` per row (plan Task 8). Measured.

### Stage 2: tile kernel, safe Rust

Score one 32-row tile at a time directly from the `TiledSoa32` bytes, coordinate
by coordinate, into 32 running sums; then add the 96 residual lookups per row.
This removes per-row `DirectCode` unpacking and keeps each coordinate's 16-entry
table hot for all 32 rows.

It uses plain `i64` addition instead of `checked_accumulate`. The scorer already
proves that 864 entries each at most `MAX_ABSOLUTE_TABLE_ENTRY` cannot overflow
`i64`; a `debug_assert` keeps the check in test builds. This kernel is also the
fallback on every CPU without the stage 3 kernel.

### Stage 3: NEON primary kernel

The only unsafe code in the workspace, in one module, `spherra-simd/src/neon.rs`.

NEON has no gather instruction, so a direct `i64` table lookup does not
vectorize. It does have a fast 16-entry byte table lookup (`vqtbl1q_u8`), which
matches one coordinate of a `TiledSoa32` tile: 16 bytes holding 32 nibbles, low
nibbles for even rows and high nibbles for odd rows.

Exact method:

1. With FRACTIONAL_BITS = 24 and unit-length query and table values, every
   primary entry lies far inside the signed 32-bit range. Each entry is biased by
   2^31 into an unsigned 32-bit value and split into four bytes, giving four
   16-entry byte tables per coordinate.
2. For each coordinate: load the 16 tile bytes, split them into even-row and
   odd-row nibble indices, look up each of the four byte tables for both halves,
   and widen-add the results into per-row, per-byte `u16` sums.
3. The `u16` sums are flushed into `u32` sums at most every 257 coordinates,
   before 257 × 255 could overflow them.
4. Each row's score is `Σ byte_sum[b] << 8b` minus 768 × 2^31, which equals the
   scalar `i64` sum exactly.

`prepare_query` already records the largest absolute primary and residual table
entries (`LookupScaleMeasurement`). If any primary entry falls outside the
32-bit range, that query uses the stage 2 kernel. Partial tail tiles use the
stage 2 kernel.

`spherra-simd` sits below `spherra-codec` in the dependency order, so the kernel
takes plain byte tables and slices. The codec builds the byte tables from its
lookup tables and calls it.

Safety evidence, given that AddressSanitizer cannot run on this machine
(project guide caveat): every load is from a slice whose length is checked before
the loop, only full tiles reach the kernel, and exhaustive property tests compare
it to the scalar path. The project guide currently states that the workspace has no
unsafe code; that statement is corrected in the same change.

### Stage 4: NEON residual kernel, conditional

Built only if, after stage 3, residual scoring is more than 25% of measured query
time and the target is still unmet. Residual codes are byte indices into
256-entry tables; the same byte-lane method applies using four 64-entry
`vqtbl4q_u8` lookups per lane.

### Not done

x86 AVX2 (the development machine is ARM; other CPUs use the stage 2 kernel),
memory mapping, and GPU.

## 6. What a certificate means in this library

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

## 7. On-disk layout and crash safety

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

## 8. Build

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

## 9. Append

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

## 10. Required changes to existing crates

**spherra-codec**
- `QuantizerTable::from_centers` and `Pq96Codebook::from_centroids`: rebuild
  tables from stored values and recompute their identities with the existing
  derivations. Today only `train` exists, so a written index cannot be opened.
- A constructor that turns stored certificate terms into an `ErrorCertificate`,
  recomputing `epsilon` and rejecting scorer-constant mismatches, plus a public
  outward-rounded `bounds` for a score.
- The append combination in section 9, implemented beside
  `build_exhaustive_certificate` so it reuses the same outward-rounding helpers.
- The stage 2 tile kernel, and construction of the stage 3 byte-lane tables from
  a prepared query's lookup tables.

**spherra-simd**
- `neon.rs`, compiled only on `aarch64`, with `unsafe` allowed in that module
  alone. Every other crate and module keeps the workspace `deny(unsafe_code)`.
- `proptest` as a dev-dependency.

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
- The caveat stating that the workspace contains no unsafe code is corrected to
  name `spherra-simd/src/neon.rs` and its evidence.
- The project paragraph, product contract, frozen decisions, and status are
  rewritten for the local library.

## 11. Verification

- **Round trip.** build → write → open → search returns bit-identical hits to
  searching the built index.
- **Refine path equivalence.** For every row, `score_refined` equals
  `score_prepared_candidate`. The refined certificate was proven against the
  second path; this ties it to the one search uses.
- **Kernel equality.** Property tests over random lookup tables — including
  entries at the edges of the 32-bit range and entries outside it — random tile
  bytes, and row counts from 1 to 100: the stage 2 and stage 3 kernels equal
  `score_refined` exactly. The same holds for every row of SciFact and a
  generated 100k corpus over 20 queries.
- **Fallback.** A query with a primary entry outside the 32-bit range is routed
  to the stage 2 kernel and still matches.
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
- **Determinism.** Search results are identical at 1, 2, and 8 threads and with
  every kernel.
- **Speed.** p50 and p99 query latency and resident memory at 100k, 1M, and 10M
  generated rows after each speed stage, judged against section 5. Recall is not
  measured at 10M: the exact oracle does not fit in memory at that size.
