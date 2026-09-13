# Local index container v1 goldens

These bytes were generated independently with Python `struct`, BLAKE3, and a
bitwise CRC32C implementation, then compared with the Rust encoders. They pin
container encoding, not a complete searchable index. The manifest's segment
files are deliberately absent.

Every container has an 8-byte magic, u16 version 1, u64 payload length, and a
32-byte BLAKE3 of the preceding bytes. All numeric fields are little endian.
Names and references use the BLAKE3 of the **complete container**, including its
trailer. Segment v1 identities retain their existing zeroed-header-field rule.

- `current-v1.bin` (94 bytes): magic `SPHRCUR1`; payload generation u64,
  manifest hash, then CRC32C of the common header and preceding payload.
- `model-v1.bin` (835,868 bytes): magic `SPHRMOD1`; generator version u16,
  seed u64, expanded/transform/quantizer/codebook/codec identities (32 bytes
  each), scorer and layout u32, 12,288 FP32 centers, 196,608 FP32 centroids,
  then seven FP64 drift values: primary p50/p95/p99, refined p50/p95/p99,
  and outside-center fraction. Seed 20260804; zero tables with their actual
  identities; drift values 0.1/0.2/0.3, 0.05/0.1/0.2, and 0.01.
- `manifest-v1.bin` (258 bytes): magic `SPHRMAN1`; index id (16 bytes),
  generation u64, previous/model hashes (32 each), total rows u64, count u32;
  each 108-byte entry has segment id (16), first row u64, row count u32,
  primary/residual lengths u64, then primary/residual hashes (32 each).
  The fixture has generation 1 and three rows in one segment.

Changing these fixtures requires a deliberate container-version decision.
