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
both finish. Measurements and source identity will be appended at completion.
