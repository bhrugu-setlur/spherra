# Local index accuracy experiments

Status: completed; authorized after the local-index delivery.
Results: [accuracy technical note](2026-09-13-local-index-accuracy-results.md).

## Questions and order

1. Separate candidate-selection loss from compressed reranking loss on the
   existing pinned generated 1M corpus at budgets 200/400/800/1600. Preserve
   exact row ranks, score errors, original-vector reranking, floating-point
   reconstructed ranking and public/scalar equality evidence per query.
2. Build a pinned real-document **and real-query** corpus with MPNet-768;
   start at 100k indexed passages and retain separate calibration, tuning-query
   and final-query sets. Existing held-out-document SciFact remains a regression.
3. Compare training input sizes 4096/8192/16384/32768, record actual validation
   and training counts, keep indexed bytes/query bytes/transform seed fixed,
   and choose using tuning queries before opening the final query results.
4. Use measured loss attribution to decide whether a benchmark-only training
   improvement or original-vector reranking is warranted. Production format,
   public API, score/certificate contracts and default budget stay fixed during
   diagnosis; any proposed production change needs a concrete design amendment.

## Measurement rules

- Original-space FP64 exhaustive top10 defines vector-neighbor truth. The
  existing 1M top100 artifact was pinned before delivery; it is regression
  evidence, not an untouched final test set for model selection.
- Candidate recall is exact-top10 membership in the primary top-B pool. Its
  complement is selection loss. Candidate recall minus delivered recall is
  ranking loss. Exact reranking of the same pool must recover its membership
  count exactly, including deterministic row-ID tie breaking.
- Compare public k=B/B=B finalists and public k=10/B=B hits against an
  independent checked-scalar ranking. Reconstructed FP64 scoring uses the same
  prepared transformed FP32 query and restored primary/residual values, without
  renormalization, to isolate Q24 rounding from reconstruction/transform error.
- Every excluded exact neighbor records primary rank; pool members additionally
  record refined rank, exact score and score errors. Record returned competitors
  and complete candidate rows so summaries can be independently reconstructed.
- No diagnostic timing is a latency gate. Performance comparisons run separately
  with the existing public latency protocol after model selection.
- Pin text/data/model revisions, preprocessing, FP32 bytes and hashes. Real
  queries are distinct from corpus/calibration rows. Keep held-out query sets
  fixed and do not train/select on final relevance or exact-neighbor results.
- The benchmark must fail on wrong provenance, score/rank disagreement, false
  enclosure or inconsistent loss accounting. Use clean release commits for
  recorded runs and archive evidence after they complete.

## Preregistered real-query selection

At budget 200, choose the smallest training input size whose tuning recall@10
is within 0.002 of the best of 4096/8192/16384/32768. Record that choice before
running final queries. Compare only the baseline and selected model on the
200 final queries; if baseline wins selection, run it once. Report paired
query-bootstrap intervals (10,000 resamples, seed 20260804). Budget sweeps
remain diagnostic and do not change the default. Relevance labels supplement
exact-neighbor recall; this constructed subset is not an official benchmark.

## Progress

- Preregistered the first diagnostic and data/training experiment sequence.
- `index-diagnose` implements the generated-corpus attribution path and a strict
  report schema. A compact hash-bound JSONL trace retains each largest-pool
  candidate once; the main report retains per-budget top10 and true-neighbor
  records. Traces are local artifacts, like the existing exact oracle binary.
- Focused tests cover full/partial candidate coverage, loss accounting, all-hit
  equality/enclosure, trace identities, schema requirements and bad inputs.
- Focused tests and the full CI gate passed (194 tests, 12 skipped); workspace
  tests and strict all-target clippy passed. Clean 1M measurements completed at `f3453e1`: budget 200 covers every exact
  top10 neighbor; delivered recall for the four training sizes is respectively
  0.9095/0.9135/0.9190/0.9145. Every loss is compressed ranking loss; all
  four complete candidate traces passed an independent audit.
- Real-data preparation pins `BeIR/msmarco@a918e0d1`,
  `BeIR/msmarco-qrels@253fbf8a`, and MPNet `e8c3b32e`. The planned 100k-passage
  workload retains relevant passages for 400 deterministically chosen dev
  queries (200 tuning, 200 final), plus sampled distractors, and 32,768 disjoint
  calibration passages. This is an explicitly constructed subset, not an
  official full-corpus MS MARCO score. Passage truncation at the model's 384
  word-piece limit is counted and recorded; raw texts and normalized FP32
  embeddings are retained locally. Case/whitespace-normalized duplicate passage
  texts are excluded between calibration and indexed documents, and repeated
  query texts between tuning and final queries. Near-duplicates remain possible.
- `dataset-oracle` pins real-query exact references; `index-diagnose --dataset`
  verifies vector/text hashes, query split and reusable model provenance.
- `original-rerank` is a benchmark-only prototype: public k=B search followed
  by positional FP32 original reads and exact FP64 reranking. Its warm-cache
  comparison includes existing PQ work, so it is not an optimized new API.

```bash
cargo run -p spherra-bench --release --locked -- index-diagnose \
  --rows 1000000 --queries 200 --seed 20260804 --training-rows 4096 \
  --budgets 200,400,800,1600 --reuse true --index-dir target/local-index-1m \
  --oracle-reference docs/benchmarks/results/2026-09-13-local-index-oracle-generated-correlated-1m.json \
  --output target/measure/loss-1m-training4096.json
```

The real-data snapshot is pinned in
[`corpora/msmarco/msmarco-real-queries-100k-mpnet-768.json`](../../corpora/msmarco/msmarco-real-queries-100k-mpnet-768.json).
All vector/text hashes, counts, relevance endpoints and split separation passed
`tools/audit_accuracy_dataset.py`; actual query texts also have no normalized
exact duplicates in either indexed or calibration passages. The executed data
builder and tracked builder have identical executable Python AST (formatting
and module description aside). Embedding bytes, device and dependencies are
pinned; identical floating-point bytes on other hardware are not assumed.

Real-query/prototype verification passed 197 CI tests with 12 explicitly
skipped large qualifications, workspace tests, strict all-target clippy, and
three Python sampling tests. New tests reject wrong query oracles and corrupted
vector hashes, validate reusable training identity, compare original reranking
with exhaustive truth and reject truncated original files.

Both real-query exact top100 references are now pinned under
`docs/benchmarks/results/2026-09-13-msmarco-{tuning,test}-oracle.json`,
generated on clean `2f8fcb8` before any real index measurement. No final-query
model result has been run or inspected.

## Selection recorded before final-query measurement

All tuning measurements ran on clean `ab2c369`. Recall@10 at budget 200 for
4096/8192/16384/32768 inputs is 0.9715/0.9680/0.9700/0.9725. Candidate
coverage is 1.0000 in all cases; larger budgets recover no additional neighbors.
The preregistered rule selects **4096 inputs**: it is the smallest size within
0.002 of the best (difference 0.0010). The largest model's paired-query
bootstrap 95% difference interval is −0.0065 to +0.0080. All four complete
candidate traces passed the independent audit. The final comparison therefore
requires one run, since the selected model is the baseline. No final-query
model result has been generated or inspected as of this decision.

The separate original-rerank prototype also completed on clean `ab2c369`,
with no concurrent training/embedding/test load: 1,000 queries, 50 warmups,
AC power, normal p50/p99 66.125/181.195 ms, original p50/p99
69.081/209.124 ms. Raw samples and `/usr/bin/time -l` output are archived.
This warm-cache prototype is not a new production API or SLO qualification.

## Final outcome

The selected baseline ran on the 200 final queries at clean `372c362`, reaching
0.9705 recall@10 with 1.0000 candidate coverage and 59 compressed ranking losses.
Exact reranking recovers all 2,000 true neighbors; larger pools recover none.
No serving default, public API, score/certificate contract or codec format changed.
The result note records confidence intervals, label metrics, timing/storage
costs, independent trace audits and limits. Subsequent model experiments need
new held-out queries because this final set is now exposed.
