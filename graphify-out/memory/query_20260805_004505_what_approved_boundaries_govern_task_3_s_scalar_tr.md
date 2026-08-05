---
type: "query"
date: "2026-08-05T00:45:05.177537+00:00"
question: "What approved boundaries govern Task 3's scalar transform oracle?"
contributor: "graphify"
source_nodes: ["ReliableDirection", "Task 3: Implement the exact scalar transform oracle", "spherra-simd/src/lib.rs"]
---

# Q: What approved boundaries govern Task 3's scalar transform oracle?

## Answer

Expanded from original query via graph vocabulary: [scalar, transform, codec, reliable, direction, simd, plan]. Task 3 places the fixed-size scalar H128 kernel in spherra-simd and the two-round transform plan in spherra-codec. Its public transform API must consume ReliableDirection so unreliable vectors cannot enter codec paths; the transform remains exactly two sign/permutation/H128 rounds with six 128-point blocks and no padding.

## Source Nodes

- ReliableDirection
- Task 3: Implement the exact scalar transform oracle
- spherra-simd/src/lib.rs