# Spherra local index design

Status: **approved at revision 4; approved by the user 2026-09-13**
Date: 2026-09-13
Scope: replaces, for local scope, the R7 distributed architecture
(`docs/design/2026-08-04-polar-lsm-router-design.md`) and the I4
implementation specification. Those documents remain as the record of the
stopped database direction.

## 1. Purpose

Turn the existing 768-d codec and segment format into a usable local vector
index library on one machine: build, open, search, and offline append, with
honest per-hit error intervals and a measured speed target.

## 2. Evidence this design rests on

- **Certified pruning failed** its pre-registered rule
  (`docs/experiments/2026-09-12-certified-prune-rate-results.md`). SciFact k=10:
  block certificates leave a median 2,530 of 3,688 rows; per-row certificates a
  median 943, p90 3,425; survivors grow about linearly with N. Certificates are
  therefore not used to select candidates.
- **Refining more rows does not improve recall.** Measured 2026-09-13 with the
  existing `spherra-bench codec-format`, seed 20260804, 200 queries:

  | Corpus | Budget | recall@10 | recall@100 |
  |---|---|---|---|
  | SciFact, rebuilt bytes, 3,688 indexed rows | 200 / 1,000 / 3,688 (all) | 0.9745 | 0.9811 |
  | generated-correlated 20k | 200 / 1,000 / 20,000 (all) | 0.9370 | 0.9552 |

  Recall@10 is unchanged across the measured budgets 20, 200, 1,000, and all
  rows on archived SciFact. On generated correlated 20k it rises from 0.9330 at
  budget 20 to 0.9370 at budget 200, and is unchanged at 1,000 and all rows.
  Budgets between 20 and 200 were not measured in this reference. At the
  measured budgets of 200 and above, the codec's refined ranking limits recall
  on these two corpora. This factual correction was approved by the user after
  the [Task 1 measurements](../../benchmarks/2026-09-13-local-index-reference.md).
- **The rebuilt SciFact bytes differ from August's** (BLAKE3 `b2e549ce…` against
  `8a20ab21…`), and recall@10 at budget 200 moved from 0.9775 to 0.9745. Quality
  comparisons must use one archived set of bytes.
- **Scalar throughput:** primary scan about 370,000–395,000 rows per second per
  core, including per-row code reconstruction from tiles
  (`crates/spherra-testkit/src/harness.rs`); PQ decode rerank about 160,000 per
  second.
- **Training input limits:** `QuantizerTable::train` needs at least one row;
  `Pq96Codebook::train` needs at least 256 residual rows
  (`pq96.rs` `validate_calibration_residuals`). The existing harness trains on
  a calibration split of `min(4,096, rows / 4)` for file corpora — 1,295 rows for
  SciFact — and 4,096 rows for generated corpora
  (`crates/spherra-testkit/src/corpus.rs`). Training memory scales with the
  training set: the harness holds the transformed rows, a quantizer-training
  copy, and the residuals (`harness.rs` `CodecFormatRun::prepare`), each
  `rows × 3,072` bytes, and `Pq96Codebook::train` takes the residuals as one
  slice with per-row assignment and error vectors (`pq96.rs` `train_subquantizer`).
- **Canonical values:** `QuantizerTable::train` canonicalizes zeros and takes
  sorted quantiles, so trained centers are finite, never `-0.0`, and
  non-decreasing per coordinate; its identity hashes raw bytes (`int4.rs`).
  `Pq96Codebook` identity hashes `canonicalize_zero` bits (`pq96.rs`
  `canonical_f32_bits`), so it cannot distinguish `-0.0` from `+0.0`.
- **Code gaps:** tables can only be trained, not restored; the transform is built
  only from a seed, and its identity hashes the seed, shape, and round seeds but
  **not** the generated signs and permutations (`transform.rs`
  `derive_identity`); `PrimaryFileReader::primary_code` reads a whole tile per
  row; stored certificate decoding checks only finite and non-negative;
  `ErrorCertificate` fields and its raw `bounds` are private; staging does not
  sync or rename; `M1_CODEC_ID` is defined in `spherra-testkit`; there is no
  search, ingest, or public crate.
- **Unreliable directions:** `ValidatedVector` accepts a norm below `1e-12` and
  reports it through `direction_unreliable()`
  (`crates/spherra-domain/src/record.rs`).
- **Existing equivalence:** `crates/spherra-codec/tests/scorer_contract.rs`
  asserts `score_prepared_candidate(...).raw() == score_refined(...).raw()` on its
  fixtures.
- **Platform probes on this machine, 2026-09-13, Rust 1.88.0:** `File::sync_all`
  succeeds on a file and on a directory handle on APFS; `File::lock` is unstable
  on 1.88. `rustix` 1.1.4 is already in `Cargo.lock` and provides `flock`.
- **Opening a segment** verifies the header, section directory, every CRC32C
  block, and the whole-file BLAKE3 before any accessor exists
  (`crates/spherra-format/src/reader.rs` `SegmentReaderCore::open`). Pairing
  checks the two files of one segment against each other only.

## 3. Product contract

- 768 dimensions; cosine similarity on direction only.
- Rows are identified by an assigned `RowId(u64)`, a dense ordinal starting at 0
  in commit order. The caller keeps its own mapping. Maximum `2^48 - 1`.
- A committed generation is immutable, contains at least one row, and is the
  only thing a search sees.
- Rows are added only through an offline builder that holds an exclusive lock;
  no `Index` may be open on the same directory meanwhile, in any process that
  uses this library.
- Search returns `min(k, N)` hits ranked by an approximate score. **Results are
  not the exact cosine top-k** and are never described as such.
- Every hit carries an interval certified to contain its true full-precision
  cosine score, under the trust boundary in section 7.
- A row is rejected with its position, never silently dropped, if any component
  is non-finite, its norm exceeds the finite FP16 maximum, or
  `direction_unreliable()` is true.

### Non-goals

Deletes, updates, filters, metadata, caller-supplied IDs, concurrent writers,
append while searching, certificate-based candidate selection, coarse routing
(recorded alternative, Appendix A), memory mapping, x86 SIMD, GPU, networking,
distributed operation.

## 4. Public API

```rust
pub type Vector = [f32; 768];
pub const MAX_TRAINING_ROWS: usize = 32_768;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RowId(u64);            // getter `get()`; no public constructor

pub struct CreateOptions {
    pub seed: u64,
    pub validation_rows: Option<usize>, // None = min(4,096, training.len() / 4)
}

pub struct IndexBuilder { /* holds the exclusive lock */ }

impl IndexBuilder {
    /// Absent directory only (section 9). Trains and stages a model.
    /// `training.len()` must be at most `MAX_TRAINING_ROWS`; the caller samples.
    pub fn create(dir: &Path, training: &[Vector], options: CreateOptions) -> Result<Self, Error>;
    /// Existing index only. Uses its stored model; never retrains.
    pub fn append(dir: &Path) -> Result<Self, Error>;
    /// Stages one row. Not durable until `commit` returns Ok.
    pub fn push(&mut self, vector: &Vector) -> Result<RowId, Error>;
    /// Publishes every staged row or none.
    pub fn commit(self) -> Result<CommitReport, Error>;
}

pub struct Index { /* holds a shared lock for its lifetime */ }

impl Index {
    pub fn open(dir: &Path) -> Result<Self, Error>;
    pub fn len(&self) -> u64;
    pub fn generation(&self) -> u64;
    pub fn segment_count(&self) -> u32;
    pub fn search(&self, query: &Vector, options: SearchOptions) -> Result<SearchResult, Error>;
    pub fn certificates(&self, segment: u32) -> Option<SegmentCertificates>;
}

pub struct SearchOptions {
    pub k: usize,
    pub candidate_budget: Option<usize>,
}
```

Result types have private fields, read-only getters, and no public constructor:

- `CommitReport`: `generation()`, `first_row()`, `rows_added()`, `drift()`,
  `cleanup_complete()`.
- `SearchResult`: `generation()`, `candidate_budget()`, `rows_scanned()`,
  `rows_refined()`, `hits() -> &[Hit]`.
- `Hit`: `row()`, `segment()`, `score()` (refined), `interval() -> (f64, f64)`,
  `stored_magnitude() -> f32` (stored FP16 input length promoted to FP32;
  [2026-09-14 amendment](2026-09-14-stored-magnitude-amendment.md)).
- `SegmentCertificates`: `first_row()`, `row_count()`, `primary()`, `refined()`,
  each kind exposing its five terms as getters.
- `DriftReport`: section 10.

`Index` is `Send + Sync`.

`Error` variants: `NotFound` (no committed index), `AlreadyExists`, `IndexBusy`,
`InvalidVector { position }`, `InvalidTraining`, `InvalidOptions`,
`EmptyCommit`, `RowLimit`, `SegmentLimit`, `Corrupt`, `IdentityMismatch`,
`CertificateInvalid`, `Unsupported`, `DescriptorLimit { required, available }`,
`Io`, and `CommitOutcomeUnknown { generation }`.

### Option rules

- `CreateOptions`, checked first and before any allocation, lock, or directory
  change: `training.len() > MAX_TRAINING_ROWS` is `InvalidTraining`. The library
  does not sample; deterministic sampling is the caller's choice. Then the
  validation set must have at least 1 row and the remaining
  training set at least 256 rows; otherwise `InvalidTraining`. This covers an
  explicit `validation_rows` too.
- `SearchOptions`, checked in this order: `k == 0` is `InvalidOptions`; the
  budget is the explicit value or `max(200, 2k)` computed with checked
  arithmetic, and overflow is `InvalidOptions`; a budget below `k` is
  `InvalidOptions`; only then is the budget clamped to `N`.
- `commit` with no staged rows is `EmptyCommit` and publishes nothing.

## 5. Search

For one query:

1. Validate the query with the row rules of section 3; prepare both Q24 lookup
   tables once with `FixedPointScorer::prepare_query`.
2. Split every segment's rows into whole-tile ranges and distribute them across a
   bounded worker pool (default 6; benchmarked at 4, 6, and 8).
3. Each worker scores its tiles' primary codes and keeps its best `B` rows,
   ordered by raw `i64` primary score descending, then `RowId` ascending.
4. Merge worker candidates into the global best `B`. With the same tie rule this
   is identical to sorting every row's primary score.
5. For each candidate, read its 96-byte residual code by positional read from its
   segment's residual file, and compute the refined raw score as the cached
   primary raw score plus the 96 residual lookup entries, with checked addition.
   This is the same integer `score_refined` computes.
6. Rank candidates by refined raw score descending, then `RowId` ascending; keep
   `k`; attach intervals (section 7).

`RowId` of a scored row is its segment's manifest `first_row` plus its position
in the segment. Search never reads stored row records.

Per section 2, budgets from 200 to all rows gave identical recall on both
measured corpora. A larger budget is not guaranteed to improve recall. Residual
codes are not held in memory. Memory per query is `O(workers × B)`.

## 6. Speed and memory

### Acceptance gates

Development machine, k=10, default budget, one query at a time, index open and
warm, AC power:

| Rows | p50 | p99 | Search process peak RSS |
|---|---|---|---|
| 1M | ≤ 150 ms | ≤ 300 ms | recorded |
| 10M | ≤ 1.5 s | ≤ 3 s | ≤ 20 GiB |

Builder-owned memory — everything the builder allocates, excluding the caller's
input vectors — is at most 2 GiB, for both `create` training and staging. It is
bounded by construction:

- **Training** at `MAX_TRAINING_ROWS` = 32,768: each FP32 row matrix is
  100.6 MiB; at most four exist at once (validation/training copies,
  transformed rows, quantizer-training copy, residuals), plus PQ per-row vectors
  of at most 26 bytes per row per subquantizer in training (`assignments`,
  `squared_errors`, `reseeded_rows`, `selected`, `minimum_squared_errors`) —
  under 0.08 GiB even if all 96 subquantizers ran at once. Total about 0.48 GiB.
- **Staging:** at most one segment of originals (65,536 × 3,072 bytes =
  0.19 GiB) plus its codes, the growing manifest, and the drift reservoir
  (at most 0.75 MiB, section 10).

Both are measured as a gate (section 11). A caller passing a 1M-row chunk also
owns about 3.1 GB of input; that is reported separately and not counted.

At about 395,000 rows per second per core, six ideal workers give about 0.42 s
at 1M and 4.2 s at 10M, so the latency gates need roughly 2.8× more per-core
speed. They are targets, not predictions.

### Exactness rule

Every accelerated kernel must return the same `i64` primary score as
`FixedPointScorer::score_primary` for every row. Scores are integer sums, so a
correct kernel matches exactly; there is no tolerance. The certificates,
`codec_id`, and `scorer_version` do not change.

### Stages

Work stops at the first stage that meets the latency gates; later stages are not
built.

1. **Reference.** Section 5 using checked `score_primary` per row from
   tile-decoded codes. Measured.
2. **Safe tile kernel.** Score a full or partial tile coordinate by coordinate
   from the tile bytes into up to 32 accumulators, with no `DirectCode`
   reconstruction and no per-row allocation. Addition is unchecked only after a
   per-query range proof: with `Mp` the largest absolute primary entry recorded
   by `LookupScaleMeasurement`, the kernel is admitted when `768 × Mp` computed in
   `i128` is at most `i64::MAX`, which bounds every partial sum; otherwise the
   checked reference runs. This kernel is the fallback on every CPU.
3. **NEON primary kernel.** Admitted per query only when every primary lookup
   entry lies in `[-2^31, 2^31)`, and only for full 32-row tiles. Each entry is
   biased by `2^31` to `u32` and split into four bytes, giving four 16-entry byte
   tables per coordinate. For each coordinate: load the 16 tile bytes; split low
   nibbles (even lanes) and high nibbles (odd lanes); `vqtbl1q_u8` each byte table
   for both halves; widen-add into per-row per-byte `u16` sums; flush those into
   `u32` sums at most every 257 coordinates. A row's score is
   `Σ_b (u64(sum_b) << 8b)` minus `768 × 2^31`, evaluated in `i64`.

Residual scoring is 96 lookups for `B` rows per query and is not accelerated.

If stage 3 misses the latency gates, stop and report measured results. Coarse
routing (Appendix A) is then a user decision because it adds recall risk.

### Unsafe code

Only stage 3 uses `unsafe`, confined to `crates/spherra-simd/src/neon.rs`,
compiled only on `aarch64`, behind a safe length-checked interface taking plain
byte tables and tile slices. Every other module keeps `deny(unsafe_code)`. If
stage 2 meets the gates, no unsafe code is added. Locking uses `rustix`.

## 7. Certificates and intervals

### Construction

Each segment holds 1 to 65,536 rows and is one certificate block. Its primary
and refined certificates are built with `build_exhaustive_certificate` while the
segment's originals are in memory, and stored in the segment's primary file.

### Trust layers

1. **Stored terms (untrusted).** `spherra-codec` defines `CertificateTerms`, five
   plain `f64` values. `spherra` converts each `StoredErrorCertificate` into it;
   the codec does not depend on the format crate.
2. **Validated terms (arithmetically consistent, provenance unknown).**
   `spherra_codec::validate_certificate_terms(terms, kind)` requires finite
   non-negative values, `eta_transform_dot` and `query_norm_upper` equal to the
   current scorer's, and a stored `epsilon` equal to its recomputation with the
   codec's outward arithmetic. It returns `ValidatedTerms`, whose only numeric
   operation is `arithmetic_interval(raw) -> ArithmeticInterval`, documented as
   not a certificate: a caller who supplies invented terms gets an interval with
   no guarantee.
3. **Bound certificate (certified).** A type private to `spherra` binds
   `ValidatedTerms` to the index generation, segment index, segment id, row
   range, model hash, and score kind. Only `Index::open` creates one, after every
   check in section 8 passes. Only search turns one into a `Hit` interval.

### Intervals

For a hit: primary interval from the primary bound certificate on the primary raw
score; refined interval from the refined bound certificate on the refined raw
score; the reported interval is their intersection. An empty intersection is
`CertificateInvalid`. Epsilons are never added across kinds or segments. The
certified truth value and epsilon formula are those of I4 §6.5.

### Trust boundary

- The reconstruction-error term cannot be recomputed without the originals,
  which are not stored. The library guarantees intervals for indexes it built
  whose files are intact. Checksums and hashes do not prove that a deliberately
  rewritten and re-hashed file reports an honest bound. There is no API that
  accepts certificate terms back.
- Files are verified when opened. The contract assumes index files are not
  modified outside this library while an `Index` is open; residual reads during
  search are not re-verified.

## 8. On-disk layout

```
index/
  LOCK                        permanent, never unlinked
  CURRENT
  model-<blake3>.bin
  manifest-<blake3>.bin
  <segment-id>.primary        segment format v1, unchanged
  <segment-id>.residual       segment format v1, unchanged
  *.tmp
```

All integers little endian. Every container file begins with an 8-byte magic, a
`u16` version, and a `u64` payload length, and ends with a BLAKE3 over all
preceding bytes.

- **CURRENT:** magic, generation `u64`, manifest BLAKE3, CRC32C over the
  preceding bytes.
- **Model:** transform generator version `u16`; seed `u64`; expanded-plan digest
  (BLAKE3 over both rounds' signs and permutations); transform, quantizer, and
  codebook identities; `codec_id`; `scorer_version`; layout; quantizer centers
  and PQ centroids as FP32; validation drift baselines (section 10). Writers
  store `+0.0` for any zero. Restoring rejects any non-finite value or `-0.0`,
  and rejects quantizer centers that decrease within a coordinate, so restored
  bytes are exactly the trained bytes and identities cannot collide on zero
  signs.
- **Manifest:** index id `[u8;16]`, chosen at `create`; generation `u64`;
  previous manifest BLAKE3 (zero for generation 1); model BLAKE3; total rows
  `u64`; segment count `u32`; per segment: segment id `[u8;16]`, first `RowId`
  `u64`, row count `u32`, primary and residual file lengths `u64` and whole-file
  BLAKE3.
- **Segment headers:** `collection_id` is the index id; `segment_id` is the
  manifest segment id.
- **Segment row records** (v1 requires them) are written as `chunk_id = RowId`,
  `document_id = 0`, `put_seq = PutSeq::new(0, RowId)`, and are
  **non-authoritative**: `RowId` comes only from the manifest, and no read path
  uses row records. Radius/flags: FP16 radius in bytes 0–1, zero flags.
- **Limits:** 1 to 4,096 segments; 1 to 65,536 rows per segment; total rows at
  most `2^48 - 1`.

### Open

1. Take the shared lock; `IndexBusy` if a builder holds it.
2. No `CURRENT`: `NotFound`. Otherwise decode `CURRENT` and check its CRC.
3. Read the manifest it names; require its BLAKE3, and require
   `manifest.generation == CURRENT.generation`.
4. Read the model the manifest names and require its BLAKE3. Require support,
   independent of agreement between files: every container version, the
   transform generator version, `codec_id`, `scorer_version`, and layout must
   equal the values compiled into this library (`Unsupported` otherwise).
   Rebuild the
   transform from the seed; require the expanded-plan digest and identity to
   match. Restore both tables through checked constructors; require their
   identities to match.
5. Check manifest semantics, all with checked arithmetic: segment count and each
   row count within limits; segment ids unique; segment 0 starts at row 0; each
   later segment starts exactly where the previous one ends; the sum of row counts
   equals total rows; total rows within the limit.
6. Descriptor check: an open `Index` holds one descriptor per segment (its
   residual file) plus `LOCK`. Read the soft `RLIMIT_NOFILE` with `rustix`; if
   `segment_count + 1 + 64` (64 reserved for the caller) exceeds it, return
   `DescriptorLimit`. The library never changes the limit. At launchd's default
   soft limit of 256 (processes started outside a shell), this admits 191
   segments — 12.5M rows of full segments. Shells may set a higher limit; this
   machine's shell reports 1,048,576 (probed 2026-09-13).
7. For each entry: open both files by the names derived from its segment id;
   require each file's length and whole-file BLAKE3 to equal the manifest;
   require header `segment_id`, `collection_id`, and `row_count` to equal the
   entry and the index id; require every representation identity to equal the
   model; pair the files.
8. Load primary codes tile by tile, certificates and two-byte-per-row stored
   magnitudes into owned memory (validate finite nonnegative FP16, including +0
   underflow, as specified by the magnitude amendment), then
   close the primary file before opening the next segment; keep only residual
   files open for positional reads. `spherra-format` provides this as a
   consuming `PairedSegmentReaders::into_residual` that drops the primary reader
   and returns an opaque residual reader that keeps the pairing already
   verified; it cannot be built from an unpaired file. Peak descriptors during open are
   `segment_count + 2`.
9. Validate both certificates of every segment (section 7) and bind them.

Any failure is a structured error; no partial `Index` is returned.

## 9. Directory states, commit, and recovery

### States

- **Absent:** no `CURRENT`. The directory may hold `LOCK` and unreferenced files
  from an interrupted `create`. `open` and `append` return `NotFound`.
- **Committed:** `CURRENT` names a valid generation.

`create` requires Absent (`AlreadyExists` otherwise), takes the exclusive lock,
deletes unreferenced files, and proceeds. `append` requires Committed.

### Builder lifecycle

Take the exclusive lock non-blocking (`IndexBusy` if held). Stage rows; each
time 65,536 rows are staged, encode and stage one segment and release those
originals. Any staging or verification failure poisons the builder: later calls
return the original error and nothing is published.

### Commit

1. `EmptyCommit` if no rows are staged. Stage the final partial segment.
2. For every new file (model on `create`, segments, manifest): write to a unique
   `.tmp`, reopen and verify it, `sync_all`, rename to its final name, and
   `sync_all` the directory.
3. Write and `sync_all` `CURRENT.tmp`, then rename it over `CURRENT`.
4. `sync_all` the directory.
5. Best-effort: delete files referenced by no committed manifest. Release the
   lock.

### Outcomes

| Point of failure | Result | Directory state |
|---|---|---|
| Before step 3's rename, or the rename itself returns an error | `Err` (not published) | unchanged: Absent or previous generation |
| Step 3's rename succeeds, step 4 fails | `CommitOutcomeUnknown { generation }` | new generation visible now; survival of a crash not guaranteed |
| Step 4 succeeds | `Ok(CommitReport)` | new generation |
| Step 5 fails | still `Ok`, with `cleanup_complete() == false` | new generation; the next builder retries cleanup |

The caller resolves `CommitOutcomeUnknown` by opening the index and comparing
`generation()`; it must not retry the same rows before doing so. A crash at any
point leaves Absent, the previous generation, or the new generation — never a
mix. Corruption of a committed file is an error, never a silent fallback to an
older generation.

Durability is qualified on local APFS only. The section 2 probe shows directory
`sync_all` succeeds; injected-failure and process-kill tests are the evidence.
Physical power loss is not claimed.

## 10. Build, training, and drift

**Training (`create`).** Reject more than `MAX_TRAINING_ROWS` rows before
anything else (section 4). Split `training` deterministically by seed into a
validation set of the resolved `validation_rows` and a disjoint training set
(section 4 rules). Build the transform from the seed, train the quantizer on the
transformed training set, and train the PQ codebook on its residuals, as
`CodecFormatRun::prepare` does. Training rows are not indexed unless pushed.

**Encoding.** Rows within a segment are encoded in parallel. Codes are a function
of the model and the row only, and are tested to be identical for any batching
and commit split.

**Drift baselines.** Over the validation set the model stores the p50, p95, and
p99 of per-row primary and refined reconstruction L2 error, and the fraction of
transformed coordinates outside the quantizer's outer centers.

**Drift report per commit.** The same statistics over a bounded sample of the
committed rows: a deterministic reservoir of at most 65,536 rows per commit,
selected by BLAKE3 of the `RowId` and index id, holding three `f32` values per
row (at most 0.75 MiB). Percentiles are exact over the sample; the report states
the sample size. The statistics are computed per commit only and never
accumulated across segments or generations, plus
`warned()`, true when refined p95 exceeds 1.25 × the baseline p95 or more than 5%
of committed rows exceed the baseline refined p99. A commit of fewer than 1,000
rows reports `insufficient_sample()` instead of a warning. These heuristics
detect reconstruction change, not recall loss.

## 11. Verification

**Soundness gates, never allowed to fail:**
- every hit's FP64 truth lies inside its interval;
- every accelerated kernel equals `score_primary` exactly, and cached-primary
  refinement equals `score_refined` exactly;
- after any injected failure the directory is Absent, the previous generation,
  or the new generation, and the returned outcome matches section 9's table;
- malformed files, and correctly checksummed files that violate any section 8
  semantic rule, are rejected with structured errors and no panic;
- a transform whose expanded plan differs from the model is rejected;
- a stored epsilon that differs from its recomputation is rejected;
- an unsupported container version, generator version, `codec_id`,
  `scorer_version`, or layout is rejected even when every file agrees;
- restored tables containing `-0.0`, a non-finite value, or decreasing quantizer
  centers are rejected;
- more than `MAX_TRAINING_ROWS` training rows are rejected with no directory
  change;
- no public API constructs a `Hit`, a bound certificate, or a `RowId`
  (compile-fail tests).

**Algorithm equality.** The local index returns exactly the hits and raw scores
of a plain reference search written in the test — checked scalar scoring, full
sorts, same budget and tie rule — over the index's own restored model and codes.

**Quality and performance, recorded against gates:**
- **Historical recall**, archived SciFact bytes and generated-correlated 20k:
  recall@10 at most 0.01 below the pre-implementation `codec-format` measurement
  on the same bytes and budget. The models differ because `create` holds out
  validation rows, so exact equality is not expected.
- **1M recall**, generated-correlated: recorded against a streaming exact
  reference produced before the index is measured; no historical value exists.
- Latency and memory gates of section 6; open time, build throughput, and drift
  behavior on shifted data recorded.
- **Builder memory gate:** a child process runs `create` with exactly
  `MAX_TRAINING_ROWS` rows, then pushes and commits 65,537 rows (one full and one
  partial segment). Its peak RSS minus its RSS before allocating input, minus the
  input bytes, is at most 2 GiB.
- **Descriptors:** the open process's descriptor count is recorded at 1M and
  10M and equals `segment_count + 1` plus the process baseline.

A streaming exact oracle keeps only top-k heaps per query, so a 1M reference does
not need all normalized rows in memory.

## 12. Alternatives considered

- **Refine every row with resident residuals.** Rejected: identical recall to
  budget 200 on both measured corpora, with 0.96 GB more memory at 10M.
- **One segment rewritten on every append, with certificates merged.** Rejected:
  rewrites the whole index per append and needs a certificate-merge rule whose
  refined serving term depends jointly on two maxima.
- **A format v2 transform-seed section.** Rejected in favor of the model file,
  which also carries the expanded-plan digest and drift baselines without
  changing segment bytes.
- **Coarse routing.** Deferred; outlined in Appendix A.
- **Certificate-driven candidate selection.** Failed its experiment.

## 13. Open questions for the user

- If the 10M latency gate is missed after stage 3, is multi-second search
  acceptable, or is coarse routing's recall risk acceptable?
- Which real embedding datasets define acceptable recall beyond SciFact?
- Expected append batch sizes; very small commits consume the segment limit, and
  at the default descriptor limit an index opens with at most 191 segments unless
  the caller raises `ulimit -n`.

## 14. Project guidance changes requiring user approval

- Rewrite the project guide project paragraph, product contract, status, and next
  step for the local library, and mark the R7/I4 frozen decisions superseded for
  local scope. The frozen rule that residuals are candidate-only SSD data is
  unchanged by this design.
- If stage 3 is built, correct the project guide caveat that says the workspace
  contains no unsafe code.

## Appendix A. Coarse routing alternative

Considered only if stage 3 misses the 10M gate and the user accepts added recall
risk.

**Design.** The model gains 4,096 spherical centroids (FP32, about 12.6 MB) over
transformed directions, trained deterministically at `create`. Within each
segment, rows are physically ordered by assigned centroid; a per-segment routing
sidecar stores `4,097` `u32` list offsets and is named by hash in the manifest.
Because physical order no longer follows `RowId`, stored row records become
authoritative for `RowId` and are validated on open. Search picks the `nprobe`
nearest centroids (benchmarked at 16, 32, 64, 128, 256), scans only those lists
with the same kernels (masking lanes outside a list), and refines the best `B`
with the same residual path. Certificates, intervals, and the trust layers are
unchanged: they bound the scores of returned rows and say nothing about rows in
unvisited lists. Results report `selection = routed` and visited list and row
counts.

**Append** assigns new rows with the existing centroids; new segments get their
own sidecars and certificates; commit and recovery are unchanged.

**Failure modes.** Silent recall loss when a true neighbor's list is not probed,
worse after distribution shift or list skew; certificates cannot detect it.
Balanced lists at `nprobe = 64` would visit about 156,250 of 10M rows — an
arithmetic estimate, not a result.

**Implementation outline and gates.**
1. Centroid training, assignment, sidecar encoding, and open validation of offsets
   and `RowId` records. Gate: every row belongs to exactly one list; probing all
   lists returns exactly the full-scan hits.
2. Probe selection and masked tile schedules. Gate: scored rows' raw scores equal
   the reference; no selected row skipped or duplicated.
3. Recall versus full scan at each budget and `nprobe` on the archived corpora
   and on appended shifted data. Gate: a user-agreed recall floor.
4. Latency and list-skew qualification. Gate: a measured improvement that
   justifies the added recall risk.
