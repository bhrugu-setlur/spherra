# Local index status

Updated: 2026-09-14. Branch/worktree: `local-index` / `.worktrees/local-index`.

Tasks 1–11, 13 and 14 are complete. Task 12 was skipped because stage 2 met
every latency gate. Documentation is complete.
The approved budget-20 factual correction is applied; the algorithm and default
budget of 200 are preserved.

The [test technical note](docs/benchmarks/2026-09-13-local-index-test-note.md)
records the delivery test data, setup, provenance and limitations. It consolidates
existing measurements; no new performance or quality run was made for the note.

## Completed accuracy investigation

The [accuracy technical note](docs/benchmarks/2026-09-13-local-index-accuracy-results.md)
records the separately authorized diagnosis, real-data pipeline, training sweep
and original-vector reranking prototype. Generated 1M and real 100k queries all
have 100% true-neighbor coverage at budget 200. The real tuning rule retained
4096 training inputs; untouched final recall is 0.9705 (1,941/2,000), with all
59 misses in compressed ranking. Larger pools and training sizes show no
reliable improvement. Original reranking recovers every observed true neighbor;
the warm-cache 1M prototype adds a median paired 3.223 ms and needs 3.072 GB
of extra originals per million rows. Exact vector ranking does not improve
sparse relevance-label scores on this sample. Production contracts/defaults
are unchanged. All nine complete candidate traces passed independent audits.

## Completed checkpoint: vector-length audit

The user authorized a pre-ingestion/benchmark length diagnostic. It streams
hash-pinned FP32 rows, reports FP64 norm distributions and FP16 underflow,
checks explicit unit-length/reference policies and reports invalid inputs.
Focused tests and full verification pass (201 CI tests, 12 skipped; workspace
tests and strict clippy). An existing CRC test received one transient nextest
LEAK flag; its isolated rerun passed cleanly. All four real splits and seven synthetic runs are recorded at clean `ca10f76`;
an independent calculation verified their statistics, counts and identities.
No public index API, search path or durable format changes are included.
[Protocol and results](docs/benchmarks/2026-09-14-vector-length-audit.md).

## Completed checkpoint: return stored magnitude

The user directed continuation to item 2 of the magnitude feature list.
`Hit::stored_magnitude()` and bounded checked loading of a two-byte-per-row cache
are implemented. Focused tests cover roundtrip/append, malformed magnitudes,
exactly unchanged scoring and bounded reads. CI passed 205 tests with 12 skipped;
workspace tests and strict clippy pass. Clean release `1a4b73d` passes the full
1M latency gate (54.891 ms median, 187.472 ms p99). A 10-query 10M resource smoke
opens existing bytes without rebuilding, peaks at 3,874,209,792 bytes RSS and
retains the expected 161 descriptors; it is not a new 10M tail-latency qualification.
The cache payload is 20 MB at 10M rows.
[Test data and raw results](docs/benchmarks/2026-09-14-stored-magnitude-results.md).
[Contract](docs/design/2026-09-14-stored-magnitude-amendment.md).

## Acceptance evidence

| Gate | Result |
|---|---|
| 1M latency, 1,000 queries | p50 52.622 ms; p99 147.278 ms — pass |
| 10M latency, 1,000 queries | p50 458.146 ms; p99 616.475 ms — pass |
| 10M open/search peak RSS | 3,855,040,512 bytes — below 20 GiB |
| Builder-owned peak estimate | 629,191,680 bytes — below 2 GiB |
| Retained descriptors, 1M / 10M | 17 / 161 — exactly segment count + 1 |
| Archived SciFact recall@10 | 0.9750 vs historical 0.9745 — pass |
| Generated 20k recall@10 | 0.9355 vs historical 0.9370 — pass |
| Chunked generated 1M recall@10 | 0.9095 against the previously pinned exact reference |
| Public quality, 6,000 hits | zero integer-score/rank differences; zero enclosure failures |
| Tile differential qualification | 2,073,760 primary scores; zero differences |

Timing commit: `84065a9`; quality commit: `9a48a77`. Both used clean release
builds. [Full protocol, raw timing samples and every quality hit](docs/benchmarks/README.md).
The 1M oracle artifact was pinned at `2ad9a34`, before index measurement.

The latest full quality gate passed 213 tests with 12 explicitly skipped large
qualifications. Workspace tests and strict all-target clippy pass. The release
kernel qualification passed on archived SciFact and generated 100k. Earlier
builder, recovery and decoder-fuzz qualifications remain recorded in the task
history and source tests; detailed per-task notes are preserved in Git history.

## Independent review

Independent implementation review of `bcf2a32` found no blocking issue. It covered
public contracts, publication/recovery, bounded formats and restoration, checked
opening, certificate binding, worker heaps, the tile kernel, streaming oracle and
measurement fidelity. Fresh `cargo test -p spherra -p spherra-codec -p
spherra-format --release --locked` passed. The reviewer independently verified
all four latency reports and all 6,000 quality-hit records. It did not rerun the
ignored large qualifications or fuzz campaigns.

Final documentation review found no blocking issue. Its two minor wording
corrections clarify the `2^48 - 1` row limit and the minimum reliable norm.
README examples compile as doctests. The final CI gate passed 191 tests with
12 skipped; workspace tests and strict all-target clippy passed.

## Limits and next work

- Timing is qualified on the M1 Pro and generated data; 10M recall is unmeasured.
- SciFact is a small real corpus; these are held-out vector-neighbor comparisons.
- Durability evidence covers local APFS fault injection and process death, not
  physical power loss or other filesystems. ASan runtime is unavailable here.
- No unsafe/NEON kernel, routing, deletes, filters, compaction or server is added.
- The original delivery plan is complete. The separately authorized accuracy
  investigation above is complete. The user declined original-vector reranking
  because of its storage/read tradeoff; compressed-only search and budget 200
  remain the chosen behavior. The prototype is retained as experiment evidence.
- The separately authorized vector-length audit above is complete; public commit-report integration remains separate work.
- The stored-magnitude getter checkpoint is complete. The additional dot-product
  method is the separately authorized completed checkpoint below.

## Completed checkpoint: additional dot-product search

The user requested a length-aware method while preserving current search.
`search_dot_product()` is implemented with full-scan magnitude weighting,
exact integer comparisons, distinct result types and original-dot intervals.
Focused scalar, enclosure, negative-score, scaling and cosine-isolation checks
pass. Full CI passes 213 tests with 12 skipped; workspace tests and strict clippy
pass. Clean release `12ef3ec` recovers 1953/2000 exact dot neighbors (0.9765)
on the real MovieLens factor workload, versus 846/2000 (0.4230) for cosine
against that dot objective; all 2000 original-dot intervals enclose truth.
The 1M dot run passes initial latency targets at 59.534 ms median / 152.339 ms
p99; cosine remains qualified at 58.088 / 156.389 ms. A 10-query 10M dot smoke
uses 3,872,342,016 bytes peak RSS with 161 descriptors, without a tail-latency
claim. Existing indexes were reused. Independent numerical audits pass and
[Test data and raw results](docs/benchmarks/2026-09-14-dot-product-search-results.md).
[Contract](docs/design/2026-09-14-dot-product-search-amendment.md).

Follow-up at clean `31f1466`: cosine and dot search share one scan/refinement
routine (cosine rows, scores and intervals unchanged; CI 213 passed / 12 skipped;
MovieLens hits byte-identical). On 1M native 768D DPR passage vectors with 1000
encoded NQ questions, dot recall@10 is 0.9024 with zero enclosure failures,
flat from budget 50 to 2000; cosine search on the same corpus reaches 0.9109
against exact cosine, and exact-direction/FP16-length ranking reaches 0.9943.
1M latency passes: cosine 57.984 / 144.235 ms, dot 58.658 / 174.376 ms.

Dot search remains approximate; 10M dot recall and other native dot models
remain unmeasured.

## Completed experiment: reconstruction renormalization (bench-only)

At clean `fcaf0a8`, dividing refined candidate scores by reconstruction length
raised recall@10 from 0.9095 to 0.9255 (generated 1M), 0.9705 to 0.9770 (MS
MARCO test) and 0.9024 to 0.9235 (DPR 1M dot), with no stored bytes. The user subsequently approved adoption; the new checkpoint below supersedes
this experiment-only serving decision.
At clean `7a526ab`, simulated option 2 (stored build-time alignment: exact,
FP16 or one byte) matched option 1 within noise on all three workloads, because
reconstructions align with their originals to at least 0.994; it is not worth a
format change.
[Results](docs/benchmarks/2026-09-14-renormalization-experiment.md).

## Current checkpoint: reconstruction-length correction

The user approved Option 1 in production. Both cosine and dot finalists now use
Q24 scores divided by the reconstructed compressed length; the primary scan,
budget and index bytes are unchanged. Raw-score truth certificates are retained.
Independent scalar and benchmark checks cover the corrected ranking. CI passes
218 tests with 12 skipped; strict clippy and the release full-corpus scalar
qualification pass. The separate workspace gate and clean release production
quality/latency measurements are in progress.
[Contract](docs/design/2026-09-14-reconstruction-length-amendment.md).
