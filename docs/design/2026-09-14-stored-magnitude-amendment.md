# Stored magnitude amendment — 2026-09-14

I continued with item 2 (return each result’s magnitude) after
completion of the vector-length audit. This narrow amendment extends the local
index design and implementation specification; the original 14-task plan remains
complete.

## Contract

Add `Hit::stored_magnitude() -> f32`. It returns the original input’s stored FP16
length promoted exactly to FP32. It is rounded metadata, not the original FP64
length or a confidence estimate. Tiny accepted positive inputs can round to zero;
positive FP16 subnormals are valid. The value belongs to the returned row and
remains available after the index is dropped.

Opening an index retains a boxed `u16` slice per segment, loaded from the first
two little-endian bytes of each existing radius/flags word. Reads use the checked
primary reader in blocks of at most 4096 words. Signed encodings (including
negative zero), infinities and NaNs are corruption. Flags retain their existing
contract. Load and validate all values before exposing an index handle, then
close each primary file as before.

The cache payload is exactly two bytes per row: 2 MB at 1M and 20 MB at 10M
(decimal), plus per-segment slice headers and allocator overhead. Temporary read
buffers are bounded; retained descriptors remain segment count plus one.

The magnitude is copied into a hit only after ranking. Cosine candidate
selection, Q24 scores, tie ordering, certified intervals, budget 200 and durable
bytes/identities are unchanged. Valid existing indexes need no rebuild. This
checkpoint does not implement dot product, L2, magnitude filtering or flags.

## Verification

Test varied lengths through create, append, reopen and worker counts, including
rounding, subnormals, zero underflow and maximum finite FP16. Rehashed malformed
magnitudes must fail opening. Replacing only stored magnitudes must leave every
row, integer score and interval bit-identical. Test bounded reader endpoints and
one positional read per valid block, with no reads for invalid ranges. Run full
workspace gates and record clean-release existing-index resource measurements
in a technical note; short resource runs do not qualify latency percentiles.
