# Spherra project guide

This file is the first stop for every agent working in this repository. It explains the product, the authoritative decisions, the current state, the next work, and the workflow required to keep project knowledge accurate.

## Mandatory living-document rule

**Every agent must keep this `AGENTS.md` current.** After any change to code, tests, specifications, plans, dependencies, durable formats, benchmarks, decisions, status, or workflow:

1. Re-read this file and update every affected section in the same task.
2. Update the Graphify project graph so `graphify-out/graph.json`, its manifest, and its report describe the changed repository.
3. Verify that Graphify reports no pending semantic refresh.
4. Do not claim the task or milestone is complete while this guide or the graph is stale.

Never hand-edit `graphify-out/graph.json`. Use the Graphify workflow described below.

## Start here

Read these sources in order before changing the project:

1. This file.
2. Query `graphify-out/graph.json` for the concepts involved in the task.
3. [`docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md`](docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md) — approved R7 architecture and product contract, with a non-behavioral Spherra naming amendment.
4. [`docs/superpowers/specs/2026-08-04-polar-v1-implementation-spec.md`](docs/superpowers/specs/2026-08-04-polar-v1-implementation-spec.md) — approved I4 implementation sequence and crate boundaries, with Spherra identifiers.
5. The active plan named in **Current status and next step**.
6. Relevant source and tests for implementation details.

The current repository and fresh test/benchmark output are authoritative for what actually exists and passes. The approved specifications are authoritative for intended behavior. If implementation and specification conflict, stop, document the conflict, and resolve it rather than silently choosing one.

## Project in one paragraph

Spherra is a distributed production vector database for 768-dimensional RAG and semantic-search embeddings. It combines **PolarLSM**, which owns durable writes, exact version truth, immediate visibility, immutable storage, compaction, recovery, and replication, with **PolarRouter**, which owns spherical direction cells, metadata-aware routing, certified pruning, filtered scans, and optional immutable cell-local HNSW. Each document is chunked before ingestion; each chunk receives one vector plus metadata linking it to its parent document.

## Naming contract

- The product, repository, public command, and first-party Rust package prefix are **Spherra** / `spherra` / `spherra-*`.
- **PolarLSM** and **PolarRouter** remain the names of the two core subsystems.
- **TurboQuant** refers only to the Google Research work that inspired parts of the vector-conditioning and quantization approach; it is not this project's name.
- Existing specification and plan filenames retain `polar` because they describe the PolarLSM/PolarRouter architecture and polar codec work. Do not rename those files merely for branding.

## Product and hardware contract

- Development hardware: Apple M1 Pro, 8 cores (6 performance and 2 efficiency), 32 GiB unified memory, 1 TB SSD.
- Local target: 10 million chunks.
- Local stretch target: 25 million chunks, with no SLO until it passes the same acceptance harness.
- Billion-vector scale: distributed cluster only; never promise it on the laptop.
- Steady-state process RSS cap: 20 GiB.
- Preserve at least 4 GiB for OS/page-cache headroom and at least 15% free SSD.
- Shared memtable/query/compaction working memory: at most 2 GiB.
- Acknowledged writes must appear in every subsequent session-consistent search before flush.
- Source documents remain external. A stored chunk has `chunk_id`, parent `document_id`, `chunk_ordinal`, source URI, metadata, one vector, and optional inline text capped at 16 KiB.

## Frozen architecture decisions

Do not change these without a new architecture revision and renewed Codex/Claude approval:

### Vector representation and scoring

- Dimension is 768.
- Store an FP16 original norm promoted to FP32 for inclusive radius comparisons.
- Store a direct four-bit transformed direction: 768 nibbles = 384 logical bytes/vector.
- Store a four-byte radius/flags side word.
- Store a **PQ96x8 residual**: 96 one-byte codes over 96 eight-dimensional subvectors = 96 bytes/vector.
- Residuals are candidate-only SSD/page-cache data. Do not count them as required resident scan memory and do not interleave them into the primary scan stream.
- Primary and residual files use different capability readers and carry the same 128-bit `segment_id`; pairing validates every format/codec identity before a residual row can be read.
- The transform is exactly two independently seeded rounds. Each round performs sign flips, one global permutation, and six normalized 128-point fast Hadamard transforms.
- The serving score is `dot(T(normalize(q)), p + e)`.
- Do not renormalize `p + e` and do not add a learned affine correction.
- Fixed-point lookup entries and int64 accumulation define the comparison path.
- Certified truth is the original-space FP64 dot product. Query normalization is FP64, and constructive bounds include kernel-input conversion, transform arithmetic, reconstruction, and fixed-point scoring error.
- `scorer_version` owns comparison scale; `codec_id` owns representation.
- FP32 originals may be retained externally for validation or migration but are not part of the resident database promise.

### Storage, truth, and visibility

- Shard by `hash(tenant, collection/model, document_id)` so all chunks and deletes for a document stay together.
- Direction cells are physical partitions within a vshard; they are not database shard keys or LSM merge keys.
- Primary-key and document-tombstone truth must be snapshot readable. Latest-value-only truth is incorrect.
- Mutable and frozen memtables keep normalized FP32 directions and are exactly searchable.
- Every replica applies committed Raft operations to a searchable memtable.
- Leaders alone build immutable segments and physically replicate their exact content-hashed bytes.
- A `MANIFEST_INSTALL` record is committed only after required files are staged; applying it atomically installs files and retires covered memtable/log ranges.
- The state-machine apply path is the only publisher of searchable writes. Never expose a speculative or precommit cache.
- `visible_seq` advances only through the highest contiguous committed-and-published sequence.
- Acknowledgement order is validate/sequence, durable Raft append, stable commit, atomic searchable publication, contiguous `visible_seq`, then response.
- Compaction never transcodes codes and never permanently duplicates AoS and SoA representations.

### Query and indexing

- Search mutable/frozen memtables plus installed immutable segments.
- Validate truth before heap admission and again before final emission/rerank.
- Refill candidates after stale rows or filters; never silently under-fill.
- Certified pruning uses bounds computed while original FP32 directions exist. A sampled maximum is not certified.
- The first serving index is a filtered scan.
- There is no global HNSW.
- HNSW, if admitted by benchmarks, is immutable and local to selected direction cells.
- A certified query falls back to exhaustive scoring when an HNSW frontier cannot prove safe stopping.

## Provisional, benchmark-selected decisions

The architecture is frozen, but these values remain provisional until the R7 gates pass:

- Direct-int4 quantizer tables and training details.
- PQ96x8 codebooks and candidate budget.
- Transform seeds and durable identity derivation.
- TILED_SOA_32 versus another measured eligible layout.
- Cell count and training parameters.
- HNSW admission, graph parameters, and rebuild thresholds.
- Fanout, oversampling, validation budgets, and exact memory thresholds.
- Performance SLO freeze for 10M and any claim for 25M.

Do not call a provisional value final merely because it appears in an experiment or initial format. Record the evidence and decision.

## Approved technology stack

- Rust 1.88.0, edition 2024, pinned by `rust-toolchain.toml` and `Cargo.lock`; an exact dated nightly is used only for fuzzing.
- One modular `spherra serve` process initially.
- Scalar reference kernels plus isolated Rust `core::arch` SIMD: Apple/Linux ARM NEON, x86 AVX2, and later AVX-512.
- Tokio for networking/control/Raft tasks; bounded Rayon for CPU-heavy work; separate bounded blocking I/O.
- tonic/prost gRPC for public/internal APIs; axum for health, metrics, and admin HTTP.
- OpenRaft behind project-owned adapters. Single-node mode uses a real one-member Raft group and the same commit/apply path as a cluster.
- Custom authoritative Raft log, truth storage, manifests, vector/residual/graph/blob formats. Do not put RocksDB, redb, SQLite, or another general embedded database in the authoritative path.
- Explicit little-endian durable bytes and checked positional readers in M1. A later audited module may add read-only memmap2 behind `SegmentReader`; mapped writes are forbidden.
- Roaring bitmaps, sorted postings, and zone maps for metadata filters.
- zstd for blob blocks; no default compression for random-access primary/residual codes.
- CRC32C per block and BLAKE3 whole-file identity.
- tracing, OpenTelemetry, and Prometheus-compatible metrics.
- cargo-nextest 0.9.114, proptest, cargo-fuzz 0.13.2/arbitrary, loom, injectable dependencies, Turmoil, and project FaultyFs; fuzzing uses `nightly-2025-06-26` only.
- Python/NumPy and Rust clients first.
- Multi-architecture glibc-based minimal Debian images and Helm node-pool StatefulSets; each pod hosts many vshards/Raft groups. No operator initially.

The C++20/Highway kernel path is a measured fallback only if Rust SIMD materially misses its gates. Do not introduce that second toolchain preemptively.

### Toolchain bootstrap note

- Homebrew's keg-only `rustup` formula does not provide `rustup-init` or add `~/.cargo/bin` to non-login shells. The Task 1 bootstrap uses `brew install rustup`, adds its bin directory for the installation command, and local scripts prepend `${CARGO_HOME:-$HOME/.cargo}/bin` so `bash scripts/ci.sh` remains the portable local gate.
- The required `cargo-fuzz 0.13.2` install on `nightly-2025-06-26` uses `--ignore-rust-version`: its locked `cargo-platform 0.3.3` currently declares Rust 1.91 although the required nightly is Rust 1.90.0-nightly. This preserves both approved versions and is only a tooling reproducibility amendment; the original install's exit 101 and fallback evidence are recorded with Task 1.

## Current repository state

Last updated: 2026-08-04.

### Completed

- Product name selected as **Spherra**; repository directory, public command, planned crate prefix, documentation, and prototype namespace use the new identity.
- Initial documentation and prototype baseline is version-controlled on `main` with `origin` set to `https://github.com/bhrugusetlur-art/spherra.git`.
- Root `.gitignore` excludes OS files, Python bytecode, build/fuzz output, and local Graphify runtime/cache/backup artifacts from version control.
- Graphify post-commit/post-checkout hooks are installed locally, and `.gitattributes` registers the Graphify merge driver for `graphify-out/graph.json`.
- R7 production architecture approved by Codex and Claude Opus; the later Spherra naming amendment changes no behavior.
- Rust-first technology stack approved by Codex and Claude Opus.
- C++20 scalar layout/access experiment comparing recursive angles and direct int4.
- Python synthetic retrieval-quality comparison.
- Project Graphify graph exists.
- I4 v1 implementation specification and P4 codec/format foundation plan approved by Codex and Claude Opus; Spherra package and command identifiers were applied later as a user-directed, non-behavioral naming amendment.
- Task 1 established the Rust 1.88.0 / edition 2024 resolver-3 workspace, root unsafe/warnings denial, six empty Spherra crates with the approved dependency direction, isolated nightly-only fuzz targets, lockfiles, dependency-policy check, and local/GitHub CI gates. Fresh local quality, policy, fuzz-target, and advisory checks passed.
- Task 2 defined the `spherra-domain` contracts for 768-dimensional validation, FP16-safe stored radius, direction reliability, distinct 128-bit chunk/document IDs, and 16-bit-epoch/48-bit-index put sequences. Five focused contract tests plus the complete workspace quality gate pass.
- Task 3 implemented the exact scalar transform oracle: fixed-size normalized H128, cached two-round sign/permutation/H128 plans, and a provisional domain-separated canonical-little-endian BLAKE3 transform identity. Public forward transform entry points accept only `ReliableDirection`; six focused scalar/codec property and contract tests plus the complete workspace quality gate pass.

### Existing artifacts

- `README.md`
- `.gitignore`
- `.gitattributes`
- `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `deny.toml`
- `.github/workflows/ci.yml`
- `scripts/check_dependency_policy.py`, `scripts/ci.sh`
- `crates/spherra-domain/`, `crates/spherra-simd/`, `crates/spherra-codec/`, `crates/spherra-format/`, `crates/spherra-testkit/`, `crates/spherra-bench/`
- `crates/spherra-domain/src/error.rs`, `ids.rs`, `record.rs`, `sequence.rs`, and `tests/contracts.rs`
- `crates/spherra-simd/src/scalar.rs` and `tests/scalar_transform.rs`
- `crates/spherra-codec/src/spec.rs`, `transform.rs`, and `tests/transform_contract.rs`
- `fuzz/Cargo.toml`, `fuzz/Cargo.lock`, `fuzz/fuzz_targets/`
- `docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md`
- `docs/superpowers/specs/2026-08-04-polar-v1-implementation-spec.md`
- `docs/superpowers/plans/2026-08-04-polar-codec-format-foundation.md`
- `experiments/codec_bench/README.md`
- `experiments/codec_bench/codec.hpp`
- `experiments/codec_bench/codec.cpp`
- `experiments/codec_bench/codec_test.cpp`
- `experiments/codec_bench/bench.cpp`
- `experiments/codec_bench/accuracy.py`
- `experiments/codec_bench/accuracy_test.py`
- `graphify-out/graph.json`
- `graphify-out/GRAPH_REPORT.md`

### Not implemented

There is no production database yet. Specifically absent are:

- Production Rust functionality beyond the Task 3 scalar transform oracle; direct-int4, PQ, format, testkit, and benchmark crates remain stubs.
- Production direct-int4, PQ96x8, fixed-point scoring, or certified-bound implementation.
- Production SIMD kernels or real-corpus acceptance harness.
- Durable Raft log, OpenRaft integration, searchable memtables, truth indexes, or session tokens.
- Segment/manifest files, compaction, recovery, blob store, replication, or repair.
- PolarRouter cells, certified query orchestration, filters, or HNSW.
- Public server, clients, containers, Helm deployment, or release process.

Do not describe the approved production design as a working production database.

## Current status and next step

Current phase: **P4 Task 3 exact scalar transform oracle complete; Task 4 is next.**

Active plan: approved P4 [`docs/superpowers/plans/2026-08-04-polar-codec-format-foundation.md`](docs/superpowers/plans/2026-08-04-polar-codec-format-foundation.md), with Spherra identifiers applied as a naming-only amendment.

Next executable work:

1. Execute Task 4 of the active plan: implement direct-int4 and `TILED_SOA_32`.
2. Continue the active plan through the scalar codec/format measurement gate.
3. Write the detailed M2 one-member-Raft vertical-slice plan only after M1 evidence is recorded.

Do not start Raft, networking, HNSW, sharding, or production SIMD during the active M0-M1 plan.

## Required development workflow

### Before work

1. Read this file completely.
2. Query Graphify before opening many files:

   ```bash
   $(cat graphify-out/.graphify_python) -m graphify query "<task concepts>" --budget 4000
   ```

3. Read the relevant approved-spec sections and active-plan task.
4. Check the actual workspace state and fresh verification results. Never infer completion from documentation.
5. For architecture or implementation-spec changes, use Claude Opus as an independent reviewer and obtain explicit approval of the same unchanged revision before presenting it as approved.

### During work

- Follow the active plan task-by-task.
- Use test-driven development: failing test, observed failure, minimal implementation, observed pass, then cleanup.
- Keep changes scoped. Do not opportunistically build later milestones.
- Preserve explicit durable formats and backward-compatibility tests.
- Record benchmark inputs, corpus hashes, seeds, machine profile, compiler/toolchain, command, raw results, and durability mode.
- Treat scalar/synthetic prototype results as evidence only for the question they measured.
- Never claim performance, recall, crash safety, or compatibility without a fresh command that proves it.
- Do not silently modify architecture to make a test pass. Escalate the conflict.

### After every project change

1. Run the relevant tests, formatters, linters, compatibility checks, or benchmarks.
2. Inspect the full diff and ensure unrelated user changes remain untouched.
3. Update **Current repository state**, **Current status and next step**, artifact paths, and commands in this file when affected.
4. Refresh Graphify from the repository root:

   ```bash
   $(cat graphify-out/.graphify_python) -m graphify update .
   $(cat graphify-out/.graphify_python) -m graphify check-update .
   ```

5. If docs, papers, images, or query-memory changes leave a semantic refresh pending, run the complete `/graphify . --update` skill workflow. Do not bypass semantic extraction or merely delete the pending marker.
6. Query the changed concepts and confirm the new/changed files appear in traversal.
7. Save useful Graphify answers with `graphify save-result` when the Graphify skill requests it.
8. Only then report completion.

When Git is initialized, install the Graphify post-commit hook for code updates, but continue the manual semantic refresh for documentation changes:

```bash
$(cat graphify-out/.graphify_python) -m graphify hook install
```

## Verification commands available now

The workspace quality gate is:

```bash
bash scripts/ci.sh
cargo test --workspace --all-features --locked
cargo +nightly-2025-06-26 fuzz build format_open
cargo +nightly-2025-06-26 fuzz build bound_soundness
```

The existing prototype commands are:

```bash
clang++ -std=c++20 -O3 -mcpu=native -DNDEBUG -Wall -Wextra -Werror \
  experiments/codec_bench/codec_test.cpp \
  experiments/codec_bench/codec.cpp \
  -o /tmp/spherra_codec_test
/tmp/spherra_codec_test

uv run --python 3.12 --with numpy --with scipy \
  python experiments/codec_bench/accuracy_test.py
```

Their passing state must be established by a fresh run before it is reported. The Task 1 workspace commands above also require fresh output before any claim of completion.

## Definition of done for any task

A task is complete only when:

- The requested artifact or behavior exists.
- The relevant fresh verification commands pass and their output has been read.
- No Critical or Important independent-review findings remain.
- Specifications, plan checkboxes, benchmark evidence, and this guide reflect reality.
- Graphify has been refreshed and the changed concepts are queryable.
- The next executable step is named accurately in this file.
