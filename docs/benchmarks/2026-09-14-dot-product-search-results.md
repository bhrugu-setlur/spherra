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
source identity and results will be recorded below.
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
FP64 FMA truth outside timing. The final audit will compare every query's exact
row list, recompute reported recall and check every emitted enclosure.

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
