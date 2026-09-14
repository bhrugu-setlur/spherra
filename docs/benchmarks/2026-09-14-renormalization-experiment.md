# Reconstruction renormalization experiment — 2026-09-14

This experiment tests "option 1": divide each refined candidate score by the
length of its reconstruction, `dot(T(q), p + e) / |p + e|` (times the stored
length for dot-product search). It needs no stored bytes and only touches the
refined candidate pool. This is a **benchmark-only** experiment at clean
`7043707`; serving search, scores, intervals, defaults and formats are unchanged.
Serving adoption is now approved in the
[reconstruction-length amendment](../design/2026-09-14-reconstruction-length-amendment.md).
The earlier expectation that intervals must change is corrected there: retain
the authenticated raw-score bounds on original truth, rather than rescaling
them to the displayed estimate. Results below remain the original experiment.

`index-diagnose` and `dot-product` rescore the same budget-200 candidate pool
in FP64. Earlier evidence showed FP64 and Q24 ranking of the unrenormalized
score have equal recall on these workloads (`floating_recall_at_10` below).

| Workload (budget 200, k10) | Queries | Baseline recall@10 | Renormalized | Change | Paired query bootstrap 95% |
|---|---:|---:|---:|---:|---|
| Generated 1M, cosine | 200 | 0.9095 (FP64 0.9095) | **0.9255** | +1.60 pts (44 better / 12 worse queries) | +0.85 to +2.35 pts |
| MS MARCO 100k real test queries, cosine | 200 | 0.9705 (FP64 0.9705) | **0.9770** | +0.65 pts (21 better / 8 worse) | +0.15 to +1.20 pts |
| DPR 1M native dot product | 1000 | 0.9024 | **0.9235** | +2.11 pts (211 neighbors) | not computed; report stores totals only |
| MovieLens 1170 factors, dot (sanity) | 200 | 0.9765 | 0.9790 | +0.25 pts | not computed |

All budget-200 pools contained every true neighbor for the cosine workloads, so
gains come from final ordering. Bootstrap: 10,000 resamples of per-query hit
differences, seed 20260914.

Interpretation: dividing by reconstruction length consistently recovers part of
the compressed ranking loss (18–22% of misses: 32/181, 13/59 and 211/976) at zero storage cost and
about 200 decoded candidates per query. It does not approach original-vector
reranking, which recovered all neighbors. Not yet measured: serving latency
cost, a certified interval for the corrected score, 10M rows, and a paired
interval on DPR.

## Option 2: stored build-time alignment (clean `c0b11e3`)

The builder knows each row's original direction u, so it could store the
alignment `a = dot(u, p + e)` and serving could divide the refined score by it
(the RaBitQ-style correction). Simulated three ways on the same pools: exact
FP64 `a`; `a` rounded to FP16 (2 bytes per row, fits the spare flag bytes); and
|p + e| times a one-byte code of `a / |p + e|` over [0.5, 1] (1 byte).

| Workload | Baseline | Option 1 (no bytes) | Option 2 exact | Option 2 FP16 | Option 2 one byte |
|---|---:|---:|---:|---:|---:|
| Generated 1M, cosine | 0.9095 | 0.9255 | 0.9260 | 0.9260 | 0.9255 |
| MS MARCO test, cosine | 0.9705 | 0.9770 | 0.9775 | 0.9780 | 0.9775 |
| DPR 1M, dot | 0.9024 | 0.9235 | 0.9230 | 0.9234 | 0.9231 |

Paired against option 1 (per-query, 10,000 bootstrap resamples), every option 2
variant is within noise: generated 1M +0 to +1 neighbors (95% intervals span
zero), MS MARCO +1 to +2 neighbors (0.00 to +0.25 points at best), and DPR −5
to −1 neighbors of 10,000.

Why: the smallest alignment ratio `a / |p + e|` among all candidates was
0.9943 (generated), 0.9944 (MS MARCO) and 0.9956 (DPR). Reconstructions point
almost exactly along their originals, so dividing by `a` is nearly the same as
dividing by |p + e|. The remaining error is noise perpendicular to the original,
which one stored number per row cannot remove. Option 2 adds a format change
and rebuild for no measurable gain; option 1 captures the available benefit.

Option 2 evidence: [generated 1M](results/2026-09-14-opt2-generated-1m.json),
[MS MARCO test](results/2026-09-14-opt2-msmarco-test.json),
[DPR 1M](results/2026-09-14-opt2-dpr-1m.json).

Option 1 evidence: [generated 1M](results/2026-09-14-renorm-generated-1m.json),
[MS MARCO test](results/2026-09-14-renorm-msmarco-test.json),
[DPR 1M](results/2026-09-14-renorm-dpr-1m.json). Candidate traces remain local
under `target/measure/`.
