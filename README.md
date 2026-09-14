# Spherra

Spherra is a local, embedded Rust index for 768-dimensional embeddings and
cosine similarity. It compresses each vector, scans every primary code, refines
the best candidates from disk, and returns approximate top-k hits with an
interval around each hit's true score. Rows are added offline in atomic commits.

The local library is implemented and passes its measured acceptance gates on
an Apple M1 Pro with 32 GiB RAM. The earlier distributed database direction is
stopped. Current authority is the [local index design](docs/design/2026-09-13-local-index-design.md)
and [implementation specification](docs/design/2026-09-13-local-index-implementation-spec.md).
See [STATUS.md](STATUS.md) for delivery and verification status.

## Measured results

Generated correlated 768-dimensional data, six workers, k=10, candidate budget
200, 50 warmups followed by 1,000 timed public searches, release build on AC:

| Indexed rows | p50 search | p99 search | Peak open/search RSS |
|---|---:|---:|---:|
| 1 million | 52.62 ms | 147.28 ms | 0.37 GiB |
| 10 million | 458.15 ms | 616.47 ms | 3.59 GiB |

Both latency gates pass. The separate builder probe measured about **600 MiB
of builder-owned memory**, below its 2 GiB gate, excluding caller-owned input
buffers. Building the generated indexes measured about 11,700–11,900 rows/s.
Opening took about 1.05 seconds at 1M and 11.75 seconds at 10M.

Quality uses 200 held-out queries per corpus, k=10 and budget 200:

| Corpus | Indexed rows | Recall@10 |
|---|---:|---:|
| Archived SciFact / MPNet | 3,688 | 0.9750 |
| Generated correlated 20k | 20,000 | 0.9355 |
| Chunked generated correlated 1M | 1,000,000 | 0.9095 |

All 6,000 returned hits match the independent checked-scalar reference exactly
and enclose original-space FP64 truth. The two historical recall comparisons
meet the permitted loss of 0.01. The 1M recall uses an exact reference pinned
before index measurement. These are vector-neighbor comparisons using held-out
embedding rows, not published BEIR task scores. Recall at 10M was not measured.

[Commands, raw samples, per-hit results and provenance](docs/benchmarks/README.md)
include the original scalar baselines and the final safe tile kernel results.

## Use the library

The workspace pins Rust 1.88.0, edition 2024. The public crate is `spherra`.
Supply your own finite 768-dimensional vectors. Norms below `1e-12` are rejected
as unreliable, and stored norms must fit FP16. Training rows are separate from
indexed rows unless explicitly pushed.

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
        let _row_id = builder.push(row)?;
    }
    let commit = builder.commit()?;
    println!("generation {}: {} rows added", commit.generation(), commit.rows_added());

    let index = Index::open(directory)?;
    let result = index.search(
        query,
        SearchOptions { k: 10, candidate_budget: None },
    )?;
    for hit in result.hits() {
        println!("row {}: score {}, interval {:?}, stored length {}",
            hit.row().get(), hit.score(), hit.interval(), hit.stored_magnitude());
    }
    Ok(())
}
```

`candidate_budget: None` resolves to `max(200, 2*k)`, then clamps to the index
size. `k` must be positive; an explicit budget below `k` is invalid. If `k`
exceeds the index size, every row is returned. Ordering is score descending,
then row ID ascending. Row IDs are dense ordinals scoped to one index.

Both methods divide each refined score by the length of the reconstructed
compressed vector before final ranking. This reduces compression shrinkage bias
using existing bytes, with no extra storage or rebuild. It corrects only the
shortlisted rows; it cannot recover a neighbor missed during candidate selection.
A zero reconstruction length leaves the uncorrected score unchanged.

`hit.stored_magnitude()` returns the input length rounded to FP16 and promoted
to FP32. Tiny positive lengths may round to zero. In cosine search it is
metadata and does not affect ranking or score intervals. Opening retains two bytes per row (20 MB at
10M rows, plus per-segment overhead); existing valid indexes need no rebuild.

For models whose vector length carries meaning, use the separate dot-product
method on the same index:

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

Dot-product search uses stored lengths during the full scan and refinement.
Its distinct result type reports approximate original-vector dot products,
including query length, and intervals that account for compression and stored
length rounding. Primary selection uses exact integer products; finalists use
FP64 scores corrected for reconstruction length. Displayed scores may round ties. Top-k remains approximate. Very short vectors rank poorly
among themselves: lengths below about 3e-8 are stored as zero (those rows score
zero and tie in row-ID order), and below about 1e-5 length rounding exceeds 0.5%.
Their intervals remain valid. When all stored row lengths equal one, row
ordering matches cosine.
It uses the existing length cache and needs no index rebuild or original vectors.
`search()` keeps its existing cosine behavior.

Drop all open `Index` handles before appending; they hold shared locks for their
lifetime. Builders take a nonblocking exclusive lock and return `IndexBusy` on
contention. Reopen after a commit to search the new generation.

```rust
use std::path::Path;
use spherra::{Error, IndexBuilder, RowId, Vector};

fn append_one(directory: &Path, row: &Vector) -> Result<RowId, Error> {
    let mut builder = IndexBuilder::append(directory)?;
    let id = builder.push(row)?;
    builder.commit()?;
    Ok(id)
}
```

## Scoring and intervals

Each vector has a 384-byte direct-int4 primary code, a four-byte radius/flags
word, and a 96-byte PQ residual. Two seeded sign/permutation/Hadamard rounds
condition the direction. Primary tiles stay in owned memory; residuals are
read positionally only for selected candidates.

The serving kernel is portable safe Rust on every CPU. It accumulates directly
from 32-row tiles after proving the integer range, with a checked scalar
fallback. It returns exactly the original Q24 integer scores. Stage 2 met the
latency gates, so no NEON kernel or unsafe code was added. Performance was
qualified on the M1 Pro; other CPUs use the same kernel without those timing
claims. There is no HNSW, routing, or certificate-based pruning.

A hit's interval encloses its original-space FP64 cosine score for an index
built by this library whose files remain intact. It does **not** prove that the
hit is an exact top-k neighbor. Primary and refined intervals are intersected;
their epsilons are not added. They use the original raw scores and still certify
truth after length correction; the displayed estimate need not lie inside the
interval. Hashes detect corruption but cannot establish an
honest bound in a deliberately rewritten, re-hashed file. Files must not be
modified outside the library while an `Index` is open.

## Training, append limits and drift

- At most 32,768 training rows. Default validation size is
  `min(4096, training.len()/4)`; validation needs at least one row and the
  remaining training set at least 256. Splits are deterministic and disjoint.
- Each segment contains at most 65,536 rows; staging retains at most one segment
  of originals. The format admits at most 4,096 segments and a total-row limit
  of `2^48 - 1`. The tested local target is 10M rows.
- An open index retains one residual descriptor per segment plus `LOCK`.
  Admission also reserves 64 descriptors for the caller. For example, a soft
  limit of 256 admits 191 segments. The library never raises that limit.
- Small commits consume segment slots. There are no deletes, filters, caller
  IDs, compaction, or segment merging in this version.
- Every commit reports reconstruction drift over a deterministic sample of at
  most 65,536 rows. It warns when refined p95 exceeds 1.25 times validation p95,
  or more than 5% of committed rows exceed validation refined p99. Commits below
  1,000 rows report an insufficient sample instead. This detects reconstruction
  change; it is not a measurement of recall loss.

## Directory states and durability

A directory without `CURRENT` is **Absent**, even if an interrupted create left
`LOCK` or unreferenced files. `open` and `append` return `NotFound`; `create`
cleans owned unreferenced artifacts and can retry. A valid `CURRENT` names one
committed generation. Corruption is an error, never silent fallback to an older
one. Unrelated user files and committed predecessor manifests are preserved.

| Commit outcome | Meaning and caller action |
|---|---|
| Error before `CURRENT` publication | Absent or the previous generation remains. |
| `CommitOutcomeUnknown { generation }` | Publication occurred but the final directory sync failed. Reopen and compare generations before retrying rows. |
| `Ok(CommitReport)` | The new generation was published and synced. |
| Success with `cleanup_complete() == false` | The commit succeeded; the next builder retries cleanup. |

New files are reopened and verified, synced, renamed, and followed by a directory
sync. `CURRENT` publishes last. Staging failures poison the builder. The recovery
suite covers injected failures, short writes, process kills and aborts on local
APFS. Physical power loss and other filesystems have not been qualified.

## Development

The normal gates are:

```bash
bash scripts/ci.sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Large corpus and recovery qualifications are explicit ignored tests; commands
and results are recorded in the [benchmark protocol](docs/benchmarks/README.md)
and [status](STATUS.md). Existing segment v1 bytes, codec identity, scorer
version and default candidate budget are unchanged.
