# Spherra

I built Spherra as a local, embedded Rust vector index for 768-dimensional
embeddings. It shrinks each vector to about one sixth of its size, searches the
small copies, and returns the best matches with a proven range for each score.
It runs inside your program: there is no server.

This README explains the math. It walks one small 3D vector through every step,
so you can see what happens to the numbers. The real index does the same steps
in 768 dimensions.

## The idea in one line

Store each vector as a **direction** (a point on a sphere) plus a **length**.
Compress the direction in two layers: a coarse layer that is fast to scan, and a
fine layer that corrects it for the best few candidates.

| Per stored row | Size |
|---|---:|
| Original FP32 vector (768 × 4 bytes) | 3,072 bytes |
| Coarse direction code (768 × 4 bits) | 384 bytes |
| Fine correction code (96 × 1 byte) | 96 bytes |
| Radius/flags word, including the FP16 length | 4 bytes |
| **Stored total** | **484 bytes (about 6.3× smaller)** |

## Compressing a vector

The picture below follows the example vector `x = (4, 1, 0.5)`. To keep the
errors big enough to see, the 3D toy uses a 4-level grid and a 4-entry codebook;
Spherra uses 16 levels and 256 entries.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/compression-dark.svg">
  <img src="docs/images/compression-light.svg" alt="Four panels. 1: x is scaled to the unit vector u. 2: a random rotation turns u, whose coordinates are 0.96, 0.24 and 0.12, into y, whose coordinates are 0.64, 0.52 and 0.56. 3: each coordinate of y snaps to the nearest grid level, giving p. 4: the leftover e = y − p is replaced by its nearest codebook entry ê, and r = p + ê lands much closer to y.">
</picture>

### 1. Keep the direction, store the length

$$u = \frac{x}{\lVert x \rVert}, \qquad \lVert x \rVert = \sqrt{4^2 + 1^2 + 0.5^2} = 4.153$$

So `u = (0.963, 0.241, 0.120)`, which has length 1. Cosine similarity only
depends on direction, so the rest of the pipeline works with `u`. The length is
kept as a 16-bit float for the optional dot-product search.

### 2. Randomly rotate

`u` is lopsided: nearly all of its size sits in the first coordinate. A grid with
the same levels on every coordinate would waste most of its levels on that
imbalance. So Spherra first applies a random rotation:

$$y = H \cdot P \cdot S \cdot u$$

- **S** flips the sign of some coordinates. Here `S = diag(1, −1, −1)`.
- **P** shuffles the coordinates into a new order.
- **H** mixes every coordinate into every other one. Spherra uses a normalized
  Hadamard block. The 3D figure uses `H = I − (2/3)·J`, where `J` is the all-ones
  matrix. Both are rotations that undo themselves.

The result is `y = (−0.642, −0.522, 0.562)`. The biggest coordinate dropped from
0.96 to 0.64, and all three are now similar in size.

A rotation never changes lengths or angles. For any two vectors,
`(R·a)·(R·b) = a·b`. That fact is what makes every later step allowed: a score
computed after rotating is the same score as before rotating.

In 768 dimensions Spherra runs this twice with different random choices. Each
round flips signs, shuffles all 768 coordinates, then applies six 128 × 128
Hadamard blocks. The random choices come from a seed, so the same seed always
rebuilds the same rotation.

### 3. Snap each coordinate to a grid (the coarse code)

Each coordinate of `y` is replaced by its nearest grid level:

| Coordinate | y | Nearest level from (−0.75, −0.25, 0.25, 0.75) | Code |
|---|---:|---:|---:|
| c₁ | −0.642 | −0.75 | 0 |
| c₂ | −0.522 | −0.75 | 0 |
| c₃ | 0.562 | 0.75 | 3 |

The coarse copy is `p = (−0.75, −0.75, 0.75)`, and only the codes `(0, 0, 3)`
are stored. Its error is `‖y − p‖ = 0.315`.

In Spherra every coordinate has its own 16 levels, so each code takes 4 bits.
The levels are chosen from training vectors: they are 16 evenly spaced points
in the sorted values seen for that coordinate (quantiles). This works well only
because step 2 made the coordinates look alike.

### 4. Fix the leftover with a codebook (the fine code)

The leftover is `e = y − p = (0.108, 0.228, −0.188)`. Spherra doesn't store it
directly. It stores the number of the closest entry in a shared codebook:

$$\hat e = \arg\min_{c \in \text{codebook}} \lVert e - c \rVert = (0.2, 0.2, -0.2), \qquad r = p + \hat e = (-0.55, -0.55, 0.55)$$

The error falls from 0.315 to `‖y − r‖ = 0.097`.

In 768 dimensions this is **product quantization**: the leftover is cut into 96
pieces of 8 numbers, and each piece picks one of 256 entries from its own
codebook. That is 96 bytes. Each codebook is learned from training leftovers
with k-means.

## Searching

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/search-dark.svg">
  <img src="docs/images/search-light.svg" alt="Left: the query q and the vectors y, p and r in 3D. Right: bars comparing scores with the true cosine 0.933: fast scan q·p gives 1.203, refined q·r gives 0.882, and length-corrected q·r divided by the length of r gives 0.926.">
</picture>

A query `z = (3, 2, 1)` is normalized and rotated with the same `S`, `P` and `H`,
giving `q = (−0.535, −0.267, 0.802)`. Because rotation keeps dot products,
`q·y` is exactly the true cosine between `z` and `x`: **0.933**.

| Step | Formula | Example | Work in 768-D |
|---|---|---:|---|
| True cosine | `q·y` | 0.933 | not available: originals are not stored |
| 1. Fast scan, every row | `q·p` | 1.203 | 768 table lookups |
| 2. Refine the best candidates | `q·r = q·p + q·ê` | 0.882 | plus 96 lookups |
| 3. Length correction | `q·r / ‖r‖` | 0.926 | one length per candidate |

**Step 1: fast scan.** Since `p` uses only 16 possible values per coordinate,
the query builds a table once: entry `[i][k]` holds `q[i] × level[i][k]`. A row's
score is then the sum of 768 lookups. Table entries are stored as whole numbers
scaled by 2²⁴, so the scan does exact integer addition. Spherra scores every row
this way and keeps the best `max(200, 2k)` as candidates.

**Step 2: refine.** For those candidates only, it reads the 96-byte fine code
from disk and adds `q·ê`, which uses a second lookup table.

**Step 3: correct the length.** The true `y` has length exactly 1, but `r` does
not: here `‖r‖ = 0.953`. Its length stretches or shrinks the score, so Spherra
divides it out. In the example this moves the score from 0.882 to 0.926, closer
to the truth. The final top k are sorted by this corrected score.

The coarse score overshoots in the example (1.203) because `‖p‖ = 1.30`. This
does not break the scan, because ranking only needs good candidates to land in
the top 200.

### Dot-product search

For embeddings where length matters, `search_dot_product` uses the stored length:

$$z \cdot x \approx \lVert z \rVert \cdot \lVert x \rVert \cdot \frac{q \cdot r}{\lVert r \rVert}$$

Both the scan and the refinement multiply each row's score by its stored length,
so long vectors are not dropped before refinement.

### Score ranges you can trust

Every hit comes with an interval that contains its true score. The main tool is
the Cauchy–Schwarz inequality:

$$\lvert q \cdot y - q \cdot r \rvert \le \lVert q \rVert \cdot \lVert y - r \rVert$$

When a segment is written, Spherra records the largest reconstruction error
among its rows, for both `‖y − p‖` and `‖y − r‖`. It then adds bounds for every
rounding step (floating-point rotation, the 2²⁴ integer scale). A hit's interval is its uncorrected score plus
or minus that total. Spherra intersects the coarse and refined intervals.

The interval is a promise about **that row's score**. It does not prove the
returned rows are the exact top k. The corrected score can fall slightly
outside the interval, because the interval is centered on the uncorrected score.

## How well it works

On an Apple M1 Pro with 32 GiB RAM, measured against exact search:

| Workload | Rows | Recall@10 | Median / p99 latency |
|---|---:|---:|---:|
| Generated correlated cosine | 1,000,000 | 0.9255 | 72.31 / 113.56 ms |
| MS MARCO cosine | 100,000 | 0.9770 | — |
| DPR dot product | 1,000,000 | 0.9234 | 73.01 / 108.24 ms |

Recall@10 is the share of the true 10 nearest rows that Spherra returns. The
[benchmark notes](docs/benchmarks/README.md) have the commands, raw results, and
limits of these numbers.

## Use the library

The workspace pins Rust 1.88.0 and edition 2024. Supply finite 768-dimensional
vectors; unreliable norms and values outside the stored FP16 range are rejected.
Training rows are separate from indexed rows unless explicitly pushed.

```rust
use std::path::Path;
use spherra::{CreateOptions, Error, Index, IndexBuilder, SearchOptions, Vector};

fn build_and_search(
    directory: &Path,
    training: &[Vector],
    rows: &[Vector],
    query: &Vector,
) -> Result<(), Error> {
    let mut builder = IndexBuilder::create(
        directory,
        training,
        CreateOptions { seed: 20260804, validation_rows: None },
    )?;
    for row in rows {
        builder.push(row)?;
    }
    let commit = builder.commit()?;

    let index = Index::open(directory)?;
    let result = index.search(
        query,
        SearchOptions { k: 10, candidate_budget: None },
    )?;
    println!("generation {}: {} rows added", commit.generation(), commit.rows_added());
    for hit in result.hits() {
        println!("row {}: score {}, interval {:?}",
            hit.row().get(), hit.score(), hit.interval());
    }
    Ok(())
}
```

`candidate_budget: None` resolves to `max(200, 2*k)`, clamped to the index size.
Results are ordered by score, highest first, then by row ID.

For non-normalized embeddings, the optional method uses input length:

```rust,no_run
# fn example(index: &spherra::Index, query: &spherra::Vector) -> Result<(), spherra::Error> {
let result = index.search_dot_product(
    query,
    spherra::SearchOptions { k: 10, candidate_budget: None },
)?;
for hit in result.hits() {
    println!("row {}: dot {}, interval {:?}",
        hit.row().get(), hit.score(), hit.interval());
}
# Ok(()) }
```

Drop open `Index` handles before appending. Builders take a nonblocking
exclusive lock, and the commit consumes the builder:

```rust,no_run
# fn append_one(directory: &std::path::Path, row: &spherra::Vector)
#     -> Result<spherra::RowId, spherra::Error> {
let mut builder = spherra::IndexBuilder::append(directory)?;
let row_id = builder.push(row)?;
builder.commit()?;
# Ok(row_id) }
```

## Storage and limits

- Commits are atomic: files are written, checked, and synced before the
  `CURRENT` pointer switches, so a crash leaves the previous version readable.
- Training is capped at 32,768 rows; a segment at 65,536 rows; and the index at
  4,096 segments or `2^48 - 1` total rows.
- Search scans every row. There is no graph index, filtering, deletion,
  compaction, replication, or server.
- Original vectors are not kept, so results are never re-scored against them.
- Durability is tested on local APFS with injected faults and killed processes;
  physical power loss and other filesystems are untested.

The full design is in the [local index design](docs/design/2026-09-13-local-index-design.md)
and [implementation specification](docs/design/2026-09-13-local-index-implementation-spec.md).
Run `bash scripts/ci.sh` for the test suite.

## Repository map

| Path | Contents |
|---|---|
| `crates/spherra/` | Public local index, builder, storage, publication, open, search, and recovery |
| `crates/spherra-codec/` | Rotation, 4-bit grid, product quantization, integer scoring, and score intervals |
| `crates/spherra-format/` | Explicit durable model/segment formats and checked readers |
| `crates/spherra-domain/` | 768-dimensional validation, IDs, and sequence/domain contracts |
| `crates/spherra-testkit/` | Deterministic corpora, exact oracles, machine provenance, and measurement support |
| `crates/spherra-bench/` | Reproducible quality, latency, memory, norm, and dot-product commands |
| `docs/benchmarks/` | Protocols, schemas, raw results, and limitations |
| `docs/images/` | README figures, drawn by `tools/readme_math_figures.py` |
| `docs/design/` | Current local-index contracts plus clearly marked historical direction documents |
| `corpora/` | Hash-pinned corpus descriptors; large vector payloads stay outside Git |
| `fuzz/` | Nightly-only checked decoder targets |
| `scripts/` and `tools/` | CI, dependency policy, corpus builders, audits, and result summaries |
