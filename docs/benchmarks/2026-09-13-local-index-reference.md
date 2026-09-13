# Local index historical recall reference

Task 1 measurements, 2026-09-13, from clean commit
`caa8229418101dafa33749ca1148227a4dbcac7d`. Both commands exited successfully
and validated their output against the embedded measurement schema. Every
entry records the machine, compiler, identities, corpus hash, seed, and command.
These are aggregate recall references, not hit-by-hit algorithm references or
index latency measurements.

## Archived input

The full 5,183-row SciFact file is preserved outside Git at
`corpora/archive/b2e549ce3a605944e44c21fb93bd912a130ffbe1ba914ac70befddf6eaa0e254.f32`.
The copy is read-only, is 15,922,176 bytes, and was independently verified with
Python's `blake3` before measurement. Its
[descriptor](../../corpora/archive/scifact-mpnet-768-2026-09-13.json) pins that
full-file hash and path. The harness verified the length and hash again before
loading. With 200 queries, the split is 3,688 indexed rows and 1,295 calibration
rows. The JSON results' `corpus_hash` covers the indexed split only.

Keep this archive for future comparisons; rebuilding embeddings from the same
upstream revisions has already produced different bytes. The ordinary builder
output in `corpora/scifact/` is separate from this archive.

## Results

| Corpus | Candidate budget | recall@10 | recall@100 |
| --- | --- | --- | --- |
| Archived SciFact | 20 | 0.9745 | 0.2000 |
| Archived SciFact | 200 | 0.9745 | 0.98115 |
| Archived SciFact | 1,000 | 0.9745 | 0.98115 |
| Archived SciFact | 3,688 (all) | 0.9745 | 0.98115 |
| Generated correlated 20k | 20 | 0.9330 | 0.2000 |
| Generated correlated 20k | 200 | 0.9370 | 0.95525 |
| Generated correlated 20k | 1,000 | 0.9370 | 0.95525 |
| Generated correlated 20k | 20,000 (all) | 0.9370 | 0.95525 |

Every entry has zero primary and zero refined certificate violations. The
recall@100 values at budget 20 reflect its maximum of 20 returned candidates.
Raw floating-point values and all provenance are preserved in the
[SciFact JSON](results/2026-09-13-local-index-reference-beir-scifact-mpnet-768.json)
and [generated JSON](results/2026-09-13-local-index-reference-generated-correlated-768x20000.json).

## Specification correction approved by the user

The table in approved design D§2 is reproduced to its displayed precision.
However, the following sentence is contradicted:

> Recall@10 already plateaus at budget 20 on both corpora.

Replacement approved by the user and applied to D§2:

> Recall@10 is unchanged across the measured budgets 20, 200, 1,000, and all
> rows on archived SciFact. On generated correlated 20k it rises from 0.9330 at
> budget 20 to 0.9370 at budget 200, and is unchanged at 1,000 and all rows.
> Budgets between 20 and 200 were not measured in this reference.

The following design sentence, “The limit is the codec's refined score, not
the budget,” now reads:

> At the measured budgets of 200 and above, the codec's refined ranking limits
> recall on these two corpora.

The user approved this factual correction and continuing with Task 2. The
search algorithm and its default budget of 200 are unchanged.

## Reproduction

Run from the `local-index` worktree with the archive in place:

```bash
cargo run -p spherra-bench --release --locked -- codec-format \
  --corpus corpora/archive/scifact-mpnet-768-2026-09-13.json \
  --queries 200 --seed 20260804 --candidate-budget 20,200,1000,3688 \
  --layout tiled-soa-32 --output target/measure/local-index-task1-scifact.json

cargo run -p spherra-bench --release --locked -- codec-format \
  --corpus generated-correlated-768x20000 \
  --queries 200 --seed 20260804 --candidate-budget 20,200,1000,20000 \
  --layout tiled-soa-32 --output target/measure/local-index-task1-generated.json
```

The harness's default `warm` cache label is an operator assertion, not a cache
conditioning guarantee. No cache conditioning was performed. The two runs were
sequential, with no source changes during either measurement. Their scan/rerank
rates are codec-harness measurements, not public-index latency evidence.
