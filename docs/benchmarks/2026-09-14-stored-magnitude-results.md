# Stored magnitude: implementation and test data

Date: 2026-09-14. Worktree: `.worktrees/local-index`, branch `local-index`.

## Change and scope

The [user-directed amendment](../design/2026-09-14-stored-magnitude-amendment.md)
adds `Hit::stored_magnitude() -> f32`. It returns the existing FP16 stored length,
promoted to FP32. Opening retains two bytes per row (20,000,000 bytes at 10M,
about 19.07 MiB), plus per-segment slice headers and allocator overhead. At most
4096 four-byte words are read at once; the temporary byte and word buffers each
use at most 16 KiB. Primary files still close; descriptor ownership is unchanged.

The value is metadata. Cosine scan/refinement, integer ordering, intervals,
budget 200 and durable files/identities are unchanged. No original vectors are
retained. The API can report zero for tiny positive inputs that rounded down.
It does not estimate semantic confidence or improve reconstruction accuracy.

## Behavioral verification

Tests were added before implementation; library compilation failed because the
getter and cache were absent (`target/stored-magnitude-red.log`). Focused tests
then passed. The four new tests cover:

- Actual axis-aligned FP32 inputs of lengths 1, 1.0001, 14, 1e-10, 1e-7 and
  65504: create three rows, append three, reopen with one and six workers.
  Magnitudes match domain FP16 rounding bit for bit; cache payload is 12 bytes.
  All six cosine scores tie in row order and enclose truth 1. An owned hit keeps
  its length after the index is dropped. Length 1e-10 rounds to stored zero.
- Rehashed segment/manifest/CURRENT fixtures containing FP16 negative zero,
  negative one, infinity or NaN fail opening with `Error::Corrupt`.
- Eight queries over 33 rows in two segments: replacing only stored magnitudes
  with alternating zero/maximum finite values preserves all 264 returned row
  IDs, integer scores and interval endpoint bits. This intentionally rehashed
  fixture isolates score independence; it is not a valid original-norm claim.
- A 4101-row format fixture: ranges (0,4096), (4096,5) and (2,31) each use one
  positional read and preserve every byte. Empty, oversized, past-end and
  overflowing ranges fail before any read.

The complete CI gate passed 205 tests with 12 explicitly skipped qualifications.
Workspace tests and strict all-target clippy also pass. Raw local
logs are `target/stored-magnitude-{focused,ci,workspace,clippy}.log`.
Golden-format tests are included in the normal gates; no golden bytes changed.

## Clean-release resource protocol

Reuse the existing generated correlated 1M and 10M indexes, preserving their
original build provenance and validating CURRENT. Use the unchanged latency
harness, seed 20260804, 4096 training rows, six workers, k10 and budget200.
Measure 1M with 1000 queries and 50 warmups; measure 10M with 10 queries and one
warmup as an opening/memory/compatibility smoke. The latter cannot qualify p99
or renew the full 10M latency gate. Run serially from a clean release commit,
with output under ignored `target/measure/`; archive JSON and process logs after
both finish.

## Recorded results

Both serial runs completed at clean release commit
`1a4b73d413936eeaaa86d9b32cdae3a643ea5d48`, unchanged throughout, on the M1 Pro
with AC power. Existing indexes were reused without rebuilding or modifying
CURRENT. Their original build commits remain recorded separately in the JSON.

| Measurement | 1M qualification | 10M resource smoke |
|---|---:|---:|
| Measured queries / warmups | 1000 / 50 | 10 / 1 |
| Open time, seconds | 1.078291125 | 10.491265459 |
| p50, milliseconds | 54.890916 | 510.824500 |
| p99, milliseconds | 187.472417 | 621.427625 (10 samples only) |
| Peak open/search RSS, bytes | 400,834,560 | 3,874,209,792 |
| Segments / retained descriptors | 16 / 17 | 160 / 161 |
| Formal latency gate | Eligible, passed | Ineligible, not a qualification |

The 1M gate remains below its 150 ms median and 300 ms p99 limits. The 10M smoke
remains below 20 GiB process memory and exercises existing-index opening and
search with the new cache; its short sample cannot establish tail latency. Its
JSON deliberately records both `gate_eligible: false` and `gate_passed: false`.

For context, the prior full 1M run at `84065a9` recorded 52.621834/147.277750 ms
and 399,638,528 bytes RSS; the prior full 10M run recorded 3,855,040,512 bytes.
These are separate runs, not a paired overhead experiment. Their timing/RSS
differences include ordinary system and allocator variation; the exact 20 MB
cache payload is established by representation, not by subtracting peak RSS.
The previous 1000-query 10M qualification remains historical evidence.

A separate Python check recomputed both percentiles from all raw samples,
matched RSS against `time -l`, checked clean source and gate flags, verified
segment-plus-one descriptors and matched source, corpus, model, training,
build commit and CURRENT against the archived stage-2 reports. The 1M query
hash also matches that full run. The 10M smoke has a different query hash
because it measures only the first ten queries. No recall improvement or new
large-corpus ranking measurement is claimed.

Raw evidence (copied byte-for-byte from the recorded output paths):

- [1M JSON](results/2026-09-14-stored-magnitude-latency-1m.json) and
  [process log](results/2026-09-14-stored-magnitude-latency-1m.time.txt).
- [10M JSON](results/2026-09-14-stored-magnitude-resource-10m.json) and
  [process log](results/2026-09-14-stored-magnitude-resource-10m.time.txt).

Reproduction after a release build, from the measured commit:

```bash
target/release/spherra-bench latency --rows 1000000 --queries 1000 --warmup 50 \
  --training-rows 4096 --seed 20260804 --index-dir target/local-index-1m \
  --reuse true --output target/measure/2026-09-14-stored-magnitude-latency-1m.json
target/release/spherra-bench latency --rows 10000000 --queries 10 --warmup 1 \
  --training-rows 4096 --seed 20260804 --index-dir target/local-index-10m \
  --reuse true --output target/measure/2026-09-14-stored-magnitude-resource-10m.json
```

The checkpoint is complete, and the project status is refreshed with
this evidence. The existing real MPNet corpora round to length 1; a meaningful
varied-length real retrieval dataset and a separate score/API design are still
needed before any dot-product feature. Neither is needed to expose stored
length as metadata.
