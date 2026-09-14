# Spherra

Spherra is a local, embedded Rust vector index for 768-dimensional embeddings.
It shrinks each vector to about one sixth of its size, searches the small
copies, and returns the best matches with a proven range for each score.

This README explains the math by following one small 3D vector through every
step. The real index does exactly the same steps, just with 768 numbers instead
of 3.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/transform-animation-dark.svg">
  <img src="docs/images/transform-animation-light.svg" alt="Animation: the vector x = (4, 1, 0.5) is normalized to length 1, has signs flipped, coordinates shuffled and mixed until all three are similar in size, then snaps to a grid point p and is corrected to r = p + ê.">
</picture>

## The problem

An embedding is a list of numbers, called coordinates, that describes a piece of
text or an image. Two embeddings are "similar" when they point in nearly the
same direction. That is measured by the **cosine similarity**: 1 means the same
direction, 0 means unrelated.

A 768-number embedding stored as normal 32-bit floats takes 3,072 bytes.
Ten million of them take 30 GB. Spherra's goal is to store each one in far fewer
bytes while still being able to compute similarity scores that are close to the
real ones.

The plan has two parts:

1. **Reshape** the vector so every coordinate is about the same size. This
   changes nothing about similarity, but it makes the next part work well.
2. **Round** each coordinate to a few allowed values, then store a small
   correction for the rounding mistake.

We will follow this example vector the whole way:

$$x = (4,\ 1,\ 0.5)$$

## Part 1: Reshape the vector

### Step 1: Separate the direction from the length

Cosine similarity only cares about direction. So we split `x` into two pieces:
its **length**, and a **direction** vector of length 1.

The length comes from the Pythagorean theorem:

$$\lVert x \rVert = \sqrt{4^2 + 1^2 + 0.5^2} = \sqrt{17.25} = 4.153$$

Dividing every coordinate by the length gives the direction:

$$u = \frac{x}{\lVert x \rVert} = \left(\frac{4}{4.153},\ \frac{1}{4.153},\ \frac{0.5}{4.153}\right) = (0.963,\ 0.241,\ 0.120)$$

`u` points the same way as `x` but has length exactly 1. The length 4.153 is
saved separately in 2 bytes, for searches where length matters.

**The problem with `u`:** almost all of its size is in the first coordinate
(0.963), while the last one is tiny (0.120). In Part 2 we will round every
coordinate. Rounding a big coordinate can cause a big mistake, and one coordinate
should not carry all the risk. We want all three coordinates to be about the
same size. Steps 2 to 4 fix this.

### Step 2: Flip some signs

Multiply each coordinate by a random `+1` or `−1`. Here the random choice is
`(+1, −1, −1)`:

$$(0.963 \times 1,\ \ 0.241 \times -1,\ \ 0.120 \times -1) = (0.963,\ -0.241,\ -0.120)$$

### Step 3: Shuffle the coordinates

Move the coordinates into a random new order. Here the second coordinate moves
to the first slot, the third to the second, and the first to the third:

$$(0.963,\ -0.241,\ -0.120) \rightarrow (-0.241,\ -0.120,\ 0.963)$$

Steps 2 and 3 do not spread anything out yet. They add randomness, so that the
mixing in Step 4 never lines up badly with some unlucky input vector.

### Step 4: Mix the coordinates together

Now every coordinate is blended with every other one. In this 3D example, the
mix is:

> add up the three coordinates, take 2/3 of that total, and subtract it from
> each coordinate.

The total is `−0.241 − 0.120 + 0.963 = 0.602`, and 2/3 of that is `0.401`:

$$y = (-0.241 - 0.401,\ \ -0.120 - 0.401,\ \ 0.963 - 0.401) \approx (-0.642,\ -0.522,\ 0.562)$$

(Numbers in this README are rounded to 3 decimals, so the last digit can be off
by one.)

Look at what happened to the sizes of the coordinates:

| | Coordinate 1 | Coordinate 2 | Coordinate 3 |
|---|---:|---:|---:|
| Before (`u`) | 0.963 | 0.241 | 0.120 |
| After (`y`) | 0.642 | 0.522 | 0.562 |

The big coordinate got smaller, the small ones got bigger, and now all three are
similar. That was the goal.

Spherra uses a **Hadamard transform** for the mix. It follows the same idea
across 128 coordinates at a time. Spherra runs Steps 2 to 4 twice, with
different random choices each time. The random choices come from a saved seed,
so the exact same reshaping can be repeated for every vector and every query.

### Why reshaping is allowed

Steps 2, 3 and 4 are all **rotations** (or mirror flips) of space. They move the
vector around without stretching it, so:

- `y` still has length 1: $0.642^2 + 0.522^2 + 0.562^2 = 1$.
- The angle between any two vectors stays the same. So if we reshape the query
  the same way, the cosine similarity between query and vector is unchanged.

That means we never need to undo Steps 2 to 4. Spherra searches directly in the
reshaped space.

## Part 2: Store the vector in a few bits

### Step 5: Round each coordinate to a grid

Instead of storing exact numbers, we only allow a few values per coordinate.
To keep the rounding mistakes easy to see, this example allows just 4 values:

$$\text{allowed values} = (-0.75,\ -0.25,\ 0.25,\ 0.75)$$

Each allowed value gets a number, called its **code**: `0, 1, 2, 3`. We round
each coordinate of `y` to the nearest allowed value and keep only its code:

| Coordinate | Value in `y` | Nearest allowed value | Code stored |
|---|---:|---:|---:|
| 1 | −0.642 | −0.75 (0.108 away) | 0 |
| 2 | −0.522 | −0.75 (0.228 away) | 0 |
| 3 | 0.562 | 0.75 (0.188 away) | 3 |

The rounded vector is `p = (−0.75, −0.75, 0.75)`, and all we store is
`(0, 0, 3)`. Four possible codes need only 2 bits each, so this takes 6 bits
instead of 96.

How big was the mistake? The distance between `y` and `p` is:

$$\lVert y - p \rVert = \sqrt{0.108^2 + 0.228^2 + 0.188^2} = 0.315$$

Spherra allows 16 values per coordinate (4 bits each). It picks those 16 values
separately for each coordinate by looking at training vectors and choosing
values spread evenly through the numbers it saw. That is where 768 × 4 bits =
**384 bytes** comes from.

### Step 6: Store a correction for the rounding mistake

The mistake left over from rounding is:

$$e = y - p = (-0.642 + 0.75,\ \ -0.522 + 0.75,\ \ 0.562 - 0.75) = (0.108,\ 0.228,\ -0.188)$$

Storing `e` exactly would cost as much as storing `y`. Instead, Spherra keeps a
shared list of typical mistakes, called a **codebook**, that is learned once
from training vectors. For each vector we store only the position of the list
entry closest to its mistake.

This example uses a codebook with 4 entries:

| Entry | Correction | Distance from `e` |
|---:|---|---:|
| **0** | **(0.2, 0.2, −0.2)** | **0.097** |
| 1 | (−0.2, 0.2, 0.2) | 0.496 |
| 2 | (0.2, −0.2, 0.2) | 0.585 |
| 3 | (−0.2, −0.2, −0.2) | 0.528 |

Entry 0 is closest, so we store `0`. Call that correction `ê`. Adding it to the
rounded vector gives our best rebuilt version of `y`:

$$r = p + \hat e = (-0.75 + 0.2,\ \ -0.75 + 0.2,\ \ 0.75 - 0.2) = (-0.55,\ -0.55,\ 0.55)$$

The mistake shrank from 0.315 to:

$$\lVert y - r \rVert = 0.097$$

A single codebook for all 768 numbers would need to be enormous. So Spherra
cuts the mistake into **96 pieces of 8 numbers** and gives each piece its own
codebook of 256 entries. One byte is enough to pick one of 256 entries, so the
correction costs **96 bytes**. This technique is called **product
quantization**, and the codebooks are learned with k-means clustering.

### The whole process in one picture

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/compression-dark.svg">
  <img src="docs/images/compression-light.svg" alt="Four panels. 1: x is scaled to the unit vector u. 2: a random rotation turns u, whose coordinates are 0.96, 0.24 and 0.12, into y, whose coordinates are 0.64, 0.52 and 0.56. 3: each coordinate of y snaps to the nearest grid level, giving p. 4: the leftover e = y − p is replaced by its nearest codebook entry ê, and r = p + ê lands much closer to y.">
</picture>

### What gets stored

| | 3D example | Spherra (768 numbers) |
|---|---|---:|
| Original vector | 3 floats = 12 bytes | 3,072 bytes |
| Rounded codes (Step 5) | `(0, 0, 3)` = 6 bits | 384 bytes |
| Correction code (Step 6) | `0` = 2 bits | 96 bytes |
| Length and flags (Step 1) | 4.153 | 4 bytes |
| **Total** | | **484 bytes, 6.3× smaller** |

To rebuild a vector: look up the rounded values from the codes to get `p`, look
up the correction to get `ê`, and add them to get `r`.

## Searching

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/search-dark.svg">
  <img src="docs/images/search-light.svg" alt="Left: the query q and the vectors y, p and r in 3D. Right: bars comparing scores with the true cosine 0.933: fast scan q·p gives 1.203, refined q·r gives 0.882, and length-corrected q·r divided by the length of r gives 0.926.">
</picture>

A query such as `(3, 2, 1)` goes through Steps 1 to 4 with the same random
choices, giving `q`. Because reshaping keeps angles, `q·y` is the true cosine
similarity: **0.933**. Spherra never has `y`, so it estimates that score in three
rounds:

| Round | Score | Example | Done for |
|---|---|---:|---|
| 1. Quick score | `q·p` | 1.203 | every stored vector |
| 2. Add the correction | `q·r` | 0.882 | the best 200 from round 1 |
| 3. Fix the length | `q·r / ‖r‖` | 0.926 | the same 200 |

Round 3 is needed because `r` is only close to length 1 (here 0.953), and a
shorter or longer vector skews the score. Dividing by its length removes that
skew. The final results are sorted by the round 3 score.

**Proven score ranges.** The Cauchy–Schwarz inequality says
$\lvert q \cdot y - q \cdot r \rvert \le \lVert q \rVert \, \lVert y - r \rVert$.
Spherra records the largest rebuild mistake `‖y − r‖` when it writes vectors to
disk, so every result comes with a range that is guaranteed to contain its true
score.

## How well it works

On an Apple M1 Pro, compared with exact search
([details](docs/benchmarks/README.md)):

| Workload | Vectors | Recall@10 | Median / p99 |
|---|---:|---:|---:|
| Generated cosine | 1,000,000 | 0.9255 | 72 / 114 ms |
| MS MARCO cosine | 100,000 | 0.9770 | — |
| DPR dot product | 1,000,000 | 0.9234 | 73 / 108 ms |

Recall@10 is the share of the true 10 closest vectors that Spherra returns.
