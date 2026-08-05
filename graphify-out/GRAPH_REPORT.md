# Graph Report - rust-workspace-bootstrap  (2026-08-04)

## Corpus Check
- 31 files · ~21,974 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 290 nodes · 479 edges · 29 communities (24 shown, 5 thin omitted)
- Extraction: 96% EXTRACTED · 4% INFERRED · 0% AMBIGUOUS · INFERRED: 19 edges (avg confidence: 0.94)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `ffc34ab2`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- C++ Angle Codec
- Task 2 Domain Contracts
- V1 Implementation Specification
- Domain Errors and IDs
- Project Guide and Workflow
- Vector Validation and Direction
- Production Architecture
- C++ Benchmark Harness
- Python Accuracy Harness
- C++ Codec Tests
- Project Documentation
- Python Accuracy Tests
- Project Guide Query Memory
- Technology Stack Query
- Project Naming Query
- Task 2 Query Memory
- Dependency Policy Script
- Rust Security Tooling
- Local CI Script
- Vector Dimension Contract
- Quality CI Workflow
- Security CI Workflow

## God Nodes (most connected - your core abstractions)
1. `PolarLSM + PolarRouter Approved Production Design` - 19 edges
2. `evaluate()` - 15 edges
3. `Spherra project guide` - 14 edges
4. `Spherra polar codec and format foundation implementation plan` - 14 edges
5. `AngleTables` - 13 edges
6. `Spherra v1 implementation specification` - 12 edges
7. `Task 2: Encode domain invariants before codec code` - 10 edges
8. `DomainError` - 9 edges
9. `ValidatedVector` - 9 edges
10. `main()` - 9 edges

## Surprising Connections (you probably didn't know these)
- `Polar Codec Experiment README` --references--> `V1 Quality Performance and Correctness Gates`  [EXTRACTED]
  experiments/codec_bench/README.md → docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md
- `Polar Codec Experiment README` --references--> `Adaptive AoS and Tiled-SoA Layouts`  [INFERRED]
  experiments/codec_bench/README.md → docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md
- `Polar Codec Experiment README` --references--> `Recursive Hyperspherical Angles`  [EXTRACTED]
  experiments/codec_bench/README.md → docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md
- `Task 2 Domain Invariants Complete` --references--> `Task 2: Encode domain invariants before codec code`  [EXTRACTED]
  AGENTS.md → docs/superpowers/plans/2026-08-04-polar-codec-format-foundation.md
- `Task 1 Pinned Rust Workspace` --rationale_for--> `Task 2: Encode domain invariants before codec code`  [EXTRACTED]
  graphify-out/memory/query_20260805_000124_start_task_2__encode_domain_invariants_before_code.md → docs/superpowers/plans/2026-08-04-polar-codec-format-foundation.md

## Import Cycles
- None detected.

## Communities (29 total, 5 thin omitted)

### Community 0 - "C++ Angle Codec"
Cohesion: 0.18
Nodes (35): AngleTables, angle_count, AngleTables::AngleTables(), cosine, cosines_, set, sine, sines_ (+27 more)

### Community 1 - "Task 2 Domain Contracts"
Cohesion: 0.08
Nodes (29): Frozen Vector Representation and Scoring, Task 2 Domain Invariants Complete, Task 3 Exact Scalar Transform Next, Approval record, 768-Dimensional Vector Invariant, Distinct ChunkId and DocumentId, Domain Contract Tests, FP16-Safe Original Radius (+21 more)

### Community 2 - "V1 Implementation Specification"
Cohesion: 0.07
Nodes (27): 10. Status and document maintenance, 11. Approval record, 1. Purpose and authority, 2. Starting state, 3. Fixed implementation constraints, 4. Implementation strategy, 5. Workspace and dependency boundaries, 6.1 Codec boundary (+19 more)

### Community 3 - "Domain Errors and IDs"
Cohesion: 0.10
Nodes (14): DomainError, Result, ChunkId, DocumentId, Opaque Identifier Contract, Self, 48-bit Put Sequence Contract, PutSeq (+6 more)

### Community 4 - "Project Guide and Workflow"
Cohesion: 0.08
Nodes (24): After every project change, Approved technology stack, Before work, Completed, Current repository state, Current status and next step, Definition of done for any task, During work (+16 more)

### Community 5 - "Vector Validation and Direction"
Cohesion: 0.16
Nodes (15): Direction Reliability Contract, FP16 Radius Contract, FP64 Normalization Contract, ReliableDirection, Result, Self, ValidatedVector, Vector Validation Contract (+7 more)

### Community 6 - "Production Architecture"
Cohesion: 0.16
Nodes (23): V1 Quality Performance and Correctness Gates, Adaptive AoS and Tiled-SoA Layouts, Blob Storage and Snapshot Leases, Immutable Cell-Local HNSW, Certified Pruning Error Bounds, Direct Int4 Direction Code, Spherical Direction Cells, PolarLSM + PolarRouter Approved Production Design (+15 more)

### Community 7 - "C++ Benchmark Harness"
Cohesion: 0.22
Nodes (18): size_t, uint8_t, vector, main(), make_angle_codes(), make_angle_tables(), make_cartesian_codes(), make_query() (+10 more)

### Community 8 - "Python Accuracy Harness"
Cohesion: 0.31
Nodes (17): cone_metrics(), decode_scalar(), encode_scalar(), evaluate(), fit_scalar_codec(), from_hyperspherical(), main(), make_dataset() (+9 more)

### Community 9 - "C++ Codec Tests"
Cohesion: 0.45
Nodes (10): expect_near(), main(), test_cartesian_int4_score_maps_endpoints_to_unit_range(), test_hyperspherical_decode_matches_known_vector(), test_nibbles_round_trip(), test_packed_scores_match_unpacked_scores(), test_quantized_angle_score_uses_recursive_prefix(), test_recursive_score_matches_decoded_dot_product() (+2 more)

### Community 10 - "Project Documentation"
Cohesion: 0.36
Nodes (4): Architecture at a glance, Current target, Spherra, Start here

### Community 12 - "Project Guide Query Memory"
Cohesion: 0.40
Nodes (4): Answer, Outcome, Q: yes do that. also set up a comprehensive AGENTS.md that acts as the project guide. AGENTS.md should be set up so that any agent that reads it has an excellent understanding of the project, whats been done, what still needs t be done, what the next step is. there should be a rule in agents.md which states that agents must keep agents.md up to date. also make sure the graphify project graph stays up to date always, Source Nodes

### Community 13 - "Technology Stack Query"
Cohesion: 0.50
Nodes (3): Answer, Outcome, Q: What tech stack should PolarLSM plus PolarRouter use?

### Community 14 - "Project Naming Query"
Cohesion: 0.50
Nodes (3): Answer, Q: What should I name this project? Generate prospects across mythic, infrastructure, and AI-native styles with Claude., Source Nodes

### Community 15 - "Task 2 Query Memory"
Cohesion: 0.50
Nodes (3): Answer, Q: Start Task 2: encode domain invariants before codec code, Source Nodes

### Community 16 - "Dependency Policy Script"
Cohesion: 0.83
Nodes (3): cargo_executable(), main(), normal_or_build_dependencies()

### Community 17 - "Rust Security Tooling"
Cohesion: 0.50
Nodes (4): Advisory Database Snapshot, cargo-deny, Local Quality Gate, Security Advisories

## Knowledge Gaps
- **82 isolated node(s):** `seconds`, `vectors_per_second`, `checksum`, `cosines_`, `sines_` (+77 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **5 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Spherra polar codec and format foundation implementation plan` connect `Task 2 Domain Contracts` to `Project Documentation`?**
  _High betweenness centrality (0.046) - this node is a cross-community bridge._
- **Why does `Spherra v1 implementation specification` connect `V1 Implementation Specification` to `Project Documentation`?**
  _High betweenness centrality (0.044) - this node is a cross-community bridge._
- **Why does `Spherra project guide` connect `Project Guide and Workflow` to `Project Documentation`?**
  _High betweenness centrality (0.041) - this node is a cross-community bridge._
- **What connects `seconds`, `vectors_per_second`, `checksum` to the rest of the system?**
  _82 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Task 2 Domain Contracts` be split into smaller, more focused modules?**
  _Cohesion score 0.0812807881773399 - nodes in this community are weakly interconnected._
- **Should `V1 Implementation Specification` be split into smaller, more focused modules?**
  _Cohesion score 0.07407407407407407 - nodes in this community are weakly interconnected._
- **Should `Domain Errors and IDs` be split into smaller, more focused modules?**
  _Cohesion score 0.10333333333333333 - nodes in this community are weakly interconnected._