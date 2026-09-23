# Excluding an indexed query vector — 2026-09-23

Status: approved by the user's request to exclude the prompt vector in normal
library use and their choice that the caller supplies its index-assigned ID.
This amends the public API and search rules of the local index design. The
representation, segment v1 bytes, search ranking, score intervals, and default
candidate budget are unchanged.

## Public behavior

Add two methods to `Index`:

```rust
pub fn search_excluding(
    &self, query: &Vector, excluded: RowId, options: SearchOptions,
) -> Result<SearchResult, Error>;
pub fn search_dot_product_excluding(
    &self, query: &Vector, excluded: RowId, options: SearchOptions,
) -> Result<DotProductResult, Error>;
```

The caller retains the `RowId` returned by `IndexBuilder::push` and passes it
when querying that indexed vector. The excluded ID must refer to a vector in
the opened index; an ID greater than or equal to `Index::len()` returns
`InvalidOptions`. `RowId` is an ordinal scoped to an index. The library can
check its range but cannot establish whether an in-range ID came from another
index, so callers must keep the mapping with the corresponding index.

The existing `search` and `search_dot_product` methods remain for queries with
no indexed ID. They may return an identical indexed vector. The library cannot
infer exact vector identity from similarity scores or compressed codes, and it
does not retain original vector values after commit. Other indexed vectors
with the same values as the excluded vector remain eligible: exclusion is by
ID, not by value or score.

## Selection and counts

Both methods still score every primary vector. During each worker's primary
scan, skip the excluded ID before candidate admission; this keeps it from
occupying a candidate slot. Merge, residual refinement, final ranking and
interval construction otherwise follow the existing metric-specific rules.
Ordinary searches retain their existing scan path.

Validate `k` and requested candidate budget as before, then validate the
excluded ID, then validate the query. Clamp the effective budget to `N - 1`.
Return at most `min(k, N - 1)` hits. `rows_scanned()` remains `N`, since every
primary vector is scored, while `candidate_budget()` and `rows_refined()`
report the effective budget. Excluding the only indexed vector returns an empty
result with budget and refined count zero. The returned intervals continue to
certify the original-space score of each returned vector; exclusion adds no
exact-top-k claim.

## Verification

Focused public API tests cover cosine and dot-product exclusion before a
one-candidate budget is filled, identical vectors at different IDs, a second
segment, a one-vector index, counts, and an out-of-range ID. CI passed 221
tests with 12 large qualifications skipped. No stored index rebuild is
required.
