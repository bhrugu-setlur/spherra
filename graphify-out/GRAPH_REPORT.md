# Graph Report - .  (2026-08-04)

## Corpus Check
- 4 files · ~25,335 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 409 nodes · 688 edges · 37 communities (27 shown, 10 thin omitted)
- Extraction: 96% EXTRACTED · 4% INFERRED · 0% AMBIGUOUS · INFERRED: 26 edges (avg confidence: 0.93)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- Direct Int4 Codec
- Domain Errors and IDs
- C++ Angle Codec
- Transform Specification
- V1 Implementation Specification
- Python Accuracy Harness
- Project Guide
- Production Architecture
- Codec Foundation Plan
- C++ Benchmark Harness
- C++ Codec Tests
- Transform Contract Tests
- Task 3 Transform Oracle
- Direct Int4 Rationale
- Project Guide Query
- Tech Stack Query
- Naming Query
- Task 2 Query
- Task 3 Boundary Query
- Dependency Policy
- Quality and Security Gates
- Task 2 Query Memory
- Local Quality Gate
- Display Trait Support
- Error Trait Support
- Formatter Support
- Result Type Support
- Vector Dimension
- Radius Flags
- Quality CI Job
- Security CI Job

## God Nodes (most connected - your core abstractions)
1. `PolarLSM + PolarRouter Approved Production Design` - 19 edges
2. `evaluate()` - 15 edges
3. `Spherra project guide` - 14 edges
4. `DirectCode` - 14 edges
5. `TiledSoa32` - 14 edges
6. `AngleTables` - 13 edges
7. `Spherra v1 implementation specification` - 12 edges
8. `Spherra polar codec and format foundation implementation plan` - 12 edges
9. `QuantizerTable` - 12 edges
10. `TransformSpec` - 11 edges

## Surprising Connections (you probably didn't know these)
- `QuantizerTable::train` --implements--> `Endpoint-inclusive quantile rule`  [INFERRED]
  crates/spherra-codec/src/int4.rs → AGENTS.md
- `TiledSoa32` --implements--> `TILED_SOA_32 layout`  [INFERRED]
  crates/spherra-codec/src/tiled_soa.rs → AGENTS.md
- `DirectCode` --implements--> `Direct-int4 primary representation`  [INFERRED]
  crates/spherra-codec/src/int4.rs → AGENTS.md
- `RadiusFlags` --implements--> `Direct-int4 primary representation`  [INFERRED]
  crates/spherra-codec/src/int4.rs → AGENTS.md
- `Polar Codec Experiment README` --references--> `V1 Quality Performance and Correctness Gates`  [EXTRACTED]
  experiments/codec_bench/README.md → docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md

## Import Cycles
- None detected.

## Communities (37 total, 10 thin omitted)

### Community 0 - "Direct Int4 Codec"
Cohesion: 0.06
Nodes (52): Direct-int4 primary representation, Endpoint-inclusive quantile rule, Task 4 scalar direct-int4 and TILED_SOA_32, TILED_SOA_32 layout, canonicalize_zero, center_index, derive_identity, DirectCode (+44 more)

### Community 1 - "Domain Errors and IDs"
Cohesion: 0.07
Nodes (29): DomainError, Display, Error, Formatter, Result, ChunkId, DocumentId, Opaque Identifier Contract (+21 more)

### Community 2 - "C++ Angle Codec"
Cohesion: 0.18
Nodes (35): AngleTables, angle_count, AngleTables::AngleTables(), cosine, cosines_, set, sine, sines_ (+27 more)

### Community 3 - "Transform Specification"
Cohesion: 0.10
Nodes (18): ChaCha20Rng, Self, TransformSpec, apply_forward_round(), apply_hadamard_blocks(), apply_inverse_round(), derive_identity(), derive_round_seed() (+10 more)

### Community 4 - "V1 Implementation Specification"
Cohesion: 0.07
Nodes (27): 10. Status and document maintenance, 11. Approval record, 1. Purpose and authority, 2. Starting state, 3. Fixed implementation constraints, 4. Implementation strategy, 5. Workspace and dependency boundaries, 6.1 Codec boundary (+19 more)

### Community 5 - "Python Accuracy Harness"
Cohesion: 0.18
Nodes (18): cone_metrics(), decode_scalar(), encode_scalar(), evaluate(), fit_scalar_codec(), from_hyperspherical(), main(), make_dataset() (+10 more)

### Community 6 - "Project Guide"
Cohesion: 0.08
Nodes (24): After every project change, Approved technology stack, Before work, Completed, Current repository state, Current status and next step, Definition of done for any task, During work (+16 more)

### Community 7 - "Production Architecture"
Cohesion: 0.16
Nodes (23): V1 Quality Performance and Correctness Gates, Adaptive AoS and Tiled-SoA Layouts, Blob Storage and Snapshot Leases, Immutable Cell-Local HNSW, Certified Pruning Error Bounds, Direct Int4 Direction Code, Spherical Direction Cells, PolarLSM + PolarRouter Approved Production Design (+15 more)

### Community 8 - "Codec Foundation Plan"
Cohesion: 0.12
Nodes (16): Approval record, Plan self-review record, Planned file map, Scope and exit gate, Spherra polar codec and format foundation implementation plan, Task 1: Establish version control and the pinned Rust workspace, Task 4: Implement direct-int4 and TILED_SOA_32, Task 5: Implement PQ96x8 residual refinement (+8 more)

### Community 9 - "C++ Benchmark Harness"
Cohesion: 0.22
Nodes (18): size_t, uint8_t, vector, main(), make_angle_codes(), make_angle_tables(), make_cartesian_codes(), make_query() (+10 more)

### Community 10 - "C++ Codec Tests"
Cohesion: 0.45
Nodes (10): expect_near(), main(), test_cartesian_int4_score_maps_endpoints_to_unit_range(), test_hyperspherical_decode_matches_known_vector(), test_nibbles_round_trip(), test_packed_scores_match_unpacked_scores(), test_quantized_angle_score_uses_recursive_prefix(), test_recursive_score_matches_decoded_dot_product() (+2 more)

### Community 12 - "Task 3 Transform Oracle"
Cohesion: 0.33
Nodes (6): Task 3 Scalar Transform Status, Task 3 Transform Boundaries Query, Normalized H128, ReliableDirection Transform Boundary, Task 3 Scalar Transform Oracle, Deterministic Transform Identity

### Community 13 - "Direct Int4 Rationale"
Cohesion: 0.40
Nodes (5): Canonical BLAKE3 quantizer table identity, DirectCode, Low-even/high-odd nibble packing, Coordinate-wise quantizer training, TILED_SOA_32 coordinate-major tile storage

### Community 14 - "Project Guide Query"
Cohesion: 0.40
Nodes (4): Answer, Outcome, Q: yes do that. also set up a comprehensive AGENTS.md that acts as the project guide. AGENTS.md should be set up so that any agent that reads it has an excellent understanding of the project, whats been done, what still needs t be done, what the next step is. there should be a rule in agents.md which states that agents must keep agents.md up to date. also make sure the graphify project graph stays up to date always, Source Nodes

### Community 15 - "Tech Stack Query"
Cohesion: 0.50
Nodes (3): Answer, Outcome, Q: What tech stack should PolarLSM plus PolarRouter use?

### Community 16 - "Naming Query"
Cohesion: 0.50
Nodes (3): Answer, Q: What should I name this project? Generate prospects across mythic, infrastructure, and AI-native styles with Claude., Source Nodes

### Community 17 - "Task 2 Query"
Cohesion: 0.50
Nodes (3): Answer, Q: Start Task 2: encode domain invariants before codec code, Source Nodes

### Community 18 - "Task 3 Boundary Query"
Cohesion: 0.50
Nodes (3): Answer, Q: What approved boundaries govern Task 3's scalar transform oracle?, Source Nodes

### Community 19 - "Dependency Policy"
Cohesion: 0.83
Nodes (3): cargo_executable(), main(), normal_or_build_dependencies()

### Community 20 - "Quality and Security Gates"
Cohesion: 0.50
Nodes (4): Advisory Database Snapshot, cargo-deny, Local Quality Gate, Security Advisories

### Community 21 - "Task 2 Query Memory"
Cohesion: 0.67
Nodes (3): Start Task 2 Query Memory, Task 1 Pinned Rust Workspace, Task 2 Scope Rationale

## Knowledge Gaps
- **87 isolated node(s):** `seconds`, `vectors_per_second`, `checksum`, `cosines_`, `sines_` (+82 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **10 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `ReliableDirection` connect `Domain Errors and IDs` to `Transform Contract Tests`, `Transform Specification`?**
  _High betweenness centrality (0.076) - this node is a cross-community bridge._
- **Why does `transform()` connect `Transform Specification` to `Domain Errors and IDs`?**
  _High betweenness centrality (0.071) - this node is a cross-community bridge._
- **What connects `seconds`, `vectors_per_second`, `checksum` to the rest of the system?**
  _87 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Direct Int4 Codec` be split into smaller, more focused modules?**
  _Cohesion score 0.060455486542443065 - nodes in this community are weakly interconnected._
- **Should `Domain Errors and IDs` be split into smaller, more focused modules?**
  _Cohesion score 0.0673758865248227 - nodes in this community are weakly interconnected._
- **Should `Transform Specification` be split into smaller, more focused modules?**
  _Cohesion score 0.09682539682539683 - nodes in this community are weakly interconnected._
- **Should `V1 Implementation Specification` be split into smaller, more focused modules?**
  _Cohesion score 0.07407407407407407 - nodes in this community are weakly interconnected._