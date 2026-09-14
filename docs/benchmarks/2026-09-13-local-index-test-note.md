# Technical note: local index test and measurement results

Date: 2026-09-13 (America/New_York). Scope: the completed local embedded index
on branch `local-index`, through delivery commit `b8f99f5`. Some raw timestamps
are September 14 UTC; they belong to the same local test date.

The safe Rust tile kernel meets the approved 1M and 10M latency gates. The
builder and search memory gates pass. Public search matches the independent
checked-scalar ranking and scores on all 6,000 qualified hits, and every hit's
interval encloses original-space FP64 truth. This note consolidates existing
measurements; it does not represent a new benchmark run.

## Test setup and interpretation

| Parameter | Recorded setting |
|---|---|
| Hardware | Apple M1 Pro, 6 performance + 2 efficiency cores, 32 GiB RAM |
| Platform | aarch64, Darwin 26.6.2; local APFS |
| Compiler | Rust 1.88.0 (`6b00bc388`, 2025-06-23), release profile |
| Search | Six workers; k=10; candidate budget 200; one public search at a time |
| Vector representation | 768 dimensions; direct-int4 primary; PQ96x8 residual; `TILED_SOA_32`; scorer version 1 (Q24) |
| Latency sampling | AC power; 50 explicit warmup queries, then 1,000 timed queries per run |
| Percentiles | Nearest rank over the retained raw samples |
| Quality sampling | 200 held-out queries per corpus; 2,000 returned hits per corpus |
| Generated data | AR(1) coordinate correlation 0.85; root seed 20260804 |
| Training input | 4,096 rows for generated performance/quality runs; 1,295 for archived SciFact; separate validation holdout |

A timed search includes query preparation, exhaustive primary scanning,
candidate selection, residual reads, refinement, and final result construction.
It excludes index opening and building. The process RSS measurement has a
broader scope, described below. Warmup is explicit; these are not cold-cache
latency measurements. There is one full timing run per kernel and corpus size,
not a distribution of repeated independent runs.

Recall@10 is overlap with exhaustive original-space FP64 top-10 neighbors,
averaged over queries. Exact-reference ordering uses score descending, then row
ID ascending. Matching the scalar implementation proves implementation equality;
it does not make the compressed index an exact nearest-neighbor search.

## Latency results

| Rows | Checked scalar p50 / p99 | Safe tile p50 / p99 | Required p50 / p99 | Median speedup | Final gate |
|---|---:|---:|---:|---:|---|
| 1,000,000 | 630.172 / 807.444 ms | 52.622 / 147.278 ms | ≤150 / ≤300 ms | 11.98× | Pass |
| 10,000,000 | 6,438.614 / 6,974.116 ms | 458.146 / 616.475 ms | ≤1,500 / ≤3,000 ms | 14.05× | Pass |

Both checked-scalar runs missed their latency gates. The safe tile runs reused
the identical indexes; corpus, query, model and `CURRENT` hashes match between
stages. Every run retains all 1,000 timing samples. The safe tile kernel passed
without NEON or unsafe code, so the conditional third stage was skipped. The
codec, scoring algorithm and default budget of 200 were preserved.

| Evidence | Structured results | Raw process log |
|---|---|---|
| Scalar 1M | [JSON](results/2026-09-13-local-index-stage1-latency-1m.json) | [time log](results/2026-09-13-local-index-stage1-latency-1m.time.txt) |
| Scalar 10M | [JSON](results/2026-09-13-local-index-stage1-latency-10m.json) | [time log](results/2026-09-13-local-index-stage1-latency-10m.time.txt) |
| Safe tile 1M | [JSON](results/2026-09-13-local-index-stage2-latency-1m.json) | [time log](results/2026-09-13-local-index-stage2-latency-1m.time.txt) |
| Safe tile 10M | [JSON](results/2026-09-13-local-index-stage2-latency-10m.json) | [time log](results/2026-09-13-local-index-stage2-latency-10m.time.txt) |

The smaller [scalar worker probe](results/2026-09-13-local-index-stage1-workers.txt)
used 20 timed queries after five warmups at 1M rows. Four, six and eight workers
measured p50 853.208, 633.932 and 591.882 ms respectively. This probe changed no
default and does not replace the full acceptance runs.

## Build, open and memory results

| Rows | Build time | Build throughput | Safe tile open time | Segments / retained descriptors | Peak open/search RSS |
|---|---:|---:|---:|---:|---:|
| 1,000,000 | 85.259 s | 11,729.022 rows/s | 1.055 s | 16 / 17 | 399,638,528 B (0.372 GiB) |
| 10,000,000 | 837.817 s | 11,935.785 rows/s | 11.745 s | 160 / 161 | 3,855,040,512 B (3.590 GiB) |

Build times come from stage 1 and include generation, training, encoding,
verification, and file/directory synchronization. Stage 2 reused those builds.
Retained descriptors equal one per residual segment plus the permanent lock.
The 10M open/search process is below the 20 GiB RSS gate.

Stage 1 process peaks were 6,665,224,192 B at 1M and 7,354,368,000 B at 10M.
Those processes also built their indexes; stage 2 processes only opened and
searched them. Their RSS difference must not be attributed to kernel memory
savings. RSS is a process measurement, not total OS/page-cache usage.

The separate builder probe used the maximum 32,768 training inputs and pushed
65,537 rows, exercising a full segment and a partial segment:

| Memory component | Bytes |
|---|---:|
| Peak process RSS | 938,000,384 |
| Pre-input process RSS | 6,815,744 |
| Live caller-owned input allocations | 301,992,960 |
| Builder-owned estimate: peak − baseline − caller inputs | **629,191,680** |
| Builder-owned limit | 2,147,483,648 |

The estimate is about 600 MiB and passes the 2 GiB gate. Caller inputs remained
live through commit; the harness rejects a negative estimate instead of
clamping it. This is an RSS subtraction estimate, not allocator attribution.
See the [memory JSON](results/2026-09-13-local-index-build-memory.json) and
[raw time log](results/2026-09-13-local-index-build-memory.time.txt).

## Retrieval quality and score checks

| Corpus | Indexed rows | Exact top-10 matches / 2,000 | Recall@10 | Task 1 baseline | Change | Score/rank differences | Enclosure failures |
|---|---:|---:|---:|---:|---:|---:|---:|
| Archived SciFact / MPNet | 3,688 | 1,950 | 0.9750 | 0.9745 | +0.0005 | 0 | 0 |
| Generated correlated 20k | 20,000 | 1,871 | 0.9355 | 0.9370 | −0.0015 | 0 | 0 |
| Chunked generated correlated 1M | 1,000,000 | 1,819 | 0.9095 | Not measured | — | 0 | 0 |

Both historical comparisons pass the permitted absolute recall loss of 0.01
(one percentage point). The 1M result is measured against an exact reference
pinned before index measurement; it has no historical recall-loss comparison.
Each result file records every actual/expected row and integer score, interval,
FP64 truth, and per-query exact overlap:
[SciFact](results/2026-09-13-local-index-quality-scifact.json),
[20k](results/2026-09-13-local-index-quality-20k.json),
[1M](results/2026-09-13-local-index-quality-1m.json).

The independent reference restores checked model/segment data, scores every
primary code with checked scalar arithmetic, fully sorts the candidates, and
recomputes primary-plus-residual scores. It does not use the serving tile kernel
or its candidate heaps. All 6,000 hits match that reference and enclose truth.
The quality command also computes reference results, so its elapsed time is not
a public-search latency measurement.

Task 1's budget sweep measured generated 20k recall@10 of **0.9330 at budget
20**, increasing to **0.9370 at 200**, and unchanged at 1,000 and all rows.
Archived SciFact measured 0.9745 at all four budgets. The approved factual
correction and original data are in the
[reference note](2026-09-13-local-index-reference.md). The September archived
SciFact bytes differ from August's; August recall numbers are not interchangeable.

## Correctness, recovery and review evidence

| Check | Recorded outcome | Source / record |
|---|---|---|
| Delivery CI | 191 tests passed; 12 explicit large qualifications skipped in the normal run | [CI script](../../scripts/ci.sh), delivery logs under ignored `target/local-index-task14-*.log` |
| Workspace tests and strict clippy | Passed | `cargo test --workspace --locked`; `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| Public documentation contracts | Two README examples and four compile-fail API construction tests passed | [Public crate](../../crates/spherra/src/lib.rs) |
| Tile differential qualification | 73,760 SciFact + 2,000,000 generated-100k primary scores; zero differences | [Qualification](../../crates/spherra/src/kernel_qualification.rs) |
| Tile boundaries and fallback | Randomized tiles, active lane widths, malformed geometry, integer-range boundaries and checked fallback passed | [Kernel tests](../../crates/spherra-codec/src/kernel_tests.rs) |
| Builder qualification | Large builds, committed code/certificate equivalence, bounded drift sampling and maximum training input passed | [Builder qualification](../../crates/spherra/src/builder_qualification.rs) |
| Recovery qualification | Injected filesystem failures, short writes, poisoned staging, cleanup retries, child-process kills and aborts passed | [Recovery tests](../../crates/spherra/src/recovery_tests.rs) |
| Durable containers and capabilities | Golden bytes, malformed/checksummed-invalid inputs, pairing, certificate binding, locking and descriptor admission passed | [Container tests](../../crates/spherra/src/container_tests.rs), [open tests](../../crates/spherra/src/open_tests.rs) |
| Decoder fuzzing | Bounded no-sanitizer campaigns passed; no ASan runtime claim | [Fuzz targets](../../fuzz/fuzz_targets) |
| Independent implementation review | No blocking finding; reviewer reran release tests for `spherra`, codec and format, and checked all four latency reports and all 6,000 quality records | [Delivery status](../../STATUS.md) |

The skipped normal-suite qualifications were run separately during their
implementation tasks. The reviewer did not rerun the large ignored
qualifications or fuzz campaigns. The logs under `target/` are local artifacts,
not committed evidence; structured benchmark JSON and raw timing logs linked
above are committed. This distinction matters when reproducing from a clone.

## Provenance and reproduction

| Measurement | Clean source commit |
|---|---|
| Task 1 recall references | `caa8229418101dafa33749ca1148227a4dbcac7d` |
| Pinned 1M exact oracle | `2ad9a34df74bb84e3c1d8c629bb644201bcf3f07` |
| Builder memory and scalar 1M | `76408df13d7bfbb58a3b7c4bafb3d6b65b376a34` |
| Scalar 10M | `e96f56391d5cf003f2527648ff45602401807d94` |
| Safe tile 1M and 10M | `84065a963cbe6954979449d299ff53e7f07d4056` |
| Public quality, all three corpora | `9a48a77789a77f2c8afe0cffe5318b1332c4cf22` |

Every benchmark JSON retains its original command, full source revision,
clean-worktree flag, model identities, corpus/query hashes and machine record.
The archived paths differ from the initial ignored output paths printed in
those commands. The [benchmark protocol](README.md#local-index-measurement-protocol)
contains runnable build, oracle, latency, reuse and quality commands. Reproduce
from the appropriate clean commit; index reuse checks the recorded identities.

The [SciFact descriptor](../../corpora/archive/scifact-mpnet-768-2026-09-13.json)
pins 5,183 source rows, 15,922,176 FP32 bytes, and BLAKE3
`b2e549ce3a605944e44c21fb93bd912a130ffbe1ba914ac70befddf6eaa0e254`.
Its disjoint split has 3,688 indexed, 1,295 calibration and 200 query rows.
`IndexBuilder::create` further splits calibration into training and validation.
Generated 1M/10M sources use versioned 1M-row chunks with separate query and
calibration streams; all chunk seeds are retained in their JSON descriptors.

The [1M oracle report](results/2026-09-13-local-index-oracle-generated-correlated-1m.json)
pins top-100 rankings for 200 queries, generated in 73.761033 seconds. The
320,824-byte artifact has BLAKE3
`8c2446ed410e1fd3c104622567d9c5d048caf4c69d535fda1fd2431a9c498b38`.
The corpus bytes and oracle binary are ignored local files; the descriptors,
hashes and reports are committed. A reproduction must regenerate or obtain the
exact bytes and pass hash verification.

## Limits and decisions

- Timing is qualified on this M1 Pro with generated correlated data. Other CPUs,
  cold-cache operation and larger real corpora are unqualified.
- Recall at 10M was not measured. SciFact is a small real corpus, and held-out
  embedding-neighbor recall is not a published BEIR retrieval-task score.
- Certified intervals bound returned scores for intact, library-built indexes;
  they do not establish exact top-k recall or honesty after malicious rehashing.
- Recovery evidence covers local APFS fault injection and process death, not
  physical power loss or other filesystems. ASan stalls before target entry on
  this host; fuzz runtime evidence uses `--sanitizer none`.
- Safe tile is the delivered kernel. NEON, routing, deletes, filters, compaction
  and server/distributed operation remain outside this delivery.

## Simple Explanation

The index met its measured speed and memory targets while preserving the
reference implementation's results. Searching 10 million vectors took about
458 ms at the median and 616 ms at p99. Recall depends on the data; the largest
quality test was 1 million vectors, with 90.95% recall@10.
