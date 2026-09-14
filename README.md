# Spherra

Spherra is a local, embedded Rust vector index for 768-dimensional embeddings.
It shrinks each vector to about one sixth of its size, searches the small
copies, and returns the best matches with a proven range for each score.

This README explains the math by following one 3D vector through every step.
The real index does the same steps in 768 dimensions.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/transform-animation-dark.svg">
  <img src="docs/images/transform-animation-light.svg" alt="Animation: the vector x = (4, 1, 0.5) is normalized to length 1, has signs flipped, coordinates shuffled and mixed until all three are similar in size, then snaps to a grid point p and is corrected to r = p + ê.">
</picture>

## What gets stored

Each vector becomes a **direction** plus a **length**. The direction is stored
in two layers: a coarse code that is fast to scan, and a fine code that corrects
it for the best candidates.

| Per row | Size |
|---|---:|
| Original FP32 vector | 3,072 bytes |
| Coarse code: 768 coordinates × 4 bits | 384 bytes |
| Fine code: 96 pieces × 1 byte | 96 bytes |
| Length (FP16) and flags | 4 bytes |
| **Stored total** | **484 bytes (6.3× smaller)** |

## Compressing a vector

To keep errors visible, the 3D example uses a 4-level grid and a 4-entry
codebook. Spherra uses 16 levels and 256 entries.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/compression-dark.svg">
  <img src="docs/images/compression-light.svg" alt="Four panels. 1: x is scaled to the unit vector u. 2: a random rotation turns u, whose coordinates are 0.96, 0.24 and 0.12, into y, whose coordinates are 0.64, 0.52 and 0.56. 3: each coordinate of y snaps to the nearest grid level, giving p. 4: the leftover e = y − p is replaced by its nearest codebook entry ê, and r = p + ê lands much closer to y.">
</picture>

**1. Normalize.** $u = x / \lVert x \rVert$. For `x = (4, 1, 0.5)`, the length is
4.153 and `u = (0.963, 0.241, 0.120)`. Cosine similarity only depends on
direction; the length is kept for dot-product search.

**2. Randomly rotate.** $y = H \cdot P \cdot S \cdot u$: flip some signs (S),
shuffle the coordinates (P), and mix them together (H, a normalized Hadamard
matrix). This gives `y = (−0.642, −0.522, 0.562)`. The largest coordinate drops
from 0.96 to 0.64, so one grid now suits every coordinate. A rotation never
changes dot products, $(Ra)\cdot(Rb) = a \cdot b$, so scores computed after it
are still correct. Spherra does two seeded rounds, each with six 128 × 128
Hadamard blocks.

**3. Snap to a grid (coarse code).** Each coordinate rounds to its nearest
level. Levels `(−0.75, −0.25, 0.25, 0.75)` give `p = (−0.75, −0.75, 0.75)`,
stored as codes `(0, 0, 3)`. Error: `‖y − p‖ = 0.315`. Spherra learns 16
levels per coordinate from the quantiles of training vectors.

**4. Fix the leftover (fine code).** The leftover `e = y − p` is replaced by the
nearest entry of a learned codebook:

$$\hat e = \arg\min_{c} \lVert e - c \rVert = (0.2, 0.2, -0.2), \qquad r = p + \hat e = (-0.55, -0.55, 0.55)$$

The error falls to `‖y − r‖ = 0.097`. In 768 dimensions this is product
quantization: 96 pieces of 8 numbers, each matched against 256 k-means entries.

## Searching

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/search-dark.svg">
  <img src="docs/images/search-light.svg" alt="Left: the query q and the vectors y, p and r in 3D. Right: bars comparing scores with the true cosine 0.933: fast scan q·p gives 1.203, refined q·r gives 0.882, and length-corrected q·r divided by the length of r gives 0.926.">
</picture>

The query `z = (3, 2, 1)` is normalized and rotated the same way, giving `q`.
Because rotation keeps dot products, `q·y` is the true cosine: **0.933**.

| Step | Formula | Example | Work in 768-D |
|---|---|---:|---|
| 1. Scan every row | `q·p` | 1.203 | 768 table lookups |
| 2. Refine the top `max(200, 2k)` | `q·r = q·p + q·ê` | 0.882 | + 96 lookups, one disk read |
| 3. Correct the length | `q·r / ‖r‖` | 0.926 | one length per candidate |

- **Scan:** `p` has only 16 values per coordinate, so the query precomputes
  `q[i] × level[i][k]` as whole numbers scaled by 2²⁴. A row's score is 768
  integer lookups added together.
- **Correct:** the true `y` has length 1, but `r` does not (0.953 here), which
  skews the score. Dividing by `‖r‖` moves 0.882 to 0.926.
- **Dot product:** `search_dot_product` multiplies each score by the stored
  lengths: $z \cdot x \approx \lVert z \rVert \lVert x \rVert \, (q \cdot r) / \lVert r \rVert$.

**Score ranges.** By the Cauchy–Schwarz inequality,
$\lvert q \cdot y - q \cdot r \rvert \le \lVert q \rVert \, \lVert y - r \rVert$.
Each segment records its largest `‖y − r‖`, and Spherra adds bounds for rounding.
Each hit's interval therefore contains its true score. It does not prove the
hits are the exact top k.

## How well it works

On an Apple M1 Pro, compared with exact search
([details](docs/benchmarks/README.md)):

| Workload | Rows | Recall@10 | Median / p99 |
|---|---:|---:|---:|
| Generated cosine | 1,000,000 | 0.9255 | 72 / 114 ms |
| MS MARCO cosine | 100,000 | 0.9770 | — |
| DPR dot product | 1,000,000 | 0.9234 | 73 / 108 ms |

## Use the library

Rust 1.88.0, edition 2024. Vectors must be finite and 768-dimensional.

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
    builder.commit()?;

    let index = Index::open(directory)?;
    let result = index.search(query, SearchOptions { k: 10, candidate_budget: None })?;
    for hit in result.hits() {
        println!("row {}: score {}, interval {:?}", hit.row().get(), hit.score(), hit.interval());
    }
    Ok(())
}
```

Use `index.search_dot_product(...)` when vector length matters, and
`IndexBuilder::append(directory)` to add rows later (drop open `Index` handles
first).

## Limits

- Commits are atomic: a crash leaves the previous version readable.
- Search scans every row; there is no graph index, filtering, deletion, or server.
- Original vectors are not kept.
- Up to 4,096 segments of 65,536 rows each; training is capped at 32,768 rows.

Design: [local index design](docs/design/2026-09-13-local-index-design.md).
Tests: `bash scripts/ci.sh`.

## Repository map

| Path | Contents |
|---|---|
| `crates/spherra/` | Public index: build, open, search, recovery |
| `crates/spherra-codec/` | Rotation, grid codes, product quantization, scoring, score ranges |
| `crates/spherra-format/` | On-disk formats and checked readers |
| `crates/spherra-bench/` | Quality, latency, and memory benchmarks |
| `docs/` | Benchmarks, design specs, and README figures |
| `tools/readme_math_figures.py` | Draws the README figures and animation |
