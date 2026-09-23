# Safe primary scan optimization

Implementation: `07a4c91`. Baseline: `43c4e3f`. Both are clean release builds
with Rust 1.88.0 on the Apple M1 Pro / 32 GiB target.

## Change and arithmetic

An eligible prepared query keeps an additional 48 KiB primary lookup table with
the **same Q24 integers** stored as i32. Eligibility requires
`768 * maximum_absolute_entry <= i32::MAX`, evaluated in i128. This proves that
every entry and every coordinate-ordered partial sum fits, so narrowing loses
no information. The authoritative i64 table, comparison scale, identities,
certificates and stored index bytes are unchanged.

Full 32-row tiles use two fixed groups of 16 i32 accumulators and widen their
exact sums to i64. Partial tiles and queries without the compact table use the
previous i64 loop. The existing i128 proof for i64 admission and checked scalar
fallback remain. There is no unsafe code or handwritten SIMD. Inspection of
the M1 release assembly shows compiler-generated vector additions for the
compact loop. Existing indexes need no rebuild.

The cost is one additional 49,152-byte allocation and table conversion per
eligible query, shared by the scan workers. There is no additional per-row
resident or durable data. Small-index latency and dot-product latency were not
measured in this experiment.

## Measurements and limits

Other CPU-heavy jobs were active on the host, including several circuit
simulation processes and a virtual machine. They were left running. No Spherra
tests, builds or other Spherra measurements ran alongside these timings.
These are comparisons under background load, **not new isolated latency
qualifications**. In particular, do not compare them directly with September 14
measurements or claim a reliable p99 improvement.

The scan-only diagnostic alternates which path runs first in 20 paired trials.
Both paths use the same query/table values and 1,024 full tiles (32,768 rows),
with one warm scan per path. Disabling only the compact cache exercises the
existing i64 fallback. It checks equal checksums over all scores; independent
scalar equality is covered by the tests below. Query construction uses seed 42;
tile bytes use ChaCha20 seed 20260804 and their BLAKE3 is in the
[complete timing log](results/2026-09-23-scan-compact-kernel.txt).

- Median scan: **7.725 ms wide → 6.635 ms compact**.
- Median paired time reduction: **14.11%**; compact wins 18 of 20 trials.
- This excludes query preparation, candidate selection, residual reads and
  final ranking. It is a synthetic kernel diagnostic, not a retrieval workload.

Public cosine search, k=10, budget 200, six workers, AC power, warm caches:

| Run | Measured / warmup queries | Median | p99 | Peak open/search RSS |
|---|---:|---:|---:|---:|
| Baseline 1M | 1,000 / 50 | 94.740 ms | 138.265 ms | 401,162,240 B |
| Baseline 1M repeat | 1,000 / 50 | 101.668 ms | 158.226 ms | 402,292,736 B |
| Compact 1M | 1,000 / 50 | 86.799 ms | 147.257 ms | 401,080,320 B |
| Baseline 10M smoke | 100 / 10 | 807.739 ms | 1,032.038 ms | 3,869,065,216 B |
| Compact 10M smoke | 100 / 10 | 729.425 ms | 828.008 ms | 3,868,852,224 B |

The 1M median falls 8.4% against the first baseline and 14.6% against its
repeat. Whole-process user CPU time falls from 293.00 / 282.80 seconds to
244.97 seconds (13–16%); this includes open, warmup and query work, not just the
scan. The 1M p99 lies between the two baseline runs. The 10M median falls 9.7%,
but 100 queries do not qualify 10M tail latency or recall. RSS differences are
measurement variation, not evidence of a memory improvement.

All before/after workload fields match within each size: generated source,
corpus and query hashes, model identities, CURRENT hash, generation, training
settings, k, budget, workers and query/warmup counts. Index reuse validates the
build sidecar; no indexes were rebuilt. Percentiles were independently
recomputed from every report's samples. The 1M reports pass the harness's
numeric gates; that flag does not detect competing host jobs. The shorter 10M
runs correctly report `gate_eligible: false`.

## Correctness and reproduction

Full CI passes **223 tests**, with 12 large qualifications and the new timing
diagnostic skipped by default. Focused tests cover exact compact-table values,
the i32 admission boundary and its immediate neighbor, both signs, every
partial width, untouched output lanes, invalid geometry, and the i64 proof and
checked fallback. Randomized cases always test a full tile as well as a random
width. A separate near-limit test varies both lane and coordinate values.

The explicitly run release qualifications compare **2,073,760 primary scores**
on archived SciFact and generated 100k, with zero differences. Complete search
also matches scalar reference scores, order and intervals on SciFact and
generated 20k: 200 queries each, four k/budget combinations, zero differences
and zero interval enclosure failures. These establish equality, not a new
large-corpus recall measurement.

```bash
bash scripts/ci.sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p spherra --release --locked every_primary_score_matches_on_full_corpora -- --ignored --nocapture
cargo test -p spherra --release --locked checked_reference_search_full_corpora -- --ignored --nocapture
cargo test -p spherra-codec --release --locked --lib compare_compact_and_wide_scan_timing -- --ignored --nocapture
```

Use the [local-index measurement protocol](README.md#local-index-measurement-protocol)
for public latency runs. The reviewed reports below retain all samples and
commands. Only absolute workspace prefixes have been replaced with
`${SPHERRA_ROOT}/`; the original local reports remain under `target/measure/`.
No numerical or identity fields were changed.

| Run | Report | Process log |
|---|---|---|
| Baseline 1M | [JSON](results/2026-09-23-scan-baseline-1m.json) | [time](results/2026-09-23-scan-baseline-1m.time.txt) |
| Baseline 1M repeat | [JSON](results/2026-09-23-scan-baseline-repeat-1m.json) | [time](results/2026-09-23-scan-baseline-repeat-1m.time.txt) |
| Compact 1M | [JSON](results/2026-09-23-scan-compact-1m.json) | [time](results/2026-09-23-scan-compact-1m.time.txt) |
| Baseline 10M smoke | [JSON](results/2026-09-23-scan-baseline-10m-smoke.json) | [time](results/2026-09-23-scan-baseline-10m-smoke.time.txt) |
| Compact 10M smoke | [JSON](results/2026-09-23-scan-compact-10m-smoke.json) | [time](results/2026-09-23-scan-compact-10m-smoke.time.txt) |
