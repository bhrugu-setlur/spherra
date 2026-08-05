# Graph Report - /Users/bhrugusetlur/dev/spherra-worktrees/rust-workspace-bootstrap  (2026-08-04)

## Corpus Check
- 41 files · ~25,503 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 289 nodes · 523 edges · 37 communities (24 shown, 13 thin omitted)
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 7 edges (avg confidence: 0.86)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- C++ Angle Codec
- Direct Int4 Codec
- Domain Vector Contracts
- Transform Specification
- C++ Benchmark Driver
- Codec Contract Tests
- Python Accuracy Evaluation
- Tiled SoA Layout
- C++ Codec Tests
- Transform Contract Tests
- Scalar Hadamard Tests
- Domain Identifier Types
- Python Accuracy Tests
- Core System Architecture
- Codec Layout Alternatives
- Foundation Plan Tasks
- Dependency Policy
- M1 Codec Foundation
- Quality Gate
- CI Script
- Graphify Maintenance
- PQ Residual
- Two Round Transform
- Certified Scoring
- Rust Modular Monolith
- Project Guide
- Product Naming
- Task Two Contracts
- Task Three Transform
- Spherra Product

## God Nodes (most connected - your core abstractions)
1. `evaluate()` - 15 edges
2. `TiledSoa32` - 13 edges
3. `AngleTables` - 13 edges
4. `DirectCode` - 12 edges
5. `QuantizerTable` - 12 edges
6. `TransformSpec` - 11 edges
7. `TransformedDirection` - 10 edges
8. `DomainError` - 9 edges
9. `ValidatedVector` - 9 edges
10. `main()` - 9 edges

## Surprising Connections (you probably didn't know these)
- `PolarLSM` --semantically_similar_to--> `PolarLSM architecture`  [INFERRED] [semantically similar]
  AGENTS.md → docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md
- `PolarRouter` --semantically_similar_to--> `PolarRouter architecture`  [INFERRED] [semantically similar]
  AGENTS.md → docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md
- `Direct int4 transformed direction` --semantically_similar_to--> `Direct int4 experiment`  [INFERRED] [semantically similar]
  AGENTS.md → experiments/codec_bench/README.md
- `transformed_query()` --calls--> `transform()`  [INFERRED]
  crates/spherra-codec/tests/codec_contract.rs → crates/spherra-codec/src/transform.rs
- `apply_hadamard_blocks()` --calls--> `hadamard_128()`  [INFERRED]
  crates/spherra-codec/src/transform.rs → crates/spherra-simd/src/scalar.rs

## Import Cycles
- None detected.

## Hyperedges (group relationships)
- **M1 codec and format foundation** — docs_superpowers_specs_2026_08_04_polar_v1_implementation_spec_m1, docs_superpowers_specs_2026_08_04_polar_v1_implementation_spec_spherra_codec, docs_superpowers_specs_2026_08_04_polar_v1_implementation_spec_spherra_format [EXTRACTED 1.00]

## Communities (37 total, 13 thin omitted)

### Community 0 - "C++ Angle Codec"
Cohesion: 0.18
Nodes (35): AngleTables, angle_count, AngleTables::AngleTables(), cosine, cosines_, set, sine, sines_ (+27 more)

### Community 1 - "Direct Int4 Codec"
Cohesion: 0.11
Nodes (15): canonicalize_zero(), center_index(), derive_identity(), DirectCode, DirectCodeError, quantile_rank(), QuantizerTable, RadiusFlags (+7 more)

### Community 2 - "Domain Vector Contracts"
Cohesion: 0.10
Nodes (15): DomainError, Display, Error, Formatter, Result, ReliableDirection, Option, Result (+7 more)

### Community 3 - "Transform Specification"
Cohesion: 0.13
Nodes (15): ChaCha20Rng, Self, TransformSpec, apply_forward_round(), apply_hadamard_blocks(), apply_inverse_round(), derive_identity(), derive_round_seed() (+7 more)

### Community 4 - "C++ Benchmark Driver"
Cohesion: 0.22
Nodes (18): size_t, uint8_t, vector, main(), make_angle_codes(), make_angle_tables(), make_cartesian_codes(), make_query() (+10 more)

### Community 5 - "Codec Contract Tests"
Cohesion: 0.20
Nodes (15): affine_calibration(), affine_table(), coordinate_rank_calibration(), deterministic_nibbles(), direct_code(), direct_code_packs_endpoints_and_deterministic_random_nibbles(), directional_codec_paths_start_with_validated_transformed_direction(), quantizer_encodes_decodes_and_scores_using_its_trained_table() (+7 more)

### Community 6 - "Python Accuracy Evaluation"
Cohesion: 0.31
Nodes (17): cone_metrics(), decode_scalar(), encode_scalar(), evaluate(), fit_scalar_codec(), from_hyperspherical(), main(), make_dataset() (+9 more)

### Community 7 - "Tiled SoA Layout"
Cohesion: 0.20
Nodes (4): Option, Self, Vec, TiledSoa32

### Community 8 - "C++ Codec Tests"
Cohesion: 0.45
Nodes (10): expect_near(), main(), test_cartesian_int4_score_maps_endpoints_to_unit_range(), test_hyperspherical_decode_matches_known_vector(), test_nibbles_round_trip(), test_packed_scores_match_unpacked_scores(), test_quantized_angle_score_uses_recursive_prefix(), test_recursive_score_matches_decoded_dot_product() (+2 more)

### Community 10 - "Scalar Hadamard Tests"
Cohesion: 0.32
Nodes (3): hadamard_128(), normalized_hadamard_128_preserves_dot_product(), normalized_hadamard_128_preserves_norm_and_is_its_own_inverse()

### Community 11 - "Domain Identifier Types"
Cohesion: 0.33
Nodes (3): ChunkId, DocumentId, Self

### Community 14 - "Core System Architecture"
Cohesion: 0.50
Nodes (5): PolarLSM, PolarRouter, Spherra, PolarLSM architecture, PolarRouter architecture

### Community 15 - "Codec Layout Alternatives"
Cohesion: 0.50
Nodes (4): Direct int4 transformed direction, TILED_SOA_32, Direct int4 experiment, Recursive hyperspherical angles

### Community 16 - "Foundation Plan Tasks"
Cohesion: 0.50
Nodes (4): Task 2: Encode domain invariants before codec code, Task 3: Implement the exact scalar transform oracle, Task 4: Direct-int4 and TILED_SOA_32, Task 5: PQ96x8 residual refinement

### Community 17 - "Dependency Policy"
Cohesion: 0.83
Nodes (3): cargo_executable(), main(), normal_or_build_dependencies()

### Community 18 - "M1 Codec Foundation"
Cohesion: 1.00
Nodes (3): M1 scalar codec and durable-format foundation, spherra-codec, spherra-format

### Community 19 - "Quality Gate"
Cohesion: 0.67
Nodes (3): CI, Local quality gate, Security advisories

## Knowledge Gaps
- **18 isolated node(s):** `seconds`, `vectors_per_second`, `checksum`, `cosines_`, `sines_` (+13 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **13 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `transform()` connect `Transform Specification` to `Domain Vector Contracts`, `Codec Contract Tests`?**
  _High betweenness centrality (0.117) - this node is a cross-community bridge._
- **Why does `ReliableDirection` connect `Domain Vector Contracts` to `Transform Contract Tests`, `Transform Specification`?**
  _High betweenness centrality (0.116) - this node is a cross-community bridge._
- **Why does `TransformedDirection` connect `Transform Specification` to `Direct Int4 Codec`, `Codec Contract Tests`, `Tiled SoA Layout`?**
  _High betweenness centrality (0.089) - this node is a cross-community bridge._
- **What connects `seconds`, `vectors_per_second`, `checksum` to the rest of the system?**
  _18 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Direct Int4 Codec` be split into smaller, more focused modules?**
  _Cohesion score 0.11229946524064172 - nodes in this community are weakly interconnected._
- **Should `Domain Vector Contracts` be split into smaller, more focused modules?**
  _Cohesion score 0.09852216748768473 - nodes in this community are weakly interconnected._
- **Should `Transform Specification` be split into smaller, more focused modules?**
  _Cohesion score 0.13227513227513227 - nodes in this community are weakly interconnected._