# Spherra v1 implementation specification

> **Superseded for current implementation (2026-09-13).** This specification
> records the stopped distributed-database direction. Follow the approved
> [local index design](2026-09-13-local-index-design.md) and
> [local implementation specification](2026-09-13-local-index-implementation-spec.md).
> Its completed codec/format work remains historical evidence; its later
> database milestones are not the current plan.

Status: **I4 technical specification approved; Spherra naming amendment applied on 2026-08-04**
Revision: I4 plus naming amendment
Date: 2026-08-04
Governing architecture: [`2026-08-04-polar-lsm-router-design.md`](./2026-08-04-polar-lsm-router-design.md)

## 1. Purpose and authority

This specification turns the approved R7 production architecture into an executable delivery sequence. It defines milestone boundaries, crate ownership, dependency direction, verification gates, and the first implementation slice. It does not reopen the architecture.

If this document conflicts with the R7 architecture, R7 wins. A material architectural change requires a new design revision and renewed approval. Benchmark-selected constants may change when the R7 acceptance harness provides evidence, but the evidence and decision must be recorded before code or durable formats are changed.

## 2. Starting state

The workspace currently contains:

- The approved R7 architecture specification.
- A C++20 scalar codec/layout experiment.
- A Python synthetic retrieval-quality experiment.
- A living project guide.

The workspace does not yet contain:

- A Git repository or commit history.
- A Rust workspace, production crates, or dependency lockfile.
- Production transform, direct-int4, PQ96x8, fixed-point scoring, or certified-bound code.
- A durable log, Raft state machine, searchable memtable, segment format, truth index, server, or client.
- CI, release packaging, or production deployment files.

The experiments settle only the architectural choice between recursive hyperspherical serving and a direct compressed direction, plus provisional layout candidates. They are not a production scorer oracle: the Python experiment renormalizes decoded vectors, and the C++ benchmark does not implement the approved transform, residual, or fixed-point comparison scale.

## 3. Fixed implementation constraints

The following are not implementation choices:

- The product, repository, public command, and first-party Rust package prefix are **Spherra** / `spherra` / `spherra-*`; PolarLSM and PolarRouter remain subsystem names.

- Collections use 768-dimensional vectors.
- The first development machine is an Apple M1 Pro with 32 GiB unified memory and a 1 TB SSD.
- The local target is 10 million chunks; 25 million is a no-SLO stretch target; one billion requires a distributed cluster.
- Each ingested chunk owns one vector and carries parent-document metadata.
- A successful write acknowledgement means the row is visible to every later session-consistent search without waiting for flush.
- The transform has exactly two independently seeded rounds. Each round performs sign flips, one global permutation, and six normalized 128-point fast Hadamard transforms.
- The provisional primary representation is a four-byte radius/flags word plus a 384-byte direct-int4 direction code.
- The provisional residual is PQ96x8: 96 one-byte subquantizer codes over 96 eight-dimensional subvectors. It is candidate-only SSD/page-cache data, never part of the required resident scan set.
- Vectors below `min_norm_epsilon` remain in the non-routable radius/metadata partition and are not passed to transform, direct-int4, PQ, cosine, cone, or hybrid-vector code paths.
- Reject NaN, infinity, and norms above the largest finite FP16 value before encoding; the stored FP16 radius must remain finite.
- Primary-plus-residual scoring is `dot(T(normalize(q)), p + e)`. The reconstructed value is not renormalized and receives no learned affine correction.
- Fixed-point lookup entries and int64 accumulation define serving comparison behavior.
- Search begins with filtered scans. HNSW is optional, immutable, cell-local, and benchmark-gated. There is no global HNSW.
- Logical writes are Raft replicated. Every replica applies committed operations into searchable FP32 memtables. Leaders alone build immutable files, physically replicate them, and commit `MANIFEST_INSTALL`.
- Primary-key truth, document tombstones, snapshot validation, refill, and certified bounds are correctness requirements rather than later optimizations.

## 4. Implementation strategy

Use a risk-first sequence:

1. Prove the transform, codec byte accounting, candidate-only residual access, scorer conformance, and durable framing in scalar Rust.
2. Establish a one-member OpenRaft commit/apply path whose state machine is the only publisher of searchable writes.
3. Complete the single-node insert-to-restart vertical slice using searchable FP32 memtables, immutable scan segments, exact truth snapshots, and committed manifest installation.
4. Add filtered query planning, service APIs, clients, and operational limits.
5. Extend the existing seams to multi-node logical replication and physical segment installation.
6. Add multi-vshard routing, movement, and splits.
7. Add cell-local HNSW only if scan evidence reaches its entry gate.

The implementation is a modular monolith. Crates define ownership and dependency direction; deployment initially uses one `spherra serve` process rather than microservices.

## 5. Workspace and dependency boundaries

The planned workspace is:

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml

crates/
  spherra-domain/       IDs, records, sequences, validation, shared errors
  spherra-format/       explicit durable bytes, checksums, manifests, segment readers
  spherra-simd/         scalar kernels and isolated architecture-specific SIMD
  spherra-codec/        transform, direct int4, PQ96x8, scoring, certificates
  spherra-memtable/     searchable FP32 mutable/frozen state and visible watermark
  spherra-lsm/          truth storage, vector segments, flush, compaction, recovery
  spherra-raftlog/      append-only segmented Raft log and durability adapter
  spherra-consensus/    owned OpenRaft adapters and state-machine bridge
  spherra-router/       cells, filters, pruning, scan/HNSW candidate sources
  spherra-proto/        public and internal protobuf contracts
  spherra-server/       Tokio process composition, tonic services, axum admin surface
  spherra-cli/          administration and diagnostics
  spherra-testkit/      corpora, exact oracle, FaultyFs, deterministic clocks/networks
  spherra-bench/        reproducible codec, storage, recall, latency, and memory harnesses

clients/
  python/             NumPy-first client
```

The normative dependency DAG is one-way. `spherra-domain` is a permitted normal dependency of every other `spherra-*` crate. Every additional project-crate edge must appear below; an edge means the left crate may be a normal dependency of the right crate:

```text
spherra-domain -> spherra-simd
spherra-domain + spherra-simd -> spherra-codec
spherra-domain -> spherra-format
spherra-domain -> spherra-memtable
spherra-domain -> spherra-raftlog
spherra-domain + spherra-codec + spherra-format + spherra-memtable -> spherra-lsm
spherra-domain + spherra-raftlog + spherra-lsm -> spherra-consensus
spherra-domain + spherra-codec + spherra-lsm -> spherra-router
spherra-domain -> spherra-proto
spherra-domain + spherra-codec + spherra-format -> spherra-testkit
spherra-codec + spherra-format + spherra-testkit -> spherra-bench
spherra-router + spherra-consensus + spherra-proto -> spherra-server + spherra-cli
```

OpenRaft, tonic, memmap2, and other third-party implementation types must not leak into domain, codec, format, LSM, or router public interfaces.

`spherra-format` treats transform, codec, quantizer, PQ-codebook, scorer, and layout identities as opaque fixed-width values. It does not depend on `spherra-codec`. The M0 dependency-policy check allows the universal `spherra-domain` rule plus exactly the additional edges listed above and rejects every other normal/build project edge.

## 6. Stable project-owned interfaces

### 6.1 Codec boundary

`spherra-codec` owns:

- `TransformSpec` and deterministic transform identity.
- Scalar transform and inverse reference implementations.
- Direct-int4 training, packing, decoding, and primary scoring.
- PQ96x8 training, encoding, residual decoding, and reranking.
- Fixed-point tables, int64 accumulation, and outward-rounded error terms.
- Exhaustive block certificate construction while FP32 originals are present.

The codec API distinguishes primary scan from candidate rerank. A primary scan receives no residual handle. Only candidate rerank can request a residual row.

### 6.2 Durable-format boundary

`spherra-format` owns explicit little-endian encoding. Durable files must not use a generic serializer as their compatibility contract.

Every durable file includes:

- Magic, major/minor format version, file kind, and header length.
- Collection dimension and relevant codec/scorer/layout identities.
- Section directory with checked offsets, lengths, alignment, and row counts.
- CRC32C for independently accessed blocks.
- BLAKE3 identity for complete immutable files.

Each data-section directory entry points to a paired CRC-table section containing one little-endian u32 CRC32C per logical block in order. The final partial block checksum covers only logical bytes. CRC tables are themselves covered by the whole-file BLAKE3 identity.

Sealed files are read-only. M1 implements checked positional reads only. The reader abstraction consists of a private `SegmentReaderCore` plus public capability readers. A later milestone may add a read-only memmap source only inside `SegmentReaderCore` in one named, audited unsafe module after lifecycle and SIGBUS tests exist. No other format or engine code may call memmap constructors. The core validates all offsets, lengths, alignment, row counts, enum values, and checksums before exposing capability views. No mapped reference may outlive its reader. Compaction and garbage collection retain files until every reader lease is released.

Primary and residual files are different `file_kind`s with different capability types. `PrimaryFileReader` exposes IDs, sequences, radius/flags, primary codes, and certificates. A primary scan receives only `PrimaryFileReader`; no handle reachable from the primary-scan path exposes residual access. `ResidualFileReader` keeps row access crate-private. Only a successfully validated `PairedSegmentReaders` exposes public candidate-rerank access to residual rows.

Both files carry the same stable 128-bit `segment_id`. Opening a rerank pair requires equality of collection, segment ID, dimension, row count, transform, codec, quantizer, PQ codebook, scorer, and layout identities before any residual row may be read. Two files built with the same codec configuration but different segment IDs are never pairable.

### 6.3 Consensus and apply boundary

The engine receives an ordered committed stream and never publishes speculative writes. A real one-member OpenRaft group is used in the first storage vertical slice; there is no separate local sequencer with weaker semantics.

The application state-machine contract is:

```text
committed operation
  -> validate sequence and idempotence
  -> atomically update PK truth, document truth, masks, and searchable memtable snapshot
  -> advance visible_seq through the highest contiguous published sequence
  -> allow acknowledgement
```

`OpenRaft` is isolated behind project-owned `Consensus`, `LogStore`, `StateMachine`, and `Transport` adapters. The single-node and multi-node modes use the same proposal, stable commit, apply, publication, and acknowledgement path.

### 6.4 Segment and manifest boundary

`SegmentStore` stages content-addressed immutable files and verifies their length, block checksums, and BLAKE3 identity. `ManifestStore` atomically and idempotently publishes a new manifest only when every referenced file is locally ready.

The first vertical slice installs:

- One vector primary file.
- One separately addressed residual file.
- IDs, sequences, radius/flags, and metadata needed for search validation.
- A complete immutable truth snapshot for the checkpoint.
- A manifest carrying the highest applied sequence it subsumes.

The full leveled PK and document-tombstone LSMs follow after the vertical slice. The truth snapshot preserves exact semantics without prematurely implementing the complete compaction topology.

### 6.5 Certified-score definition

The certified truth value is the original-space FP64 score:

```text
true_score = dot(normalize_fp64(q), normalize_fp64(x))
```

Serving and certificate generation compute the L2 norm and component division in FP64 using the same specified operation order. The normalized FP64 unit vector defines truth. Conversion into the transform kernel and all transform arithmetic are covered by `delta_T`, a constructive upper bound on the L2 error of the implemented transformed unit vector relative to the exact mathematical transform. Certificate construction computes the implemented transformed reconstruction error in FP64, adds `delta_T` with outward rounding to bound the unreachable exact mathematical transform error, and then takes the exhaustive block maximum. The certificate must use:

```text
eta_transform_dot = 2*delta_T + delta_T^2
query_norm_upper  = 1 + delta_T
epsilon_primary  = eta_transform_dot
                 + query_norm_upper * max ||T_exact(normalize(x)) - p||2
                 + eta_primary_score
epsilon_rerank   = eta_transform_dot
                 + query_norm_upper * max ||T_exact(normalize(x)) - (p + e)||2
                 + eta_rerank_score
```

`eta_primary_score` and `eta_rerank_score` cover query-table quantization, table-entry rounding, deterministic int64 accumulation, and conversion to the comparison scale. Every term is outward rounded. An equivalent tighter derivation is allowed only when its proof and property/fuzz tests are recorded. Assuming the implemented transformed query has norm exactly one is forbidden.

### 6.6 Query-source boundary

`CandidateSource` produces ordered, refillable candidates plus a conservative remaining bound. The first implementation is a filtered scan. A future immutable cell-local HNSW implementation uses the same interface and cannot become the source of truth.

Queries merge memtables and installed segments, validate candidates against the same snapshot truth before heap admission and final emission, rerank only candidates, and refill until the stopping condition is satisfied or all sources are exhausted.

### 6.7 Resource-governor boundary

All CPU, blocking I/O, mutable memory, query candidates, compaction staging, replication staging, and snapshot retention obtain bounded permits. Tokio owns network and control-plane work; bounded Rayon pools own CPU-heavy work; a separate bounded blocking path owns page faults and durability operations.

## 7. Milestones

### M0: Repository and measurement bootstrap

Deliverables:

- Initialize Git and record the approved documentation as the baseline.
- Create the pinned Rust workspace and `Cargo.lock`.
- Add formatting, lint, test, dependency-policy, and benchmark commands.
- Add local CI that runs the locked build, formatting, lint, unit/property tests, and dependency policy. Long fuzz/benchmark gates remain explicitly local and recorded.
- Create deterministic real/synthetic corpus descriptors, exact FP32 top-k ground truth, machine-profile capture, and JSON result schemas.
- Preserve the existing C++/Python experiment as historical evidence, not as production source.

Exit gate:

- Workspace builds and tests under the pinned toolchain.
- Benchmark output records hardware, OS, compiler, seed, corpus hash, transform/codec/scorer/layout versions, command, and durability mode.
- No dependency inversion appears in `cargo tree`.
- The automated dependency-policy check accepts exactly the normative DAG in section 5.

### M1: Scalar codec and durable-format foundation

Deliverables:

- Domain validation and billion-capable IDs/sequences.
- Exact two-round 768D transform and inverse.
- Direct-int4 384-byte logical code and TILED_SOA_32 storage.
- PQ96x8 96-byte residual.
- Fixed-point primary and refined scorers.
- Constructive rounding terms and certified-bound property tests.
- Versioned codec header, paired primary/residual files, and corruption-safe capability readers.
- Reproducible measurement CLI.

Exit gate:

- Transform is deterministic, norm preserving, dot preserving, and invertible within declared FP tolerance.
- Primary code is exactly 384 logical bytes/vector; residual is exactly 96 bytes/vector; tail padding and header amortization are reported separately.
- Primary scan tests cannot open or read the residual extent.
- `dot(T(normalize(q)), p + e)` conformance holds without renormalization or affine correction.
- A deterministic two-million-trial certificate soak across multiple transform seeds plus the fuzz target contain no bound violation.
- Corrupt/truncated format fuzzing returns structured errors without panic or out-of-bounds access.
- Real-corpus recall and candidate-budget curves are recorded. Failure to meet R7 quality gates blocks format freeze but does not permit silent architecture changes.

Detailed execution plan: [`../plans/2026-08-04-polar-codec-format-foundation.md`](../plans/2026-08-04-polar-codec-format-foundation.md)

### M2: One-member Raft and immediate-visibility vertical slice

Deliverables:

- Custom segmented Raft log with framed CRC-checked records and platform durability adapter.
- One-member OpenRaft group behind owned adapters.
- Searchable FP32 mutable and frozen memtables.
- Atomic PK/document truth publication and hole-free `visible_seq`.
- Insert, replace, chunk delete, and document delete.
- Freeze, build, stage, and commit `MANIFEST_INSTALL`.
- Restart from installed manifest/truth snapshot and replay committed log suffix.
- Minimal scan and residual rerank over memtable plus installed segment.

Normative acknowledgement order:

1. Validate the request and assign the vshard sequence.
2. Append the framed Raft log entry.
3. Reach stable one-member Raft commit.
4. Apply atomically to truth and searchable memtable state.
5. Advance `visible_seq` through the contiguous published prefix.
6. Return the session token.

Exit gate:

```text
insert
  -> durable Raft commit
  -> immediately visible FP32 memtable search
  -> freeze and build immutable files
  -> committed MANIFEST_INSTALL
  -> segment scan plus candidate residual rerank
  -> close/reopen
  -> same live result and no stale result
```

FaultyFs tests cover every append, sync, rename, directory sync, stage, and manifest boundary. After any injected crash, every acknowledged operation is present, no unacknowledged operation is promised, staged-but-uninstalled files are ignored, and stale versions never reappear.

### M3: Filtered single-node query engine and service

Deliverables:

- Spherical cells and certified cell bounds.
- Roaring/sorted-posting/zone-map filter planning.
- Snapshot validation, candidate refill, and deterministic top-k ordering.
- Resource admission, cancellation, deadlines, and write backpressure.
- tonic/prost public API and separate internal service.
- axum health, metrics, and administration endpoints.
- Python/NumPy and Rust clients.

Exit gate:

- The 10M R7 recall, latency, QPS, mixed-ingest, RSS, and SSD-headroom gates pass on recorded hardware.
- Immediate-visibility and session-token stress tests pass under real thread pools.
- Selectivity tests choose filtered scan or broader scan correctly and never under-fill silently.

### M4: Multi-node logical and physical replication

Deliverables:

- Three-node OpenRaft group using the existing log/state-machine contracts.
- tonic internal transport with bounded heartbeat and payload channels.
- Chunked, resumable, checksummed segment transfer.
- Quorum staging, committed manifest install, stalled-follower repair, and safe staged-file cleanup.
- Snapshot installation and replica repair.

Exit gate:

- Deterministic partition, leader-loss, slow-follower, duplicate, reorder, and restart scenarios preserve Raft safety and session visibility.
- No replica advances its applied/visible watermark past a manifest install whose files are unavailable.
- Segment transfer can resume and replacement of gRPC streaming would not change the installation protocol.

### M5: Multi-vshard PolarRouter

Deliverables:

- Meta-Raft placement state.
- Document-affine vshards and multi-Raft hosting.
- Physical-shard broker, fanout, deterministic merge, and per-vshard session tokens.
- Vshard movement and quiesced split with child lineage.

Exit gate:

- Scatter/gather results match the equivalent single-shard exhaustive result.
- Session tokens survive routing changes and splits according to R7.
- Rebalancing and splitting reuse physical installation rather than inventing another file path.

### M6: Benchmark-gated cell-local HNSW

Entry gate:

- M3 evidence shows that scan misses the declared latency/CPU/QPS target for specific cell sizes and filter selectivities.

Deliverables:

- Immutable per-cell graph construction during leader compaction.
- Project-owned versioned adjacency format and traversal.
- Adaptive selection between filtered scan and HNSW based on measured cardinality/cost.
- Scan fallback for certified queries whenever the graph frontier cannot justify stopping.

Exit gate:

- HNSW wins latency or CPU at equal recall and concurrent QPS inside its memory/build budget.
- Filtered recall is measured across selectivity bands.
- No permanent duplicate AoS/SoA representation is introduced.

### M7: Production hardening and billion-scale operations

Deliverables include backup/restore, object-store integration, long-running repair and scrub, rolling compatibility tests, autoscaling evidence, capacity planning, staged shard movement, and an operator only when manual/Helm operations show a measured need.

## 8. Verification architecture

Soundness gates and quality gates are separate.

Soundness gates may never fail:

- Certified bounds enclose true stored-original scores.
- Acknowledged writes survive every modeled crash.
- Version truth and tombstones prevent stale-row emission.
- Snapshot-pinned files are not reclaimed.
- Manifest installation never references unavailable or corrupt files.
- Format readers reject incompatible or malformed data safely.

Quality/performance gates are recorded measurements:

- Recall@10 and cone behavior against exact FP32.
- Bound tightness and prune fraction.
- Primary scan and candidate rerank throughput.
- Hot/warm/cold latency and mixed-ingest interference.
- Memory, page-cache, disk, write amplification, and build headroom.
- Filter-selectivity behavior and HNSW admission evidence.

Required techniques:

- Unit tests and `cargo-nextest` for contracts.
- `proptest` model-based state-machine and mathematical properties.
- `cargo-fuzz` for format parsing and bound soundness.
- Scalar/SIMD differential tests.
- `loom` for selected publication, watermark, swap, and reader-lifetime primitives.
- Injectable clock, filesystem, network, and RNG.
- Turmoil for deterministic network scenarios.
- Project `FaultyFs` for short/torn writes, reordering, and durability failures.
- Golden format fixtures and cross-version compatibility tests.
- Multi-hour ingest/search/restart soaks with asserted resource ceilings.

## 9. Deferred work and non-goals for M0-M1

Do not implement during the codec/format foundation:

- OpenRaft or any durable application log.
- Searchable memtables or database APIs.
- Network services, protobuf contracts, or clients.
- Spherical routing cells or pruning orchestration.
- HNSW, including a serving prototype.
- Compaction policy, sharding, replication transport, or Kubernetes deployment.
- Authentication, billing, full-text search, or arbitrary embedding dimensions.
- C++ production kernels. Keep C++/Highway only as a documented fallback if measured Rust kernels later miss their gate.

## 10. Status and document maintenance

The project guide is a living document. Every project change must leave it accurate about completed work, remaining work, and the next executable step. Contributors must not claim milestone completion from code presence alone; they must run and record the milestone exit gate.

After code, test, specification, benchmark-evidence, or status changes:

1. Update the project guide when status, constraints, decisions, paths, commands, or next work changed.
2. Re-run the verification commands for the changed area.
3. Verify no semantic refresh is pending and query the changed concepts.
4. Do not report completion while the project guide is stale.

## 11. Approval record

- **Branding amendment:** `polar` product/CLI/crate identifiers were changed to Spherra identifiers at the user's direction. Milestone boundaries, dependency direction, behavior, and verification gates are unchanged; no renewed technical review was required.

No implementation may begin from this document until both conditions are satisfied for the same unchanged revision.
