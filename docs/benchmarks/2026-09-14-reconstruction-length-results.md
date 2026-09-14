# Reconstruction-length correction: production test data — 2026-09-14

Option 1 is now the default finalist correction in both cosine and dot-product
search. It divides the refined Q24 numerator by the FP64 length of the decoded
compressed vector. Primary selection, budget 200, stored bytes, residual reads
and original-truth certificates are unchanged. Existing indexes need no rebuild.
[Approved contract](../design/2026-09-14-reconstruction-length-amendment.md).

## Verification

The production change is clean commit `a407fff`. The benchmark-only reference
reuse follow-up is `79e8a39`; it changes no library code. Full CI passes 218 tests
with 12 explicitly skipped qualifications; workspace tests and strict all-target
clippy pass. The ignored release scalar-reference qualification also passes on
archived SciFact and generated 20k, each with 200 queries and four k/budget
combinations. Tests check changed ranking, raw numerator equality, exact public
corrected-score equality, unchanged same-row bounds, negatives, zero and extreme
reconstructions, zero-magnitude ties, query scaling and multiple workers/segments.

Quality/loss reports are version 2: public scores are corrected FP64 values, not
Q24 integers recoverable by multiplying by 2^24. Raw Q24 reference numerators
remain explicit. `refined_error` in loss reports still describes the raw Q24
numerator error; delivery and rank-loss fields describe corrected serving.
FP64-only/stored-alignment experiments remain separate fields. Historical
version-1 report bytes and their interpretation are preserved.

## Retrieval accuracy

All runs use k=10, candidate budget 200, seed 20260804 and 4096 training inputs.

| Workload | Queries | Previous serving | Corrected serving | Additional true neighbors |
|---|---:|---:|---:|---:|
| Generated correlated 1M, cosine | 200 | 0.9095 | 0.9255 | 32 / 2,000 |
| MS MARCO 100k, real final queries, cosine | 200 | 0.9705 | 0.9770 | 13 / 2,000 |
| DPR 1M native 768D, dot | 1000 | 0.9024 | 0.9234 | 210 / 10,000 |

The two cosine runs use clean `a407fff` and reuse their previous indexes. Corpus,
query, model, oracle and CURRENT identities match the original experiment.
Independent audits compare all 80,000 traced candidates: primary ranks, raw
primary/refined scores, FP64 numerators and truth are unchanged. Both pools
still cover all exact top-10 answers on these queries. The corrected public
scores/order match the scalar reference exactly, and every checked candidate
interval encloses original truth. Common returned hits retain bit-identical
intervals: 1,893 generated hits and 1,964 MS MARCO hits.

For both cosine workloads, the Q24 numerator gives the same recall as the earlier
FP64-numerator experiment.
Generated top-10 orders are identical between those two corrected variants. One
MS MARCO query changes order within the same top-10 set. This is the expected
small numerical difference; the production numerator and raw certificate
provenance were deliberately retained.

DPR uses clean `79e8a39`, the existing `target/dpr-dot-index-1m-renorm` index,
and the pinned exact lists from the earlier clean `7043707` experiment. The
benchmark verifies the report hash, source cleanliness, all input hashes,
workload, CURRENT and exact-list cardinality/uniqueness before reuse. It searches
every query afresh, compares all corrected candidate scores/order with scalar
reconstruction, and checks original-dot truth for returned hits. This is not a
new independent computation of the exact oracle. The initial attempt to repeat
that expensive exhaustive oracle was stopped; no partial run is counted.

Reference report BLAKE3:
`7e0ffc15a62cdac75c65cf2a44b3d279fc794da174f0ef9e80b087e8b28fca2b`.
DPR CURRENT:
`1c5f11865e61ecfbb0aed53c286eea0acf47fa4cbbfe9621668f927eaadd060b`.

DPR corrected recall is 0.9234, versus 0.9235 with the FP64 numerator: one
fewer recovered neighbor out of 10,000. Both variants use the same pool; the
production Q24 numerator is retained as specified, without tuning to recover
that one evaluation hit.
All 10,000 recorded original-dot intervals enclose truth. The independent audit
checks exact-answer lists, recalculates hit counts and verifies bit-identical
bounds on 9,214 common hits. Together, the three reports record
14,000 final hits with zero enclosure failures; cosine diagnostics additionally
check the full 80,000-candidate pools.

## Latency

Serial clean-release runs on the existing generated 1M index, six
workers, 50 warmups and 1000 measured queries per method. Historical shared-scan
baselines at `e093e25` are cosine p50/p99 57.984/144.235 ms and dot
58.658/174.376 ms. These are separate runs, not a paired measurement of the
correction's isolated cost.

| Method, 1M rows | p50 | p99 | Peak open/search RSS | Descriptors | Gate |
|---|---:|---:|---:|---:|---|
| Cosine | 72.313 ms | 113.564 ms | 401,391,616 B | 17 | pass |
| Dot product | 73.009 ms | 108.237 ms | 401,014,784 B | 17 | pass |

Both runs use clean `79e8a39`, unchanged CURRENT/model/query identities, AC power
and the existing safe tile kernel. The 1M p50≤150 ms / p99≤300 ms targets pass.
The medians are about 14 ms higher than the earlier separate runs. These
measurements do not isolate the correction's cost; the proposed sub-millisecond
overhead remains unverified.

## Raw evidence and reproduction

- [Generated 1M cosine JSON](results/2026-09-14-serving-renorm-generated-1m.json), [raw process log](results/2026-09-14-serving-renorm-generated-1m.time.txt).
- [MS MARCO final cosine JSON](results/2026-09-14-serving-renorm-msmarco-test.json), [raw process log](results/2026-09-14-serving-renorm-msmarco-test.time.txt).
- [DPR 1M dot JSON](results/2026-09-14-serving-renorm-dpr-1m.json), [raw process log](results/2026-09-14-serving-renorm-dpr-1m.time.txt).
- [Cosine latency JSON](results/2026-09-14-serving-renorm-latency-cosine-1m.json), [raw process log](results/2026-09-14-serving-renorm-latency-cosine-1m.time.txt).
- [Dot latency JSON](results/2026-09-14-serving-renorm-latency-dot-1m.json), [raw process log](results/2026-09-14-serving-renorm-latency-dot-1m.time.txt).

Each JSON preserves its exact command, hashes and machine/source
provenance; initial outputs and complete candidate traces stay under ignored
`target/measure/`. Run the tracked `tools/audit_index_loss.py` with the project
Python environment over both cosine reports to independently verify counts,
rankings, score formulas, bounds and trace hashes. Historical reports pass the
same audit.

## Limits

The correction removes length bias; it does not repair directional compression
error or candidates missed by the primary scan. Full candidate coverage is
observed here, not guaranteed. These are reused evaluation sets, not fresh
held-out model-selection evidence. No new 10M recall or full 10M latency gate
is claimed. The certified interval still encloses original truth and need not
contain the displayed estimate. There is no added per-row disk or resident data;
small per-query decode arrays and corrected keys add work proportional to the
candidate budget.
