---
type: "query"
date: "2026-08-04T22:24:56.824770+00:00"
question: "will spherra store its vectors in polar form?"
contributor: "graphify"
source_nodes: ["Vector representation and scoring", "Searchable FP32 Memtables", "Direct Int4 Direction Code", "PQ96x8 Residual Refinement", "Immutable Vector Segments", "PolarCode"]
---

# Q: will spherra store its vectors in polar form?

## Answer

Expanded from original query via graph vocabulary: [polar, vector, representation, storage, direction, angle, code, residual, refinement, searchable, memtables, segments]. Yes, with an important qualification. Newly written vectors remain as normalized FP32 directions plus an FP16 magnitude in searchable memtables so they can be searched exactly and immediately. When flushed into immutable segments, each vector is stored as an FP16 magnitude, a compressed four-bit transformed direction, and a small PQ96x8 residual used only to refine promising candidates. This is polar because magnitude and direction are separated, but the direction is not stored as 767 recursive hyperspherical angles. Full FP32 originals are not part of the resident database promise and may be retained externally for validation or migration.

## Source Nodes

- Vector representation and scoring
- Searchable FP32 Memtables
- Direct Int4 Direction Code
- PQ96x8 Residual Refinement
- Immutable Vector Segments
- PolarCode