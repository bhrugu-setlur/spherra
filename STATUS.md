# Project status

Updated: 2026-09-23

Spherra's local index is complete: building, appending, crash-safe commits,
cosine search, optional dot-product search, and proven score ranges all work and
pass CI (223 tests; 12 large qualifications and one timing diagnostic are
skipped by default).

An earlier design for a distributed vector database was stopped in favor of this
local library. Its documents are kept in [`docs/design/archive/`](docs/design/archive/)
for context.

## Current results

Measured on an Apple M1 Pro (6 performance + 2 efficiency cores, 32 GiB RAM),
against exact search.

| Workload | Rows | Recall@10 | Median / p99 latency |
|---|---:|---:|---:|
| Generated correlated cosine | 1,000,000 | 0.9255 | 72.3 / 113.6 ms |
| MS MARCO cosine (real queries) | 100,000 | 0.9770 | — |
| DPR dot product (native 768D) | 1,000,000 | 0.9234 | 73.0 / 108.2 ms |

| Resource check | Result |
|---|---|
| 10M rows, open + search peak memory | 3.86 GB (limit 20 GiB) |
| Builder peak memory estimate | 0.63 GB (limit 2 GiB) |
| 10M latency, before length correction | 458 ms median / 616 ms p99 (limits 1.5 s / 3 s) |
| Score ranges checked against exact scores | 14,000 hits, zero misses |
| Fast scan kernel vs. reference scorer | 2,073,760 scores, zero differences |

Full protocols and raw results: [`docs/benchmarks/`](docs/benchmarks/README.md).

## Design decisions

- **Faster primary scan.** Eligible queries use an exact compact lookup table
  and fixed full-tile loops, adding 48 KiB per query and no stored bytes.
  Paired scan timing falls 14%; 1M search median falls from 94.7 / 101.7 ms to
  86.8 ms under background CPU load. Tail-latency improvement is unproven.
  Scalar scores and complete-search results match exactly.
  [Verification and measurements](docs/benchmarks/2026-09-23-scan-loop-results.md).
- **Compressed-only search.** Re-scoring finalists against the original vectors
  recovered every missed neighbor, but it would add 3.07 GB of storage per
  million rows and about 3.2 ms per query. I kept search compressed-only, with a
  candidate budget of 200. The prototype remains as
  [experiment evidence](docs/benchmarks/2026-09-13-local-index-accuracy-results.md).
- **Length correction.** Dividing refined scores by the rebuilt vector's length
  raised recall@10 from 0.9095 to 0.9255 (generated), 0.9705 to 0.9770
  (MS MARCO) and 0.9024 to 0.9234 (DPR), with no extra stored bytes.
  [Results](docs/benchmarks/2026-09-14-reconstruction-length-results.md),
  [design](docs/design/2026-09-14-reconstruction-length-amendment.md).
- **Stored length and dot-product search.** Each row keeps its FP16 length, so
  `search_dot_product` can rank by true dot product without storing originals.
  [Results](docs/benchmarks/2026-09-14-dot-product-search-results.md),
  [design](docs/design/2026-09-14-dot-product-search-amendment.md).
- **No unsafe SIMD kernel.** The safe tiled scan kernel met every latency limit,
  so the planned unsafe NEON kernel was not needed.
- **Indexed-query exclusion.** The caller can pass the index-assigned ID to
  `search_excluding` or `search_dot_product_excluding` so the query vector does
  not take a result or candidate slot. Other vectors with identical values
  remain eligible. The [design amendment](docs/design/2026-09-23-query-vector-exclusion-amendment.md)
  changes no stored bytes or ordinary search behavior.

## Limits

- 10M-row recall has not been measured, and 10M latency has not been re-measured
  since the length correction.
- Latency of the new indexed-query exclusion methods has not been measured on
  the 1M or 10M benchmark workloads.
- Durability is tested on local APFS with injected faults and killed processes,
  not physical power loss or other filesystems. AddressSanitizer does not run on
  this macOS host, so fuzzing ran without it.
- There is no deletion, filtering, compaction, routing or server.

## Next work

- Repeat scan comparisons on an idle machine; measure cold caches and memory pressure.
- Build larger labeled query sets before claiming 10M-row recall.
- Compare against an established library such as FAISS on the same data.
