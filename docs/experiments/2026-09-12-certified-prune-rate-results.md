# Result: certified prune-rate experiment

**Date:** 2026-09-12
**Decision rule:** [`2026-09-12-certified-prune-rate-preregistration.md`](2026-09-12-certified-prune-rate-preregistration.md), written before any number below was produced.
**Verdict: FAIL.** The certified bounds cannot carry pruning load.

## Headline

| Corpus | N indexed | Certificate | Survivors p50 | Survivors p90 | Prune rate p50 |
|---|---|---|---|---|---|
| BEIR SciFact (real) | 3,688 | block | 2,530 | 3,688 (all) | 31.3% |
| BEIR SciFact (real) | 3,688 | per-row | 943 | 3,425 | 74.2% |
| generated-correlated | 5,000 | per-row | 4,657 | 4,833 | 6.7% |
| generated-correlated | 50,000 | per-row | 41,321 | 44,552 | 17.3% |

Pre-registered pass bar: median survivors <= 500 **and** p90 <= 1,000, zero
soundness failures. Fail bar: median above 2,000.

- Block certificates on the real corpus: **median 2,530 survivors — fail.**
  For more than 10% of queries the bound prunes *nothing at all*.
- Per-row certificates, the main E3 tightening lever, measured rather than
  assumed: median 943 survivors, which lands in the "partial" band, but p90 is
  3,425 of 3,688 rows. The tail is the whole corpus.
- Either way, a certified search would refine hundreds to thousands of rows
  where the current heuristic refines 64.

**Soundness held everywhere: 0 failures across every run.** The bounds are
correct. They are simply too wide to be useful.

## The scaling result is what settles it

Doubling down on smoke-scale numbers would be unfair to the idea, so the same
measurement was run on a synthetic corpus at two sizes:

- 5,000 rows -> 4,657 survivors
- 50,000 rows -> 41,321 survivors

Ten times the rows produced 8.9 times the survivors. The survivor set grows
**linearly with corpus size**. At the 10M-row local target this projects to
millions of rows surviving per query. A pruning rule whose output scales with
N is not a pruning rule.

## Why the bound is wide (E0 attribution)

`epsilon = transform_dot + query_norm_upper * max_reconstruction_l2_error + serving`

On the real corpus, primary certificate:

| Term | Value | Share |
|---|---|---|
| transform dot | 2.29e-5 | 0.011% |
| reconstruction | 2.145e-1 | **99.950%** |
| serving (fixed point) | 8.39e-5 | 0.039% |

The transform and fixed-point terms are noise. **Epsilon is the reconstruction
error and nothing else.** Q24 fixed point and the Walsh-Hadamard precision
drift, the two things the design treats carefully, contribute one part in ten
thousand.

## There is no tightening headroom (E1)

Certified epsilon is **3.8x** the largest error actually observed over 200
queries x 3,688 rows (0.2146 against 0.0562). On synthetic data it is 5.2x.

This corrects a belief recorded in the project guide caveat 3 and repeated on
2026-08-07: that `maximum_normalized_primary_slack` of 0.99999990 showed
roughly seven orders of magnitude of unused error budget.

That reads the statistic backwards. `normalized_slack` is 1.0 when a trial had
zero error and 0.0 when it consumed the whole budget, and the soak records the
**maximum** over 2,000,000 trials. A maximum near 1.0 only says that at least
one trial out of two million happened to have almost no error, which is
expected and says nothing about tightness. The statistic that measures
tightness is the **minimum** slack. This experiment supplies it: the worst
observed trial consumed about 26% of the budget (slack ~0.74), not one ten
millionth of it. The bound is within a
small constant factor of the true worst case, which means:

- Tightening cannot rescue this. Even a perfect bound — one exactly equal to
  the worst observed error — is only ~4x narrower, and the survivor counts
  above would still be in the thousands.
- The problem is not the proof. The problem is the codec: 4-bit primary
  quantization loses about 0.21 of dot product, while the gaps between the
  10th and 11th neighbour in a real corpus are far smaller than that. The
  bound is honestly reporting how much information the primary code threw away.

Per-row epsilons are narrowly spread (p50 0.156, max 0.215), which is why
per-row certificates buy a constant factor and not an order of magnitude: every
row reconstructs about as badly as every other row. That is the transform
working as designed — it makes all coordinates look alike.

## What this does not claim

- It does not say the certificates are wrong. They are sound in every run.
- It does not say certified bounds are worthless in general. It says these
  bounds, over this codec, cannot replace a candidate budget.
- A bound computed against the *refined* reconstruction (epsilon 0.107, half as
  wide) was not used for pruning, because refinement is the work pruning is
  meant to avoid.

## Corpus provenance problem found on the way

`corpora/scifact/scifact-mpnet-768.f32` was missing and was rebuilt from the
pinned dataset and model revisions. The rebuilt file has the same row count and
byte length but a **different BLAKE3** than the descriptor recorded in August
(`b2e549ce...` against `8a20ab21...`).

Same pins, same machine, different bytes. The embedding step is not bit
reproducible across library versions, so the hash pin does not do what the
descriptor implies. This does not affect the result above — the differences are
float noise — but any future claim of a reproducible corpus needs the `.f32`
file itself archived, not just its revisions.

## Reproducing

```
cargo build --release -p spherra-bench
./target/release/spherra-bench prune-rate \
  --corpus corpora/scifact/scifact-mpnet-768.json \
  --queries 200 --seed 7 --k 10 \
  --output docs/experiments/results/2026-09-12-prune-rate-scifact-k10.json
```

Runs in about 4 seconds. The scale check substitutes
`--corpus generated-correlated-768x50000 --queries 50`.

## Note on the rule as applied

The harness records p50/p90/p99/max, so the pre-registered "p95 survivors" bar
was read as p90. This was settled while writing the measurement code, before
any result existed, and it makes the bar slightly more lenient, not less. It
changes nothing: the run failed on the median, and p90 was the entire corpus.
