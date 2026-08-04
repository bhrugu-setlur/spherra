# Spherra

Spherra is a distributed vector database for 768-dimensional RAG and semantic-search embeddings. It combines compact polar-inspired vector storage with directional query routing, durable replication, and immediate search visibility for newly committed documents.

> **Project status:** architecture and implementation planning are approved; the production database has not been implemented yet. The repository currently contains design documents and codec/layout experiments.

## Architecture at a glance

- **PolarLSM** owns durable writes, exact version truth, searchable memtables, immutable segments, compaction, recovery, and replication.
- **PolarRouter** owns spherical direction cells, metadata-aware routing, certified pruning, filtered scans, and optional immutable cell-local HNSW.
- Each source document is chunked before ingestion. Every chunk receives one embedding plus metadata linking it to its parent document.
- Acknowledged writes must be visible to every subsequent session-consistent search before an immutable-segment flush.

The vector representation is inspired by ideas from Google Research's TurboQuant work: randomized orthogonal conditioning, low-bit scalar quantization, high-precision queries, and candidate-only residual refinement. Spherra's database architecture, durability model, routing, and experiments are project-specific.

## Current target

- Development machine: Apple M1 Pro, 32 GiB unified memory, 1 TB SSD.
- Local target: 10 million 768-dimensional chunks.
- Local stretch target: 25 million chunks after measurement.
- Billion-vector scale: distributed clusters only.

## Start here

1. Follow the living project guide and mandatory workflow.
2. Read the approved [production design](docs/design/2026-08-04-polar-lsm-router-design.md).
3. Read the approved [v1 implementation specification](docs/design/2026-08-04-polar-v1-implementation-spec.md).

The public product, repository, command, and Rust package prefix are **Spherra** / `spherra`. The names **PolarLSM** and **PolarRouter** remain the names of the two core subsystems.
