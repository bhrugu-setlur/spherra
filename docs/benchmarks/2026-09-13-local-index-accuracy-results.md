# Local index reconstruction accuracy investigation

## Findings

This experiment measures where exact vector neighbors are lost, whether more
calibration data improves compressed ranking, and the cost of reading original
vectors for final ranking. Production scoring, codec bytes, public API and
candidate budget 200 remain unchanged.

The generated 1M test contains all 2,000 exact top10 neighbors in the primary
budget-200 pool. All 181 baseline misses occur in compressed ranking. Increasing
the pool to 400/800/1600 recovers none. Replacing Q24 lookup scores with FP64
scores over the same reconstructed values leaves baseline top10 ordering
unchanged. This isolates the material problem to reconstruction/transform
accuracy, rather than selection or comparison rounding.

| Training inputs | Used for training | Validation | Generated 1M recall@10 | Selection losses | Ranking losses |
|---:|---:|---:|---:|---:|---:|
| 4,096 | 3,072 | 1,024 | 0.9095 | 0 | 181 |
| 8,192 | 6,144 | 2,048 | 0.9135 | 0 | 173 |
| 16,384 | 12,288 | 4,096 | 0.9190 | 0 | 162 |
| 32,768 | 28,672 | 4,096 | 0.9145 | 0 | 171 |

The 16,384-input difference from baseline is +0.95 percentage points, with a
paired-query bootstrap 95% interval of −0.15 to +2.05 points. The other increases
also have intervals spanning zero. This is previously exposed regression data,
not independent model-selection evidence. Increasing inputs retrains both the
primary quantizer and residual codebooks and changes validation membership; it
does not isolate a PQ-only training improvement. With 16,384 inputs, FP64 and
Q24 differ at two top10 positions but have the same aggregate neighbor recall.

## Real-query dataset and selection

The hash-pinned snapshot contains 100,000 MS MARCO passages, 32,768 separate
calibration passages, 200 tuning queries and 200 final queries. All are embedded
with pinned `all-mpnet-base-v2`, dimension 768, normalized FP32. The dataset,
qrels, model revisions, dependencies, device, vector/text hashes and row counts
are in the committed [descriptor](../../corpora/msmarco/msmarco-real-queries-100k-mpnet-768.json).

The indexed sample preserves all 435 labeled relevant passage IDs for the
selected queries and adds deterministically sampled distractors. This makes it
a constructed subset, not an official full-corpus MS MARCO benchmark. Passage
texts are deduplicated across indexed/calibration sets after case folding and
whitespace normalization. Tuning/final query texts are disjoint; actual query
texts also do not exactly duplicate passage texts. Near-duplicates and semantic
relations can remain. One indexed passage exceeded the model's 384-token limit;
no calibration passage or query was truncated. No new embedding model was
trained; the experiment improves representativeness of the measured vectors.

Exact original-space FP64 top100 references were pinned on clean `2f8fcb8`,
before any real index measurement. Training sizes and selection were
[preregistered](2026-09-13-local-index-accuracy-experiments.md): choose the
smallest input size within 0.002 recall@10 of the best tuning result at budget
200; then compare baseline and chosen model on the final queries.

| Training inputs | Tuning recall@10 | Selection losses | Ranking losses |
|---:|---:|---:|---:|
| 4,096 | 0.9715 | 0 | 57 |
| 8,192 | 0.9680 | 0 | 64 |
| 16,384 | 0.9700 | 0 | 60 |
| 32,768 | 0.9725 | 0 | 55 |

The rule selected **4,096 inputs**, within 0.0010 of the best. The largest
model's paired-query bootstrap 95% difference interval is −0.65 to +0.80
percentage points. The selection was committed at `372c362` before the final
run. Since the selected model is the baseline, only one final model was run.

On the **200 untouched final queries**, recall@10 is **0.9705** (1,941/2,000
neighbors), with a query-bootstrap 95% interval of **0.9635–0.9770**. All 2,000
true neighbors occur in the budget-200 pool; all 59 misses are ranking losses.
Original-vector reranking of that same pool recovers all 2,000. Budgets
400/800/1600 recover no additional neighbors. FP64 reconstructed ranking has
the same recall, with two positional differences from Q24 at each budget.

Sparse-label results provide a different perspective:

| Final-query method | MRR@10 | Queries with a labeled relevant passage in top10 |
|---|---:|---:|
| Compressed baseline | 0.8451 | 0.9600 (192/200) |
| Exhaustive exact vector ranking | 0.8420 | 0.9550 (191/200) |

MRR averages the reciprocal rank of the first labeled relevant passage, or zero
if none is returned. These small differences are not evidence that compression
improves semantic relevance. Labels are sparse, the collection was constructed
to retain positives, and the embedding's cosine ordering is not relevance
truth. Exact original reranking fixes neighbor fidelity; it does not guarantee
better user relevance. A stronger embedding or text reranker must be evaluated
against relevance labels separately.

For the baseline, the median exact score gap between ranks 10 and 11 is
0.00103 on generated 1M and 0.00390 on final real queries. Median absolute
reconstructed score error for exact top10 members is 0.00446 and 0.00645,
respectively. Maximum Q24-versus-FP64-reconstruction error within the
budget-200 pool is only 0.00000225 and 0.00000195. This illustrates why score
error alone is insufficient: ordering also depends on neighbor gaps and on
how errors vary between competitors.

## Original-vector reranking prototype

The benchmark calls public search with k=B=200, sorts candidate IDs into file
order, reads their original 768-dimensional FP32 rows positionally, normalizes
in FP64 and reranks by exact cosine score. Existing PQ work is included, making
this a straightforward prototype rather than an optimized serving integration.
It does not change returned production scores or certificates.

On clean `ab2c369`, 1,000 generated 1M queries plus 50 warmups per path,
budget 200 and AC power produced:

| Path | p50 | p99 |
|---|---:|---:|
| Compressed public search | 66.125 ms | 181.195 ms |
| Public finalists + original FP32 reranking | 69.081 ms | 209.124 ms |

Median per-query added time was 3.223 ms; mean added time was 3.111 ms. Paths
alternated order per query. No corpus embedding, training or test workload ran
concurrently. The original file had been hashed completely before warmups;
this is a warm-cache comparison. Compare these two paths within this run,
not against earlier gate timings. The diagnostic's 200 exact-reference queries
show 100% attainable recall for original reranking; the 1,000-query timing run
is not a claim of 100% measured recall on all 1,000 queries.

The raw process peak RSS is 6,544,146,432 bytes and includes original generation
and file validation. It is not an open/search-only memory measurement and does
not supersede the delivered memory gate.

Original storage costs 3,072 bytes/vector: 3.072 GB for 1M or 30.72 GB for 10M,
in addition to existing compressed files. Only the 1M file was created. At
budget 200, each query requests 614,400 original payload bytes; actual page I/O
can be larger. Originals are file data, not required resident scan memory, but
page-cache and storage costs remain. This experiment does not qualify cold SSD
latency or change the delivered memory/SLO claims.

## Verification and evidence

Each diagnostic checks public top-B and top10 hits against an independent
checked-scalar scan/full-sort ranking. Every selected score matches exactly;
every checked public interval encloses original-space FP64 truth. The report
stores every exact neighbor's primary/refined rank and error, returned hits,
loss counts, and hash-bound complete largest-pool traces. Exact reranking of a
pool must reproduce its measured true-neighbor coverage.

All diagnostic runs use clean release revisions, fixed original/query bytes,
seed 20260804 and public index builders. Diagnostic elapsed times are not
serving latency measurements. Raw reports retain source/build/model/codec,
CURRENT and trace identities. Raw FP32/text files, oracle binaries and full
candidate traces remain local, hash-verified artifacts; JSON reports are
committed. CI passed 197 tests with 12 explicit large-test skips, workspace
tests and strict all-target clippy passed, and three Python sampler tests passed.
Focused tests cover bad provenance, original-file truncation, exact reranking,
loss accounting and public score/enclosure agreement. No new fuzz or ASan
runtime claim is made.

- Generated 1M diagnostics (clean `f3453e1`): [4096](results/2026-09-13-loss-generated-1m-training4096.json), [8192](results/2026-09-13-loss-generated-1m-training8192.json), [16384](results/2026-09-13-loss-generated-1m-training16384.json), [32768](results/2026-09-13-loss-generated-1m-training32768.json).
- Real tuning diagnostics (clean `ab2c369`): [4096](results/2026-09-13-loss-msmarco-tuning-training4096.json), [8192](results/2026-09-13-loss-msmarco-tuning-training8192.json), [16384](results/2026-09-13-loss-msmarco-tuning-training16384.json), [32768](results/2026-09-13-loss-msmarco-tuning-training32768.json).
- [Final-query diagnostic](results/2026-09-13-loss-msmarco-test-training4096.json) (clean `372c362`); pinned [tuning oracle](results/2026-09-13-msmarco-tuning-oracle.json) and [final oracle](results/2026-09-13-msmarco-test-oracle.json).
- Original reranking [raw timing samples](results/2026-09-13-original-rerank-1m.json) and [process timing output](results/2026-09-13-original-rerank-1m.time.txt).
- Reproduction: [data builder](../../tools/build_msmarco_accuracy_corpus.py), [dataset audit](../../tools/audit_accuracy_dataset.py), [candidate-trace audit](../../tools/audit_index_loss.py), and [query-bootstrap/relevance summaries](../../tools/summarize_index_accuracy.py).

All nine diagnostic traces passed the separate Python audit: 1,800 model/query
cases, 2,880,000 largest-pool candidate records. These include repeated queries
across models; they are not 1,800 independent queries. The bootstrap resamples
200 whole queries, not individual neighbor hits, using 10,000 resamples and
seed 20260804. Its percentile interval is descriptive, conditional on this
fixed corpus/model/query sampling, and not adjusted for repeated comparisons.

## Decision after the investigation

The user declined the original-vector storage/read tradeoff and chose to move
on from this investigation. Keep compressed-only serving and budget 200. The
prototype and measurements below remain evidence; original-vector production
integration is not planned. The completed local-index plan defines no further
checkpoint, so the next milestone needs to be specified separately.

## Interpretation and possible future changes

Keep budget 200 and the current training setup: neither a larger pool nor more
training inputs demonstrated a reliable gain. Do not change Q24 precision to
address this loss; its observed effect is negligible compared with reconstruction.

The clearest measured route to higher neighbor fidelity is optional reranking
from originals. A production proposal would need an explicit external-original
provider bound to the same index generation and dense row IDs, missing/corrupt
row failure behavior, a distinct exact-score result contract, and cold-cache
latency qualification before adoption. It requires a user-approved API/design
amendment; this task implements only the benchmark prototype.

For a solution retaining today's compressed storage, the next experiment would
optimize relative score errors for real query/candidate pairs, using a new,
separate training-query split and validating on additional held-out queries.
Compare against the current residual trainer at the same 96-byte budget and
measure rank swaps, neighbor recall and label metrics—not only reconstruction
MSE. This investigation does not establish that a different trainer, extra
bits, changed transform or FP16 originals would meet the same quality/cost goals.
The final-query set above is now exposed; use a new held-out set for subsequent
model selection and confirmation.
