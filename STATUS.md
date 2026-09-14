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

## Current checkpoint: return stored magnitude

The user directed continuation to item 2 of the magnitude feature list.
`Hit::stored_magnitude()` and bounded checked loading of a two-byte-per-row cache
are implemented. Focused tests cover roundtrip/append, malformed magnitudes,
exactly unchanged scoring and bounded reads. CI passed 205 tests with 12 skipped;
workspace tests and strict clippy pass. Clean-release resource measurements are
in progress.
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

The latest full quality gate passed 201 tests with 12 explicitly skipped large
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
