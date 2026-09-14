# Reconstruction-length correction — 2026-09-14

I approved adopting the tested no-storage reconstruction-length
correction (Option 1) in serving. This supersedes the no-renormalization and
integer-only final-order rules in local design D§5, implementation Task 8 and
the dot-product amendment. Codec kernels and their integer comparison contract
are unchanged. No additional approval or format revision is required.

## Serving behavior

Both `search` and `search_dot_product` correct every refined candidate before
selecting the final k. The exhaustive primary scan, integer heap keys, candidate
budget (`max(200, 2*k)`), input validation and primary tie order are unchanged.
The correction cannot recover a row omitted by that primary pool. Full coverage
on tested cosine workloads is evidence about those workloads, not a guarantee.

For the already loaded PQ code and resident primary code, decode p and e. In
coordinate order compute `v = f64(p[i]) + f64(e[i])`, sum `v*v` without FMA, and
set `L = sqrt(sum)`. Keep the original Q24 refined integer R. The corrected key
is `C = f64(R) / L`. For a zero or non-normal L use `f64(R)` instead, avoiding
undefined divisions for a degenerate reconstruction. With validated finite FP32
centers, every nonzero reconstruction length is finite and normal in FP64.

Cosine finalists sort by C descending, then row ID ascending. Public score is
`C / 2^24`. Dot finalists sort by `C * U` in FP64, where stored row magnitude is
`U * 2^-24`; public score is `(C * U / 2^48) * query_norm`. For U=0 canonicalize
the key to positive zero, preserving row-ID ties across signs. Query norm does
not enter ranking. Equal keys tie by row ID; displayed dot scores can round
distinct keys together. No clamp, learned correction, original alignment factor
or original-vector reads are introduced. Unit stored lengths preserve identical
cosine/dot row order. Final ordering is now FP64; the primary scan and the raw
certificate numerator remain exact Q24 integer operations under scorer version 1.

This deliberately retains the Q24 numerator rather than replacing it with the
FP64 numerator in the earlier benchmark experiment. The small rounding difference
is measured against that experiment, not assumed away.

## Certificate proof and compatibility

For each returned row, reconstruct the same primary and refined intervals from
the authenticated **raw** primary and refined integers, and intersect them as
before. These bounds already enclose that row's original-space cosine truth,
which has not changed. Choosing different rows or displaying another estimate
does not invalidate a bound on the same row's truth. Dot search applies its
existing outward-rounded query/stored-length scaling to this intersection.

Do not divide the intervals by L or center their old epsilon on C: either would
change the certified target without a proof. The returned estimate need not lie
inside the truth interval. Certificates promise truth enclosure, not correctness
of the approximate estimate or exact top-k membership. Kernel, codec, model,
segment and certificate identities remain unchanged; existing indexes open
without rebuilding and produce the new scores/order under this library version.

There is no additional stored or resident per-row data and no additional disk
read. Each finalist's already loaded residual code is decoded once for its
length, alongside the resident primary code. Extra work is O(768 * budget),
with bounded temporary arrays and O(budget) keys. Latency must be measured;
"a fraction of a millisecond" is not part of this contract.

## Verification and measurement

Independent scalar references reconstruct lengths and compare public scores and
orders at multiple budgets, across partial tiles, segments and worker counts.
Tests cover shrinkage, expansion, negatives, FP64 coordinate addition, zero
reconstructions, finite extremes, zero-magnitude ties, query scaling and unchanged
same-row certificate intervals. All returned hits must still enclose truth.

Quality/loss reports use schema version 2: public score equality compares the
corrected FP64 score, never a fictitious integer recovered by multiplying it by
2^24. Raw reference numerators remain explicit. Loss traces append reconstruction
length and corrected score; delivery/rank-loss fields describe corrected serving.
FP64-only and stored-alignment experiments keep their separate fields. Schemas
and audit tooling retain support for historical version-1 evidence.

Record clean release quality on the pinned generated 1M, real MS MARCO final
queries and native DPR 1M dot workload, plus serial cosine/dot latency checks.
These reused evaluation sets verify adoption; they are not new held-out proof
that every model benefits. Keep raw test data in the technical note.
