# Search optimization follow-up checks

The compact scan remains worthwhile. On this loaded machine, it reduces median
1M search times by 11–14% with one to eight simultaneous callers, and by about
10% with cold residual-file pages. At tiny sizes, the extra setup costs only a
few microseconds. No additional serving change was made.

The user explicitly requested continuing under load and leaving the idle-machine
test pending. No unrelated workload was paused or stopped. These measurements
are paired comparisons, not idle-machine or tail-latency qualifications.

## Revisions and method

- Baseline: clean `1a4b9ab877b45c1b506710c6b713d93df448cb53`, containing the
  benchmark driver on top of `43c4e3f`. Its serving crates are unchanged.
- Optimized: clean `4e53aa36ad0210d3a72fac281a167a40544412eb`. Serving code is
  unchanged from `07a4c91`; subsequent commits add measurements and documentation.
- Both use identical Rust probe code, pinned Rust 1.88.0 release builds, and the
  same immutable index files for each pair. Executable SHA-256 hashes, model
  identities, source/build revisions, generator settings, query hashes and
  CURRENT hashes are retained in every raw report.
- Apple M1 Pro, 32 GiB, AC power. The VM and unrelated work continued. The earlier
  circuit-simulation processes were no longer present in the start/end snapshots;
  this task did not stop them. The VM used approximately 56–61% of one core in
  those snapshots. One-minute load averages varied from 28.47 to 11.29, including
  recent activity; they are not instantaneous CPU utilization. Swap was already
  in use. Builds and test runners from this task finished before timed trials.
- `search-probe` opens the index once and serves bounded JSON-line requests.
  It calls the public cosine or dot-product search with k=10, default budget 200,
  and the index's existing six scan workers. Caller threads share one `Index`.
  The two versions run sequentially in alternating order for each batch, with
  their index handles retained in separate processes. This retains two copies
  of primary tiles; these runs are not a memory measurement.
- Each caller times only the public search. A separate burst timer starts before
  the parent waits at the launch barrier and ends after joining all callers;
  it includes barrier and join overhead. Result hashing and JSON output occur
  after timing. Reported throughput is queries divided by summed burst times,
  not sustained arrival-rate capacity or throughput including the controller.
- Generated correlated 768D data, seed 20260804. Each probe hashes the same 256
  query rows. Warmups use IDs 248–255 as needed, three batches per caller count
  and metric. Timed batch IDs are `(batch * callers + lane) % 248`.

Index reuse validates source settings, training count and CURRENT against the
build sidecar; `Index::open` performs its normal full validation. The controller
compares identities exactly. Historical build duration/rate fields are retained
but excluded from equality because reading their JSON can change the final
floating-point bit. A regression test verifies that changed hashes, settings or
dirty-build status still fail comparison.

## Small indexes

Two hundred paired queries per metric and size, one caller, normal cache state.
Small indexes use 344 training inputs; the existing 1M index uses 4096. Each
old/new pair uses the same model, but results across sizes are not a controlled
training-size experiment. Values below are median milliseconds, old → new.

| Rows | Cosine | Dot product |
|---:|---:|---:|
| 1 | 0.5119 → 0.5165 | 0.5157 → 0.5179 |
| 16 | 0.5753 → 0.5777 | 0.5896 → 0.5929 |
| 31 | 0.6690 → 0.6730 | 0.6691 → 0.6709 |
| 32 | 0.6723 → 0.6689 | 0.6685 → 0.6677 |
| 33 | 0.6721 → 0.6730 | 0.6649 → 0.6653 |
| 256 | 1.4927 → 1.4937 | 1.5164 → 1.5149 |
| 4,096 | 1.8483 → 1.8089 | 1.8120 → 1.7842 |
| 32,768 | 3.9794 → 3.6251 | 4.1137 → 3.7853 |

Below one full tile, median overhead is approximately 2–5 microseconds, under
1%. The 32,768-row median improves by 8–9%. These results do not justify adding
a separate small-index preparation path.

## Simultaneous searches at 1M rows

Thirty-two paired bursts per caller count and metric, normal cache state.
Median latency pools all callers' individual search durations; queuing inside
the shared index is included. Values are old → new.

| Metric | Callers | Median latency, ms | Burst throughput, queries/s |
|---|---:|---:|---:|
| Cosine | 1 | 60.319 → 53.707 | 15.75 → 18.08 |
| Cosine | 2 | 100.538 → 88.596 | 17.13 → 20.05 |
| Cosine | 4 | 191.659 → 165.102 | 18.88 → 21.55 |
| Cosine | 8 | 356.985 → 308.628 | 19.97 → 23.10 |
| Dot product | 1 | 61.085 → 54.095 | 15.95 → 18.25 |
| Dot product | 2 | 101.299 → 89.756 | 17.37 → 19.54 |
| Dot product | 4 | 190.755 → 165.583 | 18.98 → 21.70 |
| Dot product | 8 | 366.236 → 318.275 | 19.20 → 22.45 |

The improvement persists with concurrent callers. More callers increase waiting
time and provide only a modest throughput gain; this is not evidence for raising
the internal worker count. [Raw paired bursts](results/2026-09-23-search-validation-paired-concurrent-1m.jsonl).

## Cold residual reads at 1M rows

Sixty-four paired queries per metric and cache condition, one caller. The warm
control primes the exact request immediately before timing each version. Before
each cold search, the external Python controller maps only the retained residual
files read-only, calls macOS `msync(MS_SYNC | MS_INVALIDATE)`, and verifies zero
resident pages with `mincore`. It then unmaps them before issuing the request.
The files are identified from the probe's retained descriptors and checked against
the index directory and segment count. No global cache purge is used.

All 256 cold searches passed this check: 16 files, 108,698,176 bytes, 6,643 pages,
zero resident pages before search, and 190–204 pages resident afterward. Cache
control occurs outside the timers. Primary tiles remain owned in memory. This
tests cold residual **file-cache pages**, not cold SSD/controller caches, opening
an index, or controlled memory pressure. Serving still uses no memory mapping.

| Metric | Cache | Old median, ms | New median, ms |
|---|---|---:|---:|
| Cosine | Warm control | 58.552 | 51.066 |
| Cosine | Cold residual pages | 80.810 | 73.041 |
| Dot product | Warm control | 57.821 | 50.044 |
| Dot product | Cold residual pages | 83.196 | 74.736 |

Cold residual reads add about 22–25 ms in these trials. The optimized version
still reduces median latency by 9.6% for cosine and 10.2% for dot product.
Warm and cold phases run sequentially, so background-load changes can also affect
their difference. [Raw samples and cache checks](results/2026-09-23-search-validation-paired-cold-1m.jsonl).

## Correctness, checks and next decision

Across all ten reports, 4,416 pairs / 8,832 timed searches returned matching
fingerprints, covering 81,120 returned hits across both versions. Fingerprints
include hit order, row/segment IDs, score bits, both interval endpoints, stored
magnitude bits, generation and budget. Every search also checks the full row
scan, expected refinement count and hit count. The concurrency controller checks
repeat-query consistency across caller counts. This is result equivalence on
these inputs, not a new recall or exact-truth experiment.

A separate five-second macOS stack sample of the optimized cosine probe found
17,385 of 17,551 sampled stacks inside the six workers' scan functions in tile
scoring (99.05%). This is sampled stack occupancy, including descheduled threads,
not a precise CPU-cycle breakdown or a heap-cost bound. It supports holding off
on candidate-heap changes. The profiler was not running during latency trials.
[Raw profile](results/2026-09-23-search-validation-optimized-profile.txt),
[profile provenance](results/2026-09-23-search-validation-profile-provenance.json).

Validation passed: full CI (224 tests, 13 skipped), workspace tests, strict
workspace/all-target Clippy, formatting, three Python controller/cache tests,
and an end-to-end controller smoke. The baseline independently passed full CI
(222 tests, 12 skipped). Graphify's code refresh and `check-update` completed
with no pending-update flag.

Keep the compact scan. Hold off on small-index special handling, heap changes
and worker-count changes. **The idle-machine comparison remains pending at the
user's request.** Controlled memory-pressure tests and new 10M qualification also
remain unmeasured. No new public API, index bytes, defaults or serving code were
changed by this follow-up.

[Summary and raw-file hashes](results/2026-09-23-search-validation-summary.json)
include every small-index result filename and additional paired statistics.
Raw reports retain all samples and provenance; only the local repository path
prefix was replaced with `${SPHERRA_ROOT}` in checked-in copies. Private host
process listings stay under ignored `target/measure/scan-validation`; the summary
retains aggregate load, power, swap and VM observations. Initial controller smoke
and interrupted pre-fix runs are retained there but excluded from these results.
