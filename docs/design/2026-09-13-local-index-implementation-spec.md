# Spherra local index implementation specification

Status: **approved at revision 4; approved by the user 2026-09-13**
Date: 2026-09-13
Governing design: `2026-09-13-local-index-design.md` (cited below as D§n)

## 1. Purpose and authority

This specification turns the local index design into an ordered delivery
sequence with crate ownership, interfaces, tests, and gates. If it conflicts
with the design, the design wins and the conflict is recorded before code
changes.

## 2. Starting state

Present and passing: `spherra-domain`, `spherra-simd` (scalar Hadamard only),
`spherra-codec` (transform, int4, PQ96, fixed-point scorer, certificates),
`spherra-format` (segment v1 writer, checked readers, pairing),
`spherra-testkit` (corpora, exact oracle, harness, prune-rate measurement,
`M1_CODEC_ID`), `spherra-bench` (`codec-format`, `certify`, `prune-rate`);
129 tests.

Absent: see D§2 code gaps.

## 3. Fixed constraints

- Rust 1.88.0, edition 2024; `deny(unsafe_code)` everywhere except
  `crates/spherra-simd/src/neon.rs` if Task 12 runs.
- Dependency direction, as enforced by `scripts/check_dependency_policy.py`:
  `spherra-codec` depends on `spherra-domain` and `spherra-simd`;
  `spherra-format` depends on `spherra-domain` only. The new `spherra` crate
  depends on `spherra-domain`, `spherra-codec`, and `spherra-format`;
  `spherra-bench` may depend on `spherra`. The policy script today discovers
  only names starting with `spherra-` (`check_dependency_policy.py:52`) and
  skips everything else (line 75), so it would ignore the new crate. Task 5
  changes discovery to `name == "spherra" or name.startswith("spherra-")`, moves
  the check into a pure function `invalid_edges(metadata)` called by `main`,
  and adds exactly the edges `(spherra, spherra-codec)`,
  `(spherra, spherra-format)`, and `(spherra-bench, spherra)`. The existing
  rule that every package may depend on `spherra-domain` then covers
  `(spherra, spherra-domain)`.
- The only new direct dependency is `rustix` (already in `Cargo.lock`, licensed
  under an allowed alternative) with features `fs` (`flock`) and `process`
  (`getrlimit`); `cargo deny check` must pass.
- Segment format v1 bytes do not change.
- Quality evidence uses one archived SciFact `.f32`, identified by BLAKE3 and
  stored outside Git with its descriptor committed.
- AddressSanitizer is unavailable on this machine (project guide); fuzz runs use
  `-s none` and are recorded as such.

## 4. Strategy

Correctness before speed. Tasks 1–9 deliver a complete, durable, correct index
using the checked scalar scorer. Tasks 10–12 add speed in stages and stop at the
first stage that meets D§6. Task 13 qualifies quality; Task 14 updates guidance.

## 5. Crate responsibilities

| Crate / file | Responsibility |
|---|---|
| `spherra-codec/src/identity.rs` (new) | `CODEC_ID`, moved from `spherra-testkit`; the testkit re-exports it under its current names with identical bytes |
| `spherra-codec/src/int4.rs` | `QuantizerTable::from_centers` |
| `spherra-codec/src/pq96.rs` | `Pq96Codebook::from_centroids` |
| `spherra-codec/src/transform.rs` | `GENERATOR_VERSION`, `TransformPlan::expanded_digest` |
| `spherra-codec/src/certificate.rs` | `CertificateTerms`, `validate_certificate_terms`, `ValidatedTerms::arithmetic_interval`, `ArithmeticInterval` (D§7 layers 1–2) |
| `spherra-codec/src/scorer.rs` | `refine_from_primary`; read-only access to primary lookup entries and `LookupScaleMeasurement` |
| `spherra-codec/src/kernel.rs` (new) | stage 2 tile kernel, range proof, stage 3 byte-table construction and dispatch |
| `spherra-simd/src/neon.rs` (new) | stage 3 kernel (Task 12 only) |
| `spherra-format/src/reader.rs` | `PrimaryFileReader::primary_tile` returning all codes of one tile; consuming `PairedSegmentReaders::into_residual` returning an opaque paired residual reader (D§8 step 8) |
| `spherra/src/{lib,error,model,manifest,fs,lock,builder,open,search,drift,certified}.rs` | public API and everything in D§4–D§10; `certified.rs` holds the private bound certificate (D§7 layer 3); `fs.rs` holds `FaultyFs` under `cfg(test)` |
| `spherra-testkit/src/oracle_stream.rs` (new) | streaming exact oracle |
| `spherra-bench/src/main.rs` | `index`, `latency`, and `build-memory` subcommands |
| `scripts/check_dependency_policy.py`, `scripts/test_check_dependency_policy.py` (new), `scripts/ci.sh` | discovery of `spherra`, new edges, negative tests run by CI |

## 6. Stable interfaces

### 6.1 Public API
Exactly D§4, including its option rules. Additions require a design change.

### 6.2 Kernel contract
```rust
// spherra-codec, public so `spherra` can call it
pub fn score_tile_primary(
    query: &PreparedScorerQuery,
    tile: &[u8],
    lanes: usize,
    out: &mut [i64; 32],
) -> Result<KernelPath, KernelError>;
```
`tile` must be exactly `768 × 16` bytes of one `TiledSoa32` tile and `lanes` must
be in 1..=32; otherwise the function returns `KernelError` without writing to
`out`. On success, for every `r < lanes`,
`out[r] == score_primary(query, code_r).raw()`, and `KernelPath` reports which
kernel ran. Dispatch: stage 3 if built, on `aarch64`, `lanes == 32`, and every
primary entry in `[-2^31, 2^31)`; else stage 2 if `768 × Mp ≤ i64::MAX` computed
in `i128`; else the checked reference.

### 6.3 Refinement contract
`refine_from_primary(query, primary_raw, residual_code) -> i64` equals
`score_refined(query, primary, residual).raw()` whenever
`primary_raw == score_primary(query, primary).raw()`, using checked addition.

### 6.4 Filesystem adapter
A crate-private trait in `spherra/src/fs.rs` covering create, write,
`sync_all`, rename, directory `sync_all`, read, remove, and list, with a real
implementation and a `FaultyFs` compiled only under `cfg(test)` that fails,
short-writes, or aborts at any numbered call. `FaultyFs` does not live in
`spherra-testkit`: that would need `spherra` to dev-depend on a crate that
depends on `spherra`, and the two compiled copies of the trait would be distinct
types.

### 6.5 Containers and manifest semantics
D§8 byte layouts. Decoders bound every length before allocating. Open performs
D§8 steps 1–9 in order; each support rule of step 4, each semantic rule of
step 5, the descriptor check of step 6, and each per-entry rule of step 7 is a
separately tested rejection.

### 6.6 Commit outcomes
D§9 states and outcome table are the contract for every builder test.

## 7. Tasks

Each task ends with `cargo test --workspace`, `cargo clippy --workspace
--all-targets -- -D warnings`, and a commit. Tests named in a task are written
before the implementation.

### Task 1 — Evidence protocol
- Archive the SciFact `.f32` at BLAKE3 `b2e549ce…`; commit its descriptor with
  that hash.
- Record the **historical recall reference**: `spherra-bench codec-format` on it
  and on generated-correlated 20k, seed 20260804, 200 queries, budgets 20, 200,
  1,000, and all rows. Commit the JSON. This is an aggregate recall reference
  only; it contains no hits and is not used for algorithm equality.
- **Gate:** results reproduce D§2's table.

### Task 2 — Model restoration and codec identity
- Tests: train then restore preserves identity, table values, `encode` output on
  1,000 rows, and prepared lookup entries for 20 queries, for both tables;
  restored table bytes equal trained bytes; wrong length, a non-finite value,
  `-0.0` in either table, and a quantizer coordinate whose centers decrease are
  each rejected; the expanded digest
  has a golden value for a fixed seed and changes when any sign or permutation
  entry changes; `CODEC_ID` bytes equal the current `M1_CODEC_ID`, and
  `spherra-bench codec-format` output keeps the same `codec_id` hex.
- Implement the `CODEC_ID` move, `from_centers`, `from_centroids`,
  `GENERATOR_VERSION`, and `expanded_digest`.
- **Gate:** a restored model scores identically to the trained one.

### Task 3 — Certificate layers 1–2
- Tests: `validate_certificate_terms` on a built certificate's terms succeeds and
  its `arithmetic_interval` equals `BlockCertificate` bounds for the same score;
  a changed epsilon, transform term, or query-norm term, a negative value, and a
  non-finite value are each rejected; the intersection of primary and refined
  arithmetic intervals contains the FP64 truth for every row of a 3,000-row
  generated block over 20 queries; an empty intersection is reported.
- Implement. **Gate:** zero enclosure failures.

### Task 4 — Cached-primary refinement
- Test: over every row of the archived SciFact and generated-correlated 20k, 20
  queries each, `refine_from_primary` equals `score_refined` and
  `score_prepared_candidate`.
- Implement. **Gate:** zero differences.

### Task 5 — Containers, manifest semantics, filesystem adapter, locking
- Tests: round trips for `CURRENT`, model, and manifest; truncated,
  oversized-length, bad-magic, bad-version, bad-CRC, and bad-hash inputs rejected
  without panic. Correctly checksummed but semantically invalid manifests, each
  rejected with its own error: generation differing from `CURRENT`; duplicate
  segment id; first segment not at row 0; a gap; an overlap; row-count sum not
  equal to total; zero segments; more than 4,096 segments; a segment of 0 or more
  than 65,536 rows; total above `2^48 - 1`; overflow in `first_row + count`.
  `flock` shared/exclusive exclusion across two processes.
- Dependency policy tests (`python3 -m unittest scripts/test_check_dependency_policy.py`,
  added to `scripts/ci.sh`), each on synthetic `cargo metadata` JSON passed to
  `invalid_edges`: the workspace's real edges pass; each of
  `spherra-codec -> spherra`, `spherra-format -> spherra-codec`,
  `spherra -> spherra-testkit`, and `spherra -> spherra-bench` is reported; a
  package named `spherra` is discovered while a non-workspace package named
  `spherra` is not.
- Implement D§8 encodings and step-5 checks, the D§6.4 adapter, `rustix`
  locking, and the policy script change of §3.
- Add a fuzz target over the three decoders; run 60 s each with `-s none`.
- **Gate:** no crash; `cargo deny check` and dependency policy pass.

### Task 6 — Tile decode
- Test: `primary_tile` equals `primary_code` for every row, including a partial
  final tile.
- Implement. **Gate:** passes on 1, 31, 32, 33, and 65,536 rows.

### Task 7 — Builder
- Tests:
  - rows fail with their position when non-finite, above FP16 range, or
    `direction_unreliable()`;
  - identical row codes for 70,000 rows pushed in one commit versus split across
    commits of 1, 500, and 69,499; for every split, each segment's certificates
    equal certificates built directly over exactly that segment's rows; segments
    never exceed 65,536 rows;
  - training rules: SciFact's 1,295-row calibration split with default options
    resolves to 323 validation and 972 training rows and succeeds; an explicit
    `validation_rows` leaving fewer than 256 training rows, zero validation rows,
    and an empty training slice each return `InvalidTraining`; exactly
    `MAX_TRAINING_ROWS` rows succeed; `MAX_TRAINING_ROWS + 1` rows return
    `InvalidTraining` and leave a nonexistent target directory nonexistent (no
    `LOCK`, no files), showing the check precedes any filesystem call or
    training allocation;
  - `commit` with no rows returns `EmptyCommit`;
  - a drift report warns on rows from a different generated distribution and not
    on more rows from the training distribution; a commit under 1,000 rows
    reports `insufficient_sample()`;
  - the drift reservoir is identical for the same rows regardless of batching,
    holds at most 65,536 rows for a 200,000-row commit spanning four segments,
    and equals exact percentiles when the commit has at most 65,536 rows.
- Implement `create`, `append`, `push`, and `commit` per D§9–D§10, using the
  adapter.
- **Gate:** all pass.

### Task 8 — Open, bound certificates, and reference search
- Tests:
  - build, commit, open, search returns hits whose rows and raw scores equal an
    in-test reference search — checked `score_primary` over every row, full sort
    by raw score then `RowId`, keep `B`, checked `score_refined`, full sort, keep
    `k` — over the index's own restored model and decoded codes, on archived
    SciFact and generated 20k;
  - option rules in D§4 order: `k == 0`; budget overflow; budget below `k`;
    clamping to `N`; `k > N`;
  - ties resolve by `RowId`; hits across segment boundaries carry correct `RowId`
    and `segment`;
  - open rejects, each with a structured error: missing `CURRENT` (`NotFound`);
    wrong model hash; a transform whose expanded digest differs; a segment file
    whose length or hash differs from the manifest; a segment from another index
    (`collection_id`); header `segment_id` or `row_count` differing from the
    entry; a truncated segment; a changed certificate term; with every file
    consistent and correctly hashed, an unsupported container version,
    generator version, `codec_id`, `scorer_version`, or layout (`Unsupported`);
  - descriptors: with a soft `RLIMIT_NOFILE` lowered in a child process below
    `segment_count + 65`, open returns `DescriptorLimit` with the required and
    available counts; after a successful open, the process holds exactly
    `segment_count + 1` more descriptors than before (counted from `/dev/fd`),
    and residual reads through `into_residual` readers still succeed and match
    the paired reader's rows after every primary file is closed;
  - compile-fail doctests: no public construction of `Hit`, `RowId`,
    `SegmentCertificates`, or `CommitReport`;
  - 8 threads searching one index match one thread; worker counts 1, 4, 6, and 8
    give identical results.
- Implement D§5 with the checked reference kernel, D§8 open steps 4 and 6–9, and D§7
  layer 3.
- **Gate:** equality with the in-test reference; zero enclosure failures.

### Task 9 — Directory states and recovery
- Tests with `FaultyFs`, failing at every numbered adapter call during `create`
  and during `append`:
  - the returned outcome and resulting state match the D§9 table exactly;
  - a `create` that fails before the `CURRENT` rename succeeds leaves Absent,
    and a following `create` succeeds and removes leftovers; failures after
    that rename follow the D§9 table like any commit;
  - `create` on a Committed directory returns `AlreadyExists`; `append` and
    `open` on Absent return `NotFound`;
  - a rename error returns `Err` with `CURRENT` unchanged;
  - a directory-sync failure after the rename returns `CommitOutcomeUnknown`, and
    `open` then shows the new generation;
  - a cleanup failure returns `Ok` with `cleanup_complete() == false`, and the
    next builder completes cleanup;
  - a poisoned builder publishes nothing.
- Also: a child process killed mid-commit leaves Absent, previous, or new; a
  builder cannot start while an `Index` is open; exceeding 4,096 segments returns
  `SegmentLimit`.
- Implement remaining D§9 behavior. **Gate:** all pass.

### Task 10 — Streaming oracle, 1M reference, latency harness, stage 1
- Test: the streaming oracle returns the same top-100 as `ExactOracle` on
  archived SciFact and generated 20k.
- Add a chunked generated source (1M-row chunks, per-chunk seeds; deterministic
  concatenation tested) and produce the **1M recall reference**: streaming-oracle
  top-100 for 200 generated queries over generated-correlated 1M. Commit its
  BLAKE3 and the generator parameters.
- Add `spherra-bench latency`: create from a training sample, append chunks,
  open, run 1,000 queries at k=10 after 50 warm-up queries; report p50, p99, peak
  RSS, open time, build throughput, descriptors held, and kernel used.
- Add `spherra-bench build-memory`, run as a child process: record RSS, allocate
  and record input bytes, `create` with exactly `MAX_TRAINING_ROWS` rows, push and
  commit 65,537 rows; report peak RSS (`getrusage` maximum RSS via
  `/usr/bin/time -l`) minus pre-input RSS minus input bytes. **Gate:** at most
  2 GiB (D§6, D§11).
- Measure at 1M and 10M. **Gate check:** if every D§6 gate is met, skip Tasks
  11–12.

### Task 11 — Stage 2 tile kernel
- Tests (proptest): random prepared queries and tiles, `lanes` 1..=32, satisfy
  the kernel contract; a wrong tile length and `lanes` outside 1..=32 return
  `KernelError` and leave `out` untouched; a query with `768 × Mp > i64::MAX`
  (constructed tables) reports the checked path; full-corpus equality on archived
  SciFact and generated 100k over 20 queries.
- Implement; re-run Task 10. **Gate check:** if met, skip Task 12.

### Task 12 — Stage 3 NEON kernel
- Tests (proptest): the NEON byte-lane sum equals a scalar byte-lane sum for
  random byte tables and tiles, including bytes 0 and 255 in every lane and 768
  coordinates of maximal values (flush boundary); the kernel contract with
  entries at `-2^31` and `2^31 - 1`; an entry outside that range reports stage 2;
  full-corpus equality; a build check that `unsafe` outside `neon.rs` is
  rejected.
- Implement per D§6 stage 3; re-run Task 10.
- **Gate:** zero differences, an independent line-by-line safety review of
  `neon.rs`, and a measured gain over stage 2; otherwise remove it and keep
  stage 2. If D§6 is still unmet, stop and report per D§13.

### Task 13 — Quality qualification
- `spherra-bench index` through the public API only.
- **Algorithm equality:** every hit equals the Task 8 reference search on the
  same index, for archived SciFact, generated 20k, and generated 1M.
- **Historical recall:** archived SciFact and generated 20k, recall@10 at most
  0.01 below Task 1 for the same bytes and budget.
- **1M recall:** recorded against the Task 10 reference.
- **Soundness:** interval enclosure for every hit on all three corpora.
- **Gate:** zero equality differences and zero enclosure failures; a historical
  recall drop above 0.01 stops the task and is reported to the user.

### Task 14 — Guidance and documentation
- After user approval, apply D§14.
- Rewrite `README.md`: API, measured recall and latency, kernel per CPU, interval
  meaning and trust boundary, drift heuristics, append limits, directory states,
  durability scope.
- Add superseded notices to the two 2026-08-04 specifications.

## 8. Verification architecture

Soundness gates, algorithm equality, and recorded quality gates are those of
D§11. Techniques: unit tests and proptest; differential tests between every
kernel and `score_primary`; `FaultyFs` fault injection at every filesystem call;
a child-process kill test; decoder fuzzing with `-s none`; golden fixtures for
model, manifest, and `CURRENT` bytes; correctly checksummed semantically invalid
fixtures; compile-fail doctests for construction rules; a multi-process lock
test.

## 9. Deferred work

Coarse routing (D Appendix A), residual SIMD, x86 SIMD, memory mapping, per-row
certificates, caller IDs, deletes, filters, compaction or segment merging,
qualification on filesystems other than APFS.

## 10. Status and maintenance

Update the project guide status section after each task. The D§14 content changes
were approved and applied on 2026-09-13.

## 11. Approval record

- User: APPROVE at revision 4 (2026-09-13). Also approved by the user on 2026-09-13: the D§14 project guide changes (applied), and unsafe code confined to `crates/spherra-simd/src/neon.rs` if Task 12 runs.
