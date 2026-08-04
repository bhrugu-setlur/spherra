---
type: "query"
date: "2026-08-04T19:48:59.472069+00:00"
question: "What tech stack should PolarLSM plus PolarRouter use?"
contributor: "graphify"
outcome: "useful"
---

# Q: What tech stack should PolarLSM plus PolarRouter use?

## Answer

Codex and Claude Opus jointly approved a Rust-first modular monolith: Tokio plus tonic/prost and axum; OpenRaft behind owned adapters; custom Raft log, PK/version LSM, manifests, vector, residual, HNSW, and blob formats; zerocopy plus read-only memmap2; bounded Rayon compute and blocking I/O pools; benchmark-gated per-cell HNSW; Roaring filters; zstd blobs; CRC32C and BLAKE3; tracing, OpenTelemetry, and Prometheus; nextest, proptest, fuzzing, loom, Turmoil, and FaultyFs; glibc Linux containers with Helm node-pool StatefulSets; Python/NumPy and Rust clients first. C++ Highway remains a measured SIMD fallback only.

## Outcome

- Signal: useful