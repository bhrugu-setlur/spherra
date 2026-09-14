# Vector-length input audit

Status: complete. Code and clean measurements: `2c4358f`.

This checkpoint's goal: diagnose vector-length anomalies
before ingestion while retaining compressed-only cosine search and budget 200.
This is `spherra-bench norm-audit`, not an extension to public commit reports.
The library API, stored model/segment bytes, rejection rules, scoring and
certificates remain unchanged.

## Input and statistics

Required arguments are `--input PATH --rows N --input-blake3 HASH --output PATH`.
Input is exactly N rows of 768 little-endian FP32 components, from 1 through 10M
rows. The command verifies byte length and the caller's lowercase BLAKE3 hash.
It streams original rows and retains one FP64 norm per finite row, at most 80 MB
of norm payload at 10M. It does not open an index or modify input files.

Norms use the same FP64 fused sum-of-squares and square root as domain
validation, before normalization and FP16 rounding. The report gives exact
nearest-rank min/median/p95/p99/max and near-unit fraction over **all finite
input rows**, including finite rows rejected by index validation. Statistics
are null if there are no finite rows. Row positions are zero-based.

Counts separately report non-finite components, norms below 1e-12, norms above
65,504, accepted rows, near-unit rows, near-zero rows, and positive norms that
round to zero in FP16. The maximum relative FP16 rounding error excludes zero
norms and FP16-overflow rows. At most eight invalid examples are retained.
An accepted direction can still have stored radius zero: for example 1e-10.

## Explicit policies and outcomes

- `--expect-unit true`: warn when the fraction outside `1 ± unit-tolerance`
  exceeds `max-outside-fraction`. Defaults: tolerance 0.0001 and fraction 0.01.
  Without this policy, arbitrary varying lengths are not a normalization error.
- `--near-zero VALUE`: warn for any finite norm at or below this threshold;
  default 1e-6. This is a numerical advisory, not a claim about text quality.
- `--baseline REPORT`: compare current median and p95 with a prior
  input audit's statistics. The reference must have no invalid rows and positive
  median/p95. Its report hash and input hash are retained. Warn when either
  ratio is strictly outside `[1/shift-factor, shift-factor]`; default factor 2.
  This compares batch distributions, not time series or model identities.
- Positive FP16 underflow always creates an advisory warning.
- Invalid rows always produce a report and nonzero exit. Warnings alone exit 0,
  unless `--fail-on-warning true` is supplied. `gate_passed` records this policy
  outcome; it does not certify model quality or guarantee successful ingestion.
- Invalid options, malformed/truncated input, hash mismatch and invalid baseline
  fail without a new report. Existing output paths are never overwritten.

Every output validates against [the strict schema](norm-audit.schema.json)
and records policies, input identity, source revision, machine, command and time.
Elapsed time is diagnostic bookkeeping, not a serving performance gate.

## Verification protocol

Focused tests cover mixed normalized/unnormalized rows, no unsolicited scale
warnings, deviations hidden by FP16 rounding, explicit strict-mode failures,
reference rescaling, tiny positive norms, underflow, zero/non-finite/overflow
inputs, all-invalid statistics, malformed framing, wrong hash and output
preservation. Full workspace CI, workspace tests and strict clippy passed.

Clean release evidence includes all four pinned MS MARCO splits and a
small explicitly synthetic anomaly suite. Synthetic length changes validate
policy mechanics; they are not a varied-length real retrieval benchmark or
evidence that magnitude indicates information, popularity or confidence.

## Example

```bash
cargo build -p spherra-bench --release --locked
target/release/spherra-bench norm-audit \
  --input target/accuracy/msmarco-100k-deduplicated/indexed.f32 \
  --rows 100000 \
  --input-blake3 5567023469b93d085f3c942479cfb0788d757ced07b96bd106cb1548e76e7e55 \
  --expect-unit true --fail-on-warning true \
  --output target/measure/norm-msmarco-indexed.json
```

A magnitude change can suggest an upstream preprocessing change. It cannot
identify its cause, detect every model change, or determine whether text is
meaningful. A normalized garbage-text embedding can still have length one.

## Implementation verification

Four focused contract tests pass. The complete CI gate passed 201 tests with
12 explicitly skipped large qualifications; workspace tests and strict
all-target clippy passed. Nextest marked the existing pure CRC-check test
`current_crc_is_checked_even_with_valid_trailing_hash` as `LEAK` once despite
passing; its isolated nextest rerun passed without that flag. The cause of
that transient runner flag was not established; no related source was changed.

## Recorded results

All 11 runs used the clean release revision `2c4358f`. An independent Python/
NumPy calculation verified input hashes, all rejection and underflow counts,
near-unit fractions and every reported norm percentile (within 1e-14 relative
or 1e-15 absolute tolerance for reduction-order differences).

| Input | Rows | Result |
|---|---:|---|
| MS MARCO indexed | 100,000 | Strict unit policy passes; no warnings |
| MS MARCO calibration | 32,768 | Strict unit policy passes; no warnings |
| MS MARCO tuning queries | 200 | Strict unit policy passes; no warnings |
| MS MARCO final queries | 200 | Strict unit policy passes; no warnings |
| Unit baseline | 100 | No warnings |
| Mixed lengths | 100 | Unit-length policy warning |
| 14× rescaling | 100 | Median and p95 scale-shift warnings |
| Varied lengths without unit policy | 100 | No warnings |
| Tiny positive vectors | 4 | Near-zero and FP16-underflow warnings |
| Invalid vectors | 6 | Five invalid rows; report written, exit 1 |
| Mixed lengths, strict mode | 100 | Report written, exit 1 |

The indexed real split spans approximately **0.9999999164–1.0000001123**,
with median **1.0000000259** before FP16 rounding. All 133,168 real rows pass
validation and the explicit unit policy. This audits the existing normalized
snapshot; it adds no new real-data retrieval or semantic-quality result.

Synthetic rows contain the listed value in coordinate 0 and zero elsewhere:
unit=100 copies of 1; mixed=90 copies of 1 plus 10 copies of 14; shift=100 copies
of 14; varied=integers 1 through 100; tiny=`[1e-10, 1e-7, 1e-6, 1]`;
invalid=`[0, 1e-13, 70000, NaN, +Infinity, 1]`. Values are encoded as FP32
before auditing. The tiny set has four accepted rows but one positive FP16
underflow; maximum relative rounding error is 1.0. The invalid set has two
non-finite rows, two below-minimum norms and one above-FP16-maximum norm.
Zero stored magnitude must therefore not be interpreted as proof of zero input.

Raw schema-validated JSON reports and expected exit codes are linked by the
[run inventory](results/2026-09-14-norm-run-inventory.json). Individual reports:
[indexed](results/2026-09-14-norm-msmarco-indexed.json),
[calibration](results/2026-09-14-norm-msmarco-calibration.json),
[tuning](results/2026-09-14-norm-msmarco-tuning.json),
[final](results/2026-09-14-norm-msmarco-test.json),
[unit](results/2026-09-14-norm-unit.json),
[mixed](results/2026-09-14-norm-mixed.json),
[shift](results/2026-09-14-norm-shift.json),
[varied](results/2026-09-14-norm-varied.json),
[tiny](results/2026-09-14-norm-tiny.json),
[invalid](results/2026-09-14-norm-invalid.json), and
[strict mixed](results/2026-09-14-norm-strict-mixed.json).

## Checkpoint boundary

This completes the authorized diagnostic. No magnitude cache, original-vector
storage, new search metric, flag assignment or public commit-report field was
introduced. A later application can run this audit before passing vectors to
`IndexBuilder`; integrating the report into that API is separate design work.
