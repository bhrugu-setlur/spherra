# Spherra benchmark evidence

This directory holds the **contract** for Spherra measurements: the JSON Schema
documents a result must satisfy, and the rules about what a given result is and
is not evidence for. Result files themselves live under `results/` and are only
committed after they have been checked for private paths and corpus content.

A number is not evidence unless it can be reconstructed. Everything below exists
to make reconstruction possible from the recorded file alone.

## Schemas

| Document | Produced by | Shape |
| --- | --- | --- |
| [`codec-format-baseline.schema.json`](codec-format-baseline.schema.json) | `spherra-bench codec-format` | JSON array, one entry per candidate budget |
| [`certificate-soak.schema.json`](certificate-soak.schema.json) | `spherra-bench certify` | Single JSON object |
| [`local-oracle-reference.schema.json`](local-oracle-reference.schema.json) | `spherra-bench oracle-reference` | Streaming exact-reference provenance and artifact hash |
| [`local-latency.schema.json`](local-latency.schema.json) | `spherra-bench latency` | Build/open timings, 1,000 query samples, RSS and gate result |
| [`local-build-memory.schema.json`](local-build-memory.schema.json) | `spherra-bench build-memory` | Child-process peak minus baseline and caller inputs |

These schemas set `additionalProperties: false` and require every field. An
omitted identity, corpus, byte-accounting, or bound-soundness field is a
validation failure, not a default. `crates/spherra-bench/tests/measurement_contract.rs`
holds the writer and the schema together: it lists the required fields
independently and fails if either side drifts.

`spherra-bench` validates its own output against the checked-in schema before
writing, and exits nonzero on any certificate bound violation.

## Local-index measurement protocol

The local-index commands implement the September local design, independently of
the historical M1/R7 results below. Run from a clean worktree, with a release
binary, and write initial output under ignored `target/` paths. Commit reviewed
JSON evidence afterward. A smoke run never claims to satisfy a full-size gate.

```bash
cargo run -p spherra-bench --release --locked -- oracle-reference \
  --rows 1000000 --queries 200 --seed 20260804 \
  --output target/measure/local-1m-oracle.json

cargo run -p spherra-bench --release --locked -- build-memory \
  --seed 20260804 --output target/measure/local-build-memory.json

cargo run -p spherra-bench --release --locked -- latency \
  --rows 1000000 --queries 1000 --warmup 50 --training-rows 4096 \
  --seed 20260804 --index-dir target/local-index-1m \
  --output target/measure/local-latency-1m.json
```

Repeat latency with `--rows 10000000` and a separate index/output path. Subsequent
kernel measurements use `--reuse true` on the same index. Reuse checks the
versioned generator descriptor, training size, and full CURRENT hash against
`benchmark-build.json`; the report distinguishes the original build commit and
timing from the current search commit. The library treats that benchmark sidecar
as an unrelated file.

Generated sources concatenate 1M-row chunks. Chunk seeds are the first eight
little-endian bytes of BLAKE3 derive-key with context
`spherra.generated-correlated.chunks.v1` and material `(root_seed u64, chunk u64)`.
Each chunk uses the existing indexed RNG stream and AR(1) generator (rho 0.85).
Training and queries use the existing, separate calibration/query streams with
the root seed. Descriptors record every chunk seed. Corpus and query hashes cover
their raw little-endian FP32 rows. Rows are generated without retaining additional
calibration/query matrices per chunk.

The streaming oracle retains only normalized queries and bounded top-100 heaps
per worker, merging in exact FP64 score/row order. The read-only reference artifact
is `SPHROR01`, row count u64, query count u32, k u32, then per query its actual
count u32 and `(row u64, score f64)` pairs, all little endian. Its full BLAKE3 and
generator parameters are committed before index performance/quality evidence.

Latency measures one public `search` at a time, including query preparation and
residual reads, with k=10 and the default budget. It warms with the last 50 of
1,050 generated queries and times the first 1,000. Percentiles use nearest rank;
all raw samples are retained. AC power is checked before warmup, every 100 timed
queries, and after the run. Gate eligibility requires the clean release build,
the qualified M1 Pro/32 GiB hardware, and the exact row/query/warmup counts.

`latency` and `build-memory` run a fresh child under macOS `/usr/bin/time -l`.
The adjacent `.time.txt` retains progress and the raw peak-RSS output in bytes.
Latency's peak includes the child's build, open, and search, so it is conservative
for search alone. Build throughput includes generation, training, encoding,
verification and file/directory synchronization. Open time and retained descriptors
are measured separately. Stage 1 uses the checked scalar kernel.

The memory probe defaults to exactly 32,768 training inputs and 65,537 pushed
rows. It records RSS before and after allocating caller inputs, their logical
bytes and vector capacities, then reports peak RSS minus pre-input RSS minus
input allocation bytes. A negative estimate is a measurement error; it is never
clamped to zero. Smaller explicit `--training-rows`/`--rows` runs test the command
but are not gate evidence. The full builder limit is 2 GiB. A missed eligible
gate writes its measured result and exits nonzero.

A separate ignored library `worker_probe` compares 4/6/8 workers on the measured
1M index (20 queries, five warmups). It is a scaling probe, not a substitute for
the 1,000-query acceptance runs, and does not add an option to the public API.

### Local index stage 1 results (2026-09-13)

Both complete runs used six workers, k=10, budget 200, 50 warmups and 1,000
timed public-API searches on the M1 Pro, on AC power, from clean release commits.

| Rows | p50 | p99 | Peak process RSS | Open | Build rows/s | Retained descriptors |
|---|---:|---:|---:|---:|---:|---:|
| 1M | 630.172 ms | 807.444 ms | 6,665,224,192 B | 1.119 s | 11,729.022 | 17 |
| 10M | 6,438.614 ms | 6,974.116 ms | 7,354,368,000 B | 11.732 s | 11,935.785 | 161 |

Stage 1 misses both latency targets at both sizes. The 10M process memory gate
passes. The separate builder probe also passes: 938,000,384 peak RSS minus
6,815,744 pre-input RSS minus 301,992,960 live caller-input bytes equals
**629,191,680 builder-owned bytes**, below the 2 GiB limit. These are measured
values; the design's earlier memory estimates are not substituted for them.

Results and original raw `time -l` logs:

- [1M JSON](results/2026-09-13-local-index-stage1-latency-1m.json),
  [raw log](results/2026-09-13-local-index-stage1-latency-1m.time.txt).
- [10M JSON](results/2026-09-13-local-index-stage1-latency-10m.json),
  [raw log](results/2026-09-13-local-index-stage1-latency-10m.time.txt).
- [Builder memory JSON](results/2026-09-13-local-index-build-memory.json),
  [raw log](results/2026-09-13-local-index-build-memory.time.txt).
- [Worker probe](results/2026-09-13-local-index-stage1-workers.txt): 4/6/8 workers
  gave p50 853.208 / 633.932 / 591.882 ms. This small probe changes no default.

The JSONs preserve their original commands and `target/measure/` output paths;
the links above archive those exact bytes. The 1M corpus matches the previously
pinned streaming-oracle reference.

### Local index stage 2 results (2026-09-13)

The safe tile kernel passes every latency gate at clean commit `84065a9`:

| Rows | p50 | p99 | Peak open/search RSS | Retained descriptors |
|---|---:|---:|---:|---:|
| 1M | 52.622 ms | 147.278 ms | 399,638,528 B | 17 |
| 10M | 458.146 ms | 616.475 ms | 3,855,040,512 B | 161 |

Both runs used the same protocol and identical index, corpus, query and model
identities as stage 1, verified before comparison. Each contains all 1,000 raw
samples after 50 warmups. Median search improved 11.98x at 1M and 14.05x at 10M.
The indexes were reused: these RSS measurements cover open/search, whereas the
stage 1 process also built its index. That difference is not a kernel memory
improvement. Build throughput and the independent builder-memory gate above
remain the build evidence.

- [1M JSON](results/2026-09-13-local-index-stage2-latency-1m.json),
  [raw log](results/2026-09-13-local-index-stage2-latency-1m.time.txt).
- [10M JSON](results/2026-09-13-local-index-stage2-latency-10m.json),
  [raw log](results/2026-09-13-local-index-stage2-latency-10m.time.txt).

The release differential qualification compared every primary score on archived
SciFact (73,760 scores) and generated 100k (2,000,000 scores), over 20 queries
each, with zero differences. Randomized and boundary tests cover full/partial
tiles, malformed geometry, and the checked fallback outside the range proof.
**Stage 3 is skipped.** No NEON or unsafe code is introduced; the scorer,
representation, certificates and default budget remain unchanged.

## Running the harness

Baseline measurement, one result per candidate budget:

```bash
cargo run -p spherra-bench --release --locked -- codec-format \
  --corpus generated-correlated-768x20000 \
  --queries 200 \
  --seed 20260804 \
  --candidate-budget 10,20,50,100,200 \
  --layout tiled-soa-32 \
  --output target/measure/codec-format-smoke.json
```

Certificate enclosure soak:

```bash
cargo run -p spherra-bench --release --locked -- certify \
  --trials 2000000 \
  --seed 20260804 \
  --transform-seeds 4 \
  --output target/measure/certificate-soak.json
```

`--cache-state` accepts `cold`, `warm`, or `hot` and defaults to `warm`; it is
recorded, never inferred. `durability_mode` is `not-applicable` for all of M1,
because M1 performs no durable write. A later storage milestone replaces it with
the actual fsync mode.

## Corpora

`--corpus` takes either a generated name or the path of a file-backed
descriptor.

**Generated** corpora are named `generated-<gaussian|correlated>-<dimension>x<count>`.
Every vector is a pure function of the descriptor and `--seed`. `correlated`
draws an AR(1) chain across coordinates (ρ = 0.85), which exercises the
neighbouring-coordinate correlation real embeddings show and an independent
Gaussian corpus does not.

**File-backed** corpora are pinned by a JSON descriptor recording path, byte
length, BLAKE3, row count, dimension, normalization policy, source dataset
revision, embedding model revision, and license. Loading verifies the recorded
length and hash before any vector is used, so a re-embedded or truncated file
cannot masquerade as the pinned corpus.

Every corpus is split three ways, and the splits are disjoint:

* **calibration** — the only rows the int4 quantizer and PQ96x8 codebook are
  trained on;
* **queries** — held out, so a measured neighbour is never the query itself;
* **indexed** — everything else, and the only rows `corpus_hash` covers.

`corpus_hash` is the lowercase hex BLAKE3 over the canonical little-endian FP32
bytes of the indexed rows in row-major order.

### The default real corpus

[`tools/build_scifact_corpus.py`](../../tools/build_scifact_corpus.py) builds the
BEIR SciFact corpus embedded with `sentence-transformers/all-mpnet-base-v2` at
768 dimensions, and emits a descriptor carrying the exact dataset and model
commit hashes. If it cannot resolve immutable upstream revisions it exits
nonzero rather than writing unpinned evidence.

The reviewed corpus is rebuilt with the recorded immutable revisions:

```bash
uv run --python 3.12 --with mteb --with sentence-transformers \
  --with huggingface-hub --with numpy --with blake3 \
  python tools/build_scifact_corpus.py \
  --output-dir corpora/scifact \
  --dataset-revision cf10ab6856b15b0e670ef8ae5dae4e266c12d035 \
  --model-revision e8c3b32edf5434bc2275fc9bab85f82640a19130
```

The portable descriptor is committed at
`corpora/scifact/scifact-mpnet-768.json`; the generated FP32 corpus bytes are
ignored and must reproduce its recorded length and BLAKE3 before loading.

> **SciFact is smoke-scale.** It demonstrates that the harness is correct on
> real embeddings. It is **not** sufficient evidence for a 10M format freeze.
> The R7 retrieval-quality gate stays open until results are recorded on
> multiple real corpora, including at least one with 100,000 or more vectors.

Do not commit proprietary or private corpus contents. Commit descriptors and
hashes only, and only when the license permits reproducible retrieval.

## What the recorded fields mean

**Exact truth.** `recall_at_10` and `recall_at_100` are measured against an
exhaustive FP64 oracle: the query and every stored row are normalized in FP64
and compared by FP64 dot product, ordered by score descending then row ascending.
This is the same certified truth the certificate bounds must enclose.

A budget smaller than *k* cannot reach recall 1.0 at *k*, and the harness never
pads a short candidate list to hide that. Recall also saturates below 1.0 at
large budgets — that ceiling is the codec's reconstruction error, not a budget
effect.

**Byte accounting.** `logical_primary_bytes_per_vector` is 384 (768 nibbles) and
`logical_residual_bytes_per_vector` is 96, both properties of the representation.
`physical_primary_bytes` and `tail_padding_bytes` come from the measured
`TILED_SOA_32` block, so padding in a partial tail tile is visible rather than
absorbed. `header_bytes` is the real v1 segment header plus its checked section
directory, read back from an encoded primary segment.

**Throughput.** `primary_scan_vectors_per_second` times TILED_SOA code reads and
the production fixed-point primary scorer over the whole block.
`residual_reranks_per_second` times candidate preparation, the candidate's one
permitted residual load, and the production fixed-point refined scorer.
Certificate construction and FP64-oracle validation remain mandatory but are
outside both timed regions. The primary scan does not depend on candidate
budget, so it is measured once and reported identically in every entry of one
run; only residual reranking is re-measured per budget.

**Bound soundness.** Every primary and refined score on the measured path is a
certified score, and every one is checked against FP64 truth. The violation
counts must be zero; the command exits nonzero if they are not.

`primary_bound_width_percentiles` and `refined_bound_width_percentiles` record
`upper - lower` over the certified candidates. In M1 the certificate is computed
per block, so within a single run these widths are near-constant and the four
quantiles agree closely; the summary becomes informative once bounds vary per
row or per cell. Refined widths should be materially tighter than primary
widths, which is what the residual refinement buys.

`maximum_normalized_primary_slack` and `maximum_normalized_refined_slack` in a
soak result are the *unused* fraction of the certified error budget,
`(epsilon - |score - truth|) / epsilon`. A value near 1.0 means the bound was
almost entirely slack — sound, but conservative.

## Reading a result honestly

* A scalar or synthetic result is evidence only for the question it measured.
* A provisional value does not become final by appearing in a result file. The
  int4 tables, PQ codebooks, transform seeds, layout choice, and candidate
  budget all remain benchmark-selected until the R7 gates close.
* `dirty_worktree: true` means the measured tree did not match its commit. Such
  a result is not admissible as gate evidence.
* Never claim performance, recall, crash safety, or compatibility without a
  fresh command that produced it.

## Recorded results

Committed under [`results/`](results/), all measured at commit `2bd12a0` with
`dirty_worktree: false` on an Apple M1 Pro (32 GiB), `rustc 1.88.0 (6b00bc388
2025-06-23)`, release profile, `--cache-state warm` (asserted, never enforced).

| File | Command |
| --- | --- |
| `2026-08-06-codec-format-generated-correlated-768x20000.json` | `spherra-bench codec-format --corpus generated-correlated-768x20000 --queries 200 --seed 20260804 --candidate-budget 10,20,50,100,200 --layout tiled-soa-32` |
| `2026-08-06-codec-format-beir-scifact-mpnet-768.json` | `spherra-bench codec-format --corpus corpora/scifact/scifact-mpnet-768.json --queries 200 --seed 20260804 --candidate-budget 10,20,50,100,200 --layout tiled-soa-32` |
| `2026-08-06-certificate-soak-2000000.json` | `spherra-bench certify --trials 2000000 --seed 20260804 --transform-seeds 64` |

The SciFact result uses the committed relative descriptor path directly; no
post-run path sanitization was needed.

### The pinned SciFact corpus

Built by [`tools/build_scifact_corpus.py`](../../tools/build_scifact_corpus.py)
from the immutable revisions shown above:

```
name                     beir-scifact-mpnet-768
row_count                5183          (3688 indexed after the disjoint split)
dimension                768
byte_len                 15922176      (5183 x 768 x 4)
blake3                   8a20ab21c26a201c42a7d491101f0391cc416b3b869cb61e768ac4e9ddefa4ff
source_dataset_revision  mteb/scifact@cf10ab6856b15b0e670ef8ae5dae4e266c12d035
embedding_model_revision sentence-transformers/all-mpnet-base-v2@e8c3b32edf5434bc2275fc9bab85f82640a19130
normalization            l2-unit
license                  CC BY-NC 2.0 (BEIR SciFact); model Apache-2.0
```

The corpus file itself is **not committed** — the descriptor and hashes are the
reproducible artifact, and the BEIR license does not invite redistribution here.

`corpus_hash` is `d164a143...` for `--queries 200`. Per the split rule, a
different `--queries` changes the indexed split and therefore this hash on a
file-backed corpus; it is not a property of the pinned file alone.

### Measured recall

Generated `correlated` (20,000 rows, 0 tail padding):

| Budget | recall@10 | recall@100 |
| ---: | ---: | ---: |
| 10 | 0.8540 | 0.1000 |
| 20 | 0.9330 | 0.2000 |
| 50 | 0.9370 | 0.4995 |
| 100 | 0.9370 | 0.8968 |
| 200 | 0.9370 | 0.9552 |

BEIR SciFact (3,688 indexed rows, 9,216 tail-padding bytes — a partial tail tile
the generated corpus does not exercise):

| Budget | recall@10 | recall@100 |
| ---: | ---: | ---: |
| 10 | 0.9425 | 0.1000 |
| 20 | 0.9775 | 0.2000 |
| 50 | 0.9775 | 0.5000 |
| 100 | 0.9775 | 0.9582 |
| 200 | 0.9775 | 0.9817 |

Every entry in both files reports `primary_bound_violation_count: 0` and
`refined_bound_violation_count: 0`.

## R7 quality-gate scoring

Scored against [design spec](../design/2026-08-04-polar-lsm-router-design.md)
§11.1. **The R7 quality gate as a whole remains open.** One sub-gate has passing
evidence at smoke scale; the rest are unmeasured.

| §11.1 gate | Verdict | Basis |
| --- | --- | --- |
| Several real 768D corpora plus synthetic | **FAIL** | One real corpus only (SciFact, 3,688 indexed rows). The policy above requires several real corpora including one with >= 100,000 vectors; the largest measured is 27x short. |
| Recall@10 >= 0.90 after residual rerank | **PASS (smoke scale only)** | 0.9775 SciFact and 0.9370 generated, at budget >= 20. At budget 10 SciFact is 0.9425 and generated is 0.8540, so the gate is budget-dependent. Truth is an exhaustive FP64 oracle, which is at least as strict as the spec's "exact FP32". |
| Fast/approximate cone recall >= 0.95 | **INSUFFICIENT EVIDENCE** | No cone path exists in M1. PolarRouter, cells, and fanout are later milestones; nothing was measured. |
| Filtered recall across selectivity bands | **INSUFFICIENT EVIDENCE** | No metadata filters exist in M1. Nothing was measured. |
| Oversampling and validation inside memory budgets | **FAIL** | The harness holds an FP64 oracle of `rows x 768 x 8` bytes plus per-query lookup tables — roughly 350 MB at 20,000 rows and about 61 GB of oracle alone at the 10M target, against a 20 GiB RSS cap. The measurement path cannot reach target scale in its current form. |

Consequently the primary/residual codec, transform rounds, layout, and candidate
budget **are not frozen** by this milestone. They remain provisional.
