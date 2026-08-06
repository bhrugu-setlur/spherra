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

Both schemas set `additionalProperties: false` and require every field. An
omitted identity, corpus, byte-accounting, or bound-soundness field is a
validation failure, not a default. `crates/spherra-bench/tests/measurement_contract.rs`
holds the writer and the schema together: it lists the required fields
independently and fails if either side drifts.

`spherra-bench` validates its own output against the checked-in schema before
writing, and exits nonzero on any certificate bound violation.

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

**Throughput.** `primary_scan_vectors_per_second` measures the certified primary
scan over the whole block. The scan does not depend on the candidate budget, so
it is measured once and reported identically in every entry of one run; only
`residual_reranks_per_second` is re-measured per budget.

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
