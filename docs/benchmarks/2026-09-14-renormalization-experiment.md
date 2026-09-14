# Reconstruction renormalization experiment — 2026-09-14

The user asked to test "option 1": divide each refined candidate score by the
length of its reconstruction, `dot(T(q), p + e) / |p + e|` (times the stored
length for dot-product search). It needs no stored bytes and only touches the
refined candidate pool. This is a **benchmark-only** experiment at clean
`fcaf0a8`; serving search, scores, intervals, defaults and formats are unchanged.
Adopting it would require a user-approved design change, including new
certificate intervals for the corrected score.

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

Raw evidence: [generated 1M](results/2026-09-14-renorm-generated-1m.json),
[MS MARCO test](results/2026-09-14-renorm-msmarco-test.json),
[DPR 1M](results/2026-09-14-renorm-dpr-1m.json). Candidate traces remain local
under `target/measure/`.
