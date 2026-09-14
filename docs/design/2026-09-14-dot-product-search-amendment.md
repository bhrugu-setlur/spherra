# Additional dot-product search — 2026-09-14

The user-approved [reconstruction-length amendment](2026-09-14-reconstruction-length-amendment.md) supersedes the original
final-ranking rule below. Primary Q24 scoring and raw-score certificates remain unchanged.

The user requested a method that makes vector length useful in search while
preserving the existing search. This amendment implements that authorized scope
as `Index::search_dot_product(&Vector, SearchOptions) -> Result<DotProductResult, Error>`.
It adds an optional search metric; it does not replace the cosine contract.

## Public behavior

`search` still returns `SearchResult`/`Hit` with cosine scores and intervals.
The new method returns distinct `DotProductResult`/`DotProductHit` types with
private fields, the same row/segment/count getters, `score()`, `interval()` and
`stored_magnitude()`. Dot scores approximate the original input query dotted
with the original indexed input. The query length is computed in FP64, without
FP16 rounding. Options, validation, tie order, row visibility and default
candidate budget remain identical. Unit stored row lengths yield the same row
ranking as cosine. Query rescaling by a positive factor preserves ranking and
scales the displayed dot scores (subject to normal input/arithmetic rounding).

Candidate selection and refinement both use the stored row length. This is
necessary: weighting only cosine finalists could omit long vectors that belong
in the dot-product candidate pool. The full primary scan is exhaustive; the
candidate pool and final ranking remain approximate. No exact-top-k guarantee
is introduced. A larger length is not automatically desirable: a negative
similarity times a larger positive length is a worse dot product.

## Comparison arithmetic

A finite nonnegative FP16 length is `U * 2^-24`, with integer `U < 2^40`.
For exponent bits E and fraction F, U is F if E=0, otherwise
`(1024 + F) << (E - 1)`. Multiply each primary/refined i64 Q24 score by U in
i128. Every product has absolute value below 2^103, so the ordering is exact,
including negative scores, tiny subnormals and ties. Sort descending by this
integer and ascending by row ID. The query norm is a common positive multiplier
and does not enter ordering. Displayed scores are `(key / 2^48) * query_norm`
rounded to FP64; two distinct integer keys may display the same floating score.
This derived comparison is local to this method. Codec/scorer identities and
all stored bytes remain unchanged.

The two-byte-per-row cache already introduced for stored magnitude is reused.
There is no additional per-row cache or original-vector storage. Dot candidate
heaps use i128 keys and therefore have modest additional O(workers * budget)
query memory. The existing worker pool and bounded candidate-only residual
reads are reused. Both methods share one internal scan/refinement routine that
differs only in its ordering key; the cosine key is the unchanged i64 score and
never reads the magnitude cache, so cosine ordering, scores and intervals are
unchanged.

Stored lengths use a fixed 2^-24 FP16 step below 2^-14. Lengths below about
3e-8 store as zero, so those rows tie at score zero in row-ID order; below about
1e-5 the rounding exceeds 0.5% of the length. Such rows may be misordered among
themselves, while their intervals still enclose truth.

## Original-dot intervals

Start from the same authenticated primary/refined cosine intersection. Derive
an interval for the original row norm from the closed FP16 rounding cell:
midpoints to the previous and next representable positive half. Zero has lower
endpoint zero and upper endpoint 2^-25; maximum finite uses the hypothetical
next value 65536, avoiding infinity. Midpoints are exact FP64 dyadics. Closed
endpoints safely include both tie-to-even possibilities.

The stored half rounded an FP64 norm, not an exact-real norm. With FP64 unit
roundoff u=2^-53, the nonnegative 768-FMA squared-norm reduction has relative
error bounded by gamma(768)=768u/(1-768u). Square root and its rounding remain
well below 2^-40 relative error, including the inverse relation from computed
norm to true norm. Expand cell endpoints by factors (1-2^-40, 1+2^-40), with
outward rounding. This includes all positive FP16 subnormals and zero underflow;
a uniform relative '0.05%' claim would be incorrect there.

For the query, FP32 squares are exact FP64 values. Outward rounding after each
addition, then outward square roots, encloses its exact-real norm. Multiply the
cosine, row-norm and query-norm intervals using all four endpoint products and
outward rounding, preserving negative intervals and intervals crossing zero.

Finally expand by an absolute `2^-40 * row_norm_upper * query_norm_upper`.
This bridges the certified normalized FP64 dot to the unnormalized FP64 FMA
truth: two normalization L2 errors (each below 9e-14), the normalized reduction
error and the original reduction error sum to less than 4e-13 times the norm
product, below 2^-40 (approximately 9.09e-13). Cauchy–Schwarz bounds the absolute
sum even under cancellation. The valid FP32 input range keeps these products,
norms and divisions normal in FP64. Every intermediate endpoint and padding
calculation rounds outward. Nonfinite or inverted final intervals return
`CertificateInvalid`. The same intact-file/honest-builder trust boundary applies.

## Verification and evidence

- Exhaust all 31,744 nonnegative finite half encodings for exact unit conversion,
  valid rounding cells and i64-extreme product safety.
- Compare worker counts 1/6, partial tiles, multiple segments, budgets and k>N
  against independently computed scalar weighted scans/refinement. Check original
  FP64 dot enclosures, append/reopen, invalid inputs, concurrent cosine/dot use,
  positive query scaling, negative scores and zero/subnormal length handling.
- Prove a long vector outside a one-candidate cosine pool wins dot search.
- Verify unit stored row lengths retain cosine row order across budgets.
- Use a fixed rank-64 SVD of real MovieLens ratings, zero-padded to 768, without
  row normalization: 512 disjoint item factors for codec training, 1170 indexed
  item factors and 200 user queries. Compare with exact dot retrieval and report
  cosine results against that dot objective. This is a model-factor retrieval
  test, not held-out recommendation relevance or a native 768D model claim.
- Run all normal gates. Use clean release commits for serial 1M cosine and dot
  latency qualifications and a short 10M dot resource smoke. New dot performance
  is checked against the existing numerical latency targets as an initial
  measurement, not a general SLO promise. Archive raw data in the technical note.
