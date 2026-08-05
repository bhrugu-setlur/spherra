# Graph Report - .  (2026-08-04)

## Corpus Check
- 6 files · ~25,236 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 410 nodes · 665 edges · 39 communities (29 shown, 10 thin omitted)
- Extraction: 97% EXTRACTED · 3% INFERRED · 0% AMBIGUOUS · INFERRED: 22 edges (avg confidence: 0.92)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- Domain Errors and IDs
- C++ Angle Codec
- Transform Specification
- Direct Int4 Codec
- V1 Implementation Specification
- Python Accuracy Harness
- Project Guide
- Production Architecture
- Codec Foundation Plan
- C++ Benchmark Harness
- Tiled SoA Layout
- Codec Contract Tests
- Task 4 Direct Int4 Layout
- C++ Codec Tests
- Project Guide Query
- Task 3 Transform Query
- Naming Query
- Task 2 Query
- Task 3 Boundary Query
- Task 2 Query Memory
- Task 3 Query Memory
- Dependency Policy
- Local Quality Gate
- Task 2 Scope Rationale
- Format Crate Stub
- Testkit Crate Stub
- Bound Fuzz Target
- Format Fuzz Target
- Quality CI Job
- Security CI Job
- Codec Contract Support
- Quality CI Support
- Security CI Support

## God Nodes (most connected - your core abstractions)
1. `PolarLSM + PolarRouter Approved Production Design` - 19 edges
2. `evaluate()` - 15 edges
3. `Spherra project guide` - 14 edges
4. `AngleTables` - 13 edges
5. `TiledSoa32` - 13 edges
6. `Spherra v1 implementation specification` - 12 edges
7. `Spherra polar codec and format foundation implementation plan` - 12 edges
8. `DirectCode` - 12 edges
9. `QuantizerTable` - 11 edges
10. `TransformSpec` - 10 edges

## Surprising Connections (you probably didn't know these)
- `Polar Codec Experiment README` --references--> `V1 Quality Performance and Correctness Gates`  [EXTRACTED]
  experiments/codec_bench/README.md → docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md
- `Polar Codec Experiment README` --references--> `Adaptive AoS and Tiled-SoA Layouts`  [INFERRED]
  experiments/codec_bench/README.md → docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md
- `Polar Codec Experiment README` --references--> `Recursive Hyperspherical Angles`  [EXTRACTED]
  experiments/codec_bench/README.md → docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md
- `RadiusFlags` --conceptually_related_to--> `Provisional scalar direct-int4 primary representation`  [INFERRED]
  docs/superpowers/plans/2026-08-04-polar-codec-format-foundation.md → AGENTS.md
- `Polar Codec Experiment README` --references--> `Direct Int4 Direction Code`  [EXTRACTED]
  experiments/codec_bench/README.md → docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md

## Import Cycles
- None detected.

## Communities (39 total, 10 thin omitted)

### Community 0 - "Domain Errors and IDs"
Cohesion: 0.07
Nodes (29): DomainError, Display, Error, Formatter, Result, ChunkId, DocumentId, Opaque Identifier Contract (+21 more)

### Community 1 - "C++ Angle Codec"
Cohesion: 0.18
Nodes (35): AngleTables, angle_count, AngleTables::AngleTables(), cosine, cosines_, set, sine, sines_ (+27 more)

### Community 2 - "Transform Specification"
Cohesion: 0.10
Nodes (18): ChaCha20Rng, Self, TransformSpec, apply_forward_round(), apply_hadamard_blocks(), apply_inverse_round(), derive_identity(), derive_round_seed() (+10 more)

### Community 3 - "Direct Int4 Codec"
Cohesion: 0.12
Nodes (14): canonicalize_zero(), center_index(), derive_identity(), DirectCode, DirectCodeError, quantile_rank(), QuantizerTable, RadiusFlags (+6 more)

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

### Community 10 - "Tiled SoA Layout"
Cohesion: 0.20
Nodes (4): Self, Vec, TiledSoa32, Option

### Community 11 - "Codec Contract Tests"
Cohesion: 0.23
Nodes (12): affine_calibration(), affine_table(), coordinate_rank_calibration(), deterministic_nibbles(), direct_code(), direct_code_packs_endpoints_and_deterministic_random_nibbles(), quantizer_encodes_decodes_and_scores_using_its_trained_table(), Vec (+4 more)

### Community 12 - "Task 4 Direct Int4 Layout"
Cohesion: 0.17
Nodes (13): Canonical BLAKE3 quantizer table identity, DirectCode, Low-even/high-odd nibble packing, Coordinate-wise quantizer training, RadiusFlags, TILED_SOA_32 coordinate-major tile storage, Direct-int4/layout contract tests, Endpoint-inclusive quantile rank rule (+5 more)

### Community 13 - "C++ Codec Tests"
Cohesion: 0.45
Nodes (10): expect_near(), main(), test_cartesian_int4_score_maps_endpoints_to_unit_range(), test_hyperspherical_decode_matches_known_vector(), test_nibbles_round_trip(), test_packed_scores_match_unpacked_scores(), test_quantized_angle_score_uses_recursive_prefix(), test_recursive_score_matches_decoded_dot_product() (+2 more)

### Community 15 - "Task 3 Transform Query"
Cohesion: 0.33
Nodes (6): Task 3 Scalar Transform Status, Task 3 Transform Boundaries Query, Normalized H128, ReliableDirection Transform Boundary, Task 3 Scalar Transform Oracle, Deterministic Transform Identity

### Community 16 - "Naming Query"
Cohesion: 0.40
Nodes (4): Answer, Outcome, Q: yes do that. also set up a comprehensive AGENTS.md that acts as the project guide. AGENTS.md should be set up so that any agent that reads it has an excellent understanding of the project, whats been done, what still needs t be done, what the next step is. there should be a rule in agents.md which states that agents must keep agents.md up to date. also make sure the graphify project graph stays up to date always, Source Nodes

### Community 17 - "Task 2 Query"
Cohesion: 0.50
Nodes (3): Answer, Outcome, Q: What tech stack should PolarLSM plus PolarRouter use?

### Community 18 - "Task 3 Boundary Query"
Cohesion: 0.50
Nodes (3): Answer, Q: What should I name this project? Generate prospects across mythic, infrastructure, and AI-native styles with Claude., Source Nodes

### Community 19 - "Task 2 Query Memory"
Cohesion: 0.50
Nodes (3): Answer, Q: Start Task 2: encode domain invariants before codec code, Source Nodes

### Community 20 - "Task 3 Query Memory"
Cohesion: 0.50
Nodes (3): Answer, Q: What approved boundaries govern Task 3's scalar transform oracle?, Source Nodes

### Community 21 - "Dependency Policy"
Cohesion: 0.83
Nodes (3): cargo_executable(), main(), normal_or_build_dependencies()

### Community 22 - "Local Quality Gate"
Cohesion: 0.50
Nodes (4): Advisory Database Snapshot, cargo-deny, Local Quality Gate, Security Advisories

### Community 23 - "Task 2 Scope Rationale"
Cohesion: 0.67
Nodes (3): Start Task 2 Query Memory, Task 1 Pinned Rust Workspace, Task 2 Scope Rationale

## Knowledge Gaps
- **89 isolated node(s):** `seconds`, `vectors_per_second`, `checksum`, `cosines_`, `sines_` (+84 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **10 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `ReliableDirection` connect `Domain Errors and IDs` to `Transform Specification`, `Project Guide Query`?**
  _High betweenness centrality (0.071) - this node is a cross-community bridge._
- **Why does `transform()` connect `Transform Specification` to `Domain Errors and IDs`?**
  _High betweenness centrality (0.066) - this node is a cross-community bridge._
- **What connects `seconds`, `vectors_per_second`, `checksum` to the rest of the system?**
  _89 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Domain Errors and IDs` be split into smaller, more focused modules?**
  _Cohesion score 0.0673758865248227 - nodes in this community are weakly interconnected._
- **Should `Transform Specification` be split into smaller, more focused modules?**
  _Cohesion score 0.09915966386554621 - nodes in this community are weakly interconnected._
- **Should `Direct Int4 Codec` be split into smaller, more focused modules?**
  _Cohesion score 0.11553030303030302 - nodes in this community are weakly interconnected._
- **Should `V1 Implementation Specification` be split into smaller, more focused modules?**
  _Cohesion score 0.07407407407407407 - nodes in this community are weakly interconnected._