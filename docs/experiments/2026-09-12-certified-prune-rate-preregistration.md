# Pre-registration: certified prune-rate experiment

**Date written:** 2026-09-12
**Status:** written before any measurement was run.
**Decides:** whether Spherra's certified error bounds can skip real search work,
and therefore whether the project has a differentiated core worth building an
engine around.

This file exists because the 2026-08-07 session decided the decision rule must
be fixed *before* the numbers arrive, to stop a weak result from being talked
into a good one. Do not edit the thresholds in this file after measurement
begins. Record the outcome in a separate results file.

## Background

Spherra scores a query against every row from a 384-byte primary code, then
refines a fixed budget of top candidates from their 96-byte residual codes.
Today that budget is a guess: "take the top 64 by primary score." Nothing
proves the true top-10 is inside it.

Every score also carries a certificate: a proven interval `[score - e, score + e]`
that contains the true full-precision similarity. If those intervals are tight,
they replace the guess with a proof — refine exactly the rows that cannot be
ruled out, and know nothing was missed. If they are loose, they prove nothing
useful and the project has no unique core.

## What is measured

Corpus: BEIR SciFact, `all-mpnet-base-v2`, 768-d, 5,183 rows, pinned by dataset
and model revision and by BLAKE3 hash. Split into disjoint indexed /
calibration / query sets by the existing harness. `k = 10`.

For each query, over all N indexed rows:

1. Compute the certified primary score and its bounds for every row. This is
   the scan the engine already performs; bounds cost two additions per row on
   top of it.
2. Let `tau` be the k-th largest **lower** bound across all rows.
3. A row is **pruned** when its **upper** bound is below `tau`. Such a row is
   provably not in the true top-k.
4. **survivors** = N - pruned. These are the rows that would have to be refined
   to return a certified top-k.

Reported per query and aggregated: prune rate, survivor count (median, p95,
max), and whether the true top-10 is contained in the survivor set (a soundness
check; a survivor set that misses a true neighbour means the bound is wrong,
not merely loose).

Also reported, from the existing certificate accessors:

- **E0 attribution.** `epsilon = eta_transform_dot + query_norm_upper *
  max_reconstruction_l2_error + eta_serving_score`. Report each of the three
  terms and its share, for the primary and refined certificates.
- **E1 tightness.** The largest observed `|certified score - true FP64 score|`
  against `epsilon`. The ratio is the headroom available to any future
  tightening.

## Decision rule

The 2026-08-07 note expected a 5-10% prune rate as break-even. **That bar is
mis-specified and is not used.** Pruning 10% of a 10M-row corpus still leaves
9M rows to refine, against a heuristic budget of 64 — far worse than the guess
it replaces. The number that matters is how many rows survive, not what
fraction dies.

Costs, per query: the primary scan happens either way; bound evaluation is
negligible; refinement costs one residual read plus 96 lookups per survivor.
So certified pruning is worth having when the survivor set is within the same
order as the heuristic budget it replaces.

**Pass:** median survivors at k=10 <= 500 rows, and p95 survivors <= 1,000,
with zero soundness failures.
(On N = 5,183 that is a prune rate of about 90.4% and 80.7%.)

**Partial:** median survivors between 500 and 2,000. The bound prunes
measurably but not enough to beat the heuristic today. Proceed only to the E3
tightening levers (per-row certificates instead of block-max, radius-aware
bounds, concentration argument), then re-measure once against this same rule.
One tightening pass, not an open-ended campaign.

**Fail:** median survivors above 2,000 (prune rate below about 61%), or any
soundness failure. The bounds do not carry load. Publish the codec and the
write-up, close the database direction, and stop.

## Known limits of this evidence

- 5,183 rows is smoke scale. A prune rate here does not establish one at 10M.
  What it can do is kill the idea cheaply, which is the point of the week.
- The certificate is currently computed per block using the **maximum**
  reconstruction error over the rows in the block, so a single bad row widens
  every interval in it. With the whole corpus as one block, this is the
  worst case for the bound. Per-row certificates are an E3 lever, not a
  defect to fix before measuring.
- Survivor counts depend on the score distribution of the corpus, which is
  model- and dataset-specific. A pass here justifies one larger real-corpus
  run, not a format freeze.
