# Additional dot-product search: test data and results

Date: 2026-09-14. Branch/worktree: `local-index` / `.worktrees/local-index`.
The user requested a way to use length in an additional search method.
[Contract and arithmetic proof](../design/2026-09-14-dot-product-search-amendment.md).

## Implementation

`Index::search_dot_product()` uses length during the full primary scan and
residual refinement. It has distinct result/hit types, approximate original-dot
scores and intervals accounting for compression, FP16 rounding and FP64
arithmetic. It reuses the existing two-byte-per-row cache. There is no additional
per-row memory, original-vector storage, durable format change or change to the
existing cosine scan. Dot-query heaps use larger integer keys, bounded by the
same candidate budget. The normal cosine method stays `Index::search()`.

## Behavioral tests

New library tests were written first and failed compilation because the method
was absent (`target/dot-product-red.log`). The implementation then passed:

- All 31,744 nonnegative finite FP16 encodings: exact integer unit conversion,
  full rounding-cell coverage, and safe multiplication by both i64 extremes.
- Positive, negative and zero-centered intervals across normal, subnormal,
  zero-underflow and maximum magnitudes; tiny and maximum valid query scales.
- Eight queries over 65 varied-length rows, appended in two segments, with
  worker counts 1/6 and four k/budget combinations: independent checked scalar
  primary and refined weighted rankings, displayed scores and original FP64
  dot enclosures agree, including k greater than row count and partial tiles.
- A long vector that loses cosine candidate selection wins dot search even with
  budget 1. A negative query correctly prefers a less negative dot score;
  positive query rescaling doubles its reported score. Hits survive index drop.
- Invalid options/vectors fail; concurrent cosine and dot queries complete, and
  interleaving them preserves cosine row IDs, raw scores and interval values.
- Unit stored lengths retain cosine row order over five candidate budgets.
- CLI qualification output contains every independently required schema field;
  wrong hashes fail before index creation, original-dot intervals enclose truth,
  and an existing output is preserved. Dot latency has a distinct metric kind.

An initial CLI test caught swapped schema/instance arguments in the new benchmark
writer. It was corrected before any recorded measurement; the focused regression
passes. The final CI gate passes 213 tests with 12 explicitly skipped large
qualifications. Workspace tests and strict all-target clippy also pass. Measurement
source identity and results are recorded below.
Local focused logs: `target/dot-product-focused.log` and
`target/dot-product-bench-tests.log`.

## Real vectors with meaningful varied lengths

We acknowledge GroupLens Research's [MovieLens 100K dataset](https://grouplens.org/datasets/movielens/100k/)
and Harper and Konstan (2015), *The MovieLens Datasets: History and Context*,
[DOI](https://doi.org/10.1145/2827872). Ratings and generated factor bytes remain
local research inputs under the upstream terms, with no redistribution here.

The original download host had an expired TLS certificate. The public ZIP was
retrieved with certificate verification disabled, then checked against the
independently published SHA1 `cd4dcac4241c8a4ad7badc7ca635da8a69dddb83` in
[Dive into Deep Learning's dataset recipe](https://d2l.ai/chapter_recommender-systems/movielens.html)
before reading any member. The reproducible builder requires the resulting
SHA256 `50d2a982c66986937beb9ffb3aa76efe955bf3d5c6b761f4e3a7cd717c6a3229`.
It reads only the known ratings member, never executes archive contents or
extracts arbitrary paths.

[Builder](../../tools/build_movielens_dot_corpus.py) uses NumPy 1.26.4 on Python
3.12.3 with one BLAS thread. It forms the zero-filled 943-user by 1682-item
rating matrix, takes a fixed untuned rank-64 SVD, canonically signs the factors,
and uses symmetric square-root singular-value scaling. Both factor types are
zero-padded to 768 and stored as FP32 without normalization. Their dot products
approximate that matrix; normalization changes this model's scoring objective.

BLAKE3 ordering selects 512 item factors for codec calibration and the remaining
1170 for indexing, with 200 user-factor queries. Codec training and indexed
items are disjoint. Calibration's default 128-row validation split leaves 384
PQ training rows. The embedding factorization itself uses all ratings; this is
**original-factor retrieval accuracy, not held-out recommendation relevance**.
It is a small real-derived 64-factor workload padded to the fixed 768D format,
not evidence about arbitrary native 768D models or a claim that length always
encodes popularity. No model rank, split, training size or budget is selected
using these query outcomes. BLAS/platform changes may alter floating-point bytes;
use the pinned split hashes in the [descriptor](../../corpora/movielens/movielens-svd64-dot-768.json).

| Split | Rows | Minimum norm | Median norm | Maximum norm |
|---|---:|---:|---:|---:|
| Indexed | 1170 | 0.015151 | 0.941998 | 6.665925 |
| Calibration | 512 | 0.025685 | 0.916403 | 6.668761 |
| Queries | 200 | 0.743657 | 2.001953 | 6.262024 |

An exact-rational check of the FP64 error formulas gives a bridge factor
3.4150460237476373e-13, below the implemented 2^-40 allowance
9.094947017729282e-13.

An independent NumPy FP64 exhaustive dot-product ordering was generated from
these pinned FP32 inputs before measurement. The Rust command separately computes
FP64 FMA truth outside timing. The final audit compared every query's exact
row list, recomputed reported recall and checked every emitted enclosure.
The pinned premeasurement oracle has BLAKE3
`c1d6aa669c4ce88098764458796e4ec00a679755f5dd7180adeea7620ed8295a`;
its minimum exact top-10 boundary gap is 0.000984910107732162.

## Clean-release protocol

Run `spherra-bench dot-product` with the three pinned paths, counts and hashes,
seed 20260804, k10 and budget200. It builds one new index, measures public dot
and cosine search, and records every dot hit and both methods' recovery of the
exact dot-product neighbors. The cosine comparison measures a different scoring
objective; it is not a regression in cosine quality. Per-query timing here is
small-workload diagnostic timing, without a formal warmup or latency gate.

Separately reuse the unchanged generated 1M index for serial cosine and
`dot-product-latency` runs, each 1000 queries plus 50 warmups, six workers,
seed 20260804 and default budget200. Use a 10-query/one-warmup 10M dot run only
for opening/search/resource compatibility. It cannot qualify p99 or a 10M SLO.
Both latency commands now share a metric-dispatch wrapper; the measured region
still includes the full public search call and excludes result-count checking.
The production cosine scan/refinement code is unchanged.

All admissible runs must use one clean, unchanged release commit. Initial output
belongs under ignored `target/measure/`, then JSON and process logs are copied
unchanged into `results/` after validation. The new method's initial 1M latency
is checked against the existing numerical targets; this is workload-specific.

## Completed measurements

All four runs used clean release commit
`12ef3ec9a2f41c29c5419b12cb0c02ebd96d6eed`, with no tracked changes during
measurement. The 1M/10M latency runs used AC power on the M1 Pro. Final documentation
updates are subsequent; serving code is the measured code.

| Real-factor retrieval, k10 / budget200 | Result |
|---|---:|
| Queries / checked dot hits | 200 / 2000 |
| Dot search recall against exact original dot | **0.9765 (1953/2000)** |
| Cosine recall against that dot objective | **0.4230 (846/2000)** |
| Original-dot enclosure failures | **0** |

This demonstrates why length must enter search when the intended score is a
dot product. It does not improve cosine reconstruction or show better held-out
recommendations. The 47 dot-product retrieval misses have not been decomposed
into selection, direction compression and length-rounding losses in this
checkpoint. The method remains approximate, and native 768D real dot models
and larger real corpora remain unqualified.

| Clean release measurement | Cosine 1M | Dot 1M | Dot 10M resource smoke |
|---|---:|---:|---:|
| Queries / warmups | 1000 / 50 | 1000 / 50 | 10 / 1 |
| p50, ms | 58.087916 | 59.534167 | 538.795166 |
| p99, ms | 156.388958 | 152.338958 | 578.149791 (10 samples only) |
| Open time, seconds | 1.069821458 | 0.956851542 | 12.148194042 |
| Peak open/search RSS, bytes | 401,702,912 | 400,834,560 | 3,872,342,016 |
| Retained descriptors | 17 | 17 | 161 |
| Latency qualification | Passed | Passed initial targets | Ineligible; resource smoke only |

Both 1M runs meet 150 ms median / 300 ms p99. Their serial timings are not a
paired overhead experiment: p99 and peak-RSS differences include normal host
and allocator variation. The 10M memory observation is below 20 GiB; the short
run deliberately reports `gate_eligible: false`, `gate_passed: false`. It is not
a failed full gate or a 10M tail-latency claim. All three reused the existing
unchanged indexes, verified CURRENT/model/build provenance, and retained exactly
segment count plus one descriptors. No index rebuild was required.

A separate NumPy/Python audit passed after the runs. It checked all three input
hashes, all 200 exact rankings against the premeasurement oracle, all 2000 hit
truths and norm roundings, every enclosure, and the recall totals. It recomputed
all latency percentiles from raw samples, matched RSS to process logs, checked
clean source/gate/workload flags and compared source, corpus, model, training,
query and CURRENT identities with the previous stored-magnitude measurements.
Local audit: `target/audit-dot-product-results.py`, output
`target/dot-product-audit.json`. The numerical error allowance was also checked
with exact rational arithmetic as described above. No independent-agent review
or new fuzz campaign is claimed.

Raw evidence, archived byte-for-byte:

- [Real-factor JSON](results/2026-09-14-dot-product-movielens.json) and
  [process log](results/2026-09-14-dot-product-movielens.time.txt). This log's RSS
  covers the whole build/qualification process, not open/search-only memory.
- [Cosine 1M JSON](results/2026-09-14-dot-product-cosine-latency-1m.json) and
  [process log](results/2026-09-14-dot-product-cosine-latency-1m.time.txt).
- [Dot 1M JSON](results/2026-09-14-dot-product-latency-1m.json) and
  [process log](results/2026-09-14-dot-product-latency-1m.time.txt).
- [Dot 10M resource JSON](results/2026-09-14-dot-product-resource-10m.json) and
  [process log](results/2026-09-14-dot-product-resource-10m.time.txt).

## Reproduction

Install NumPy 1.26.4 and blake3 1.0.8 in an isolated Python 3.12 environment.
Acquire the upstream archive under its terms and use the pinned archive builder:

```bash
OPENBLAS_NUM_THREADS=1 VECLIB_MAXIMUM_THREADS=1 python tools/build_movielens_dot_corpus.py \
  --archive target/dot-product-data/ml-100k.zip \
  --output-dir target/dot-product-data/movielens-svd64
cargo build -p spherra-bench --release --locked
```

Each result JSON records the exact benchmark command, all input hashes and
workload options. Execute the commands from the measured clean commit, using a
fresh index/output path for the real-factor command. The descriptor records all
split bytes and the reconstruction recipe. Existing generated 1M/10M indexes
must have the recorded build sidecars and CURRENT hashes to pass reuse checks.

The checkpoint is complete: source, tests, guide, status and schemas
are current. Existing compressed-only cosine behavior is retained, while users
can opt into length-aware dot-product search on the same saved index.
