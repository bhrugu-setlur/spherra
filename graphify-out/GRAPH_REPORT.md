# Graph Report - spherra  (2026-08-04)

## Corpus Check
- 43 files · ~24,711 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 367 nodes · 575 edges · 37 communities (31 shown, 6 thin omitted)
- Extraction: 96% EXTRACTED · 4% INFERRED · 0% AMBIGUOUS · INFERRED: 22 edges (avg confidence: 0.92)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `de9601ae`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- C++ Angle Codec
- transform.rs
- Spherra project guide
- Spherra polar codec and format foundation implementation plan
- Spherra v1 implementation specification
- PutSeq
- .new_with_min_norm_epsilon
- PolarLSM + PolarRouter Approved Production Design
- bench.cpp
- accuracy.py
- codec_test.cpp
- transform_contract.rs
- AccuracyTest
- Task 3 Scalar Transform Oracle
- Q: yes do that. also set up a comprehensive AGENTS.md that acts as the project guide. AGENTS.md should be set up so that any agent that reads it has an excellent understanding of the project, whats been done, what still needs t be done, what the next step is. there should be a rule in agents.md which states that agents must keep agents.md up to date. also make sure the graphify project graph stays up to date always
- Q: What tech stack should PolarLSM plus PolarRouter use?
- Q: What should I name this project? Generate prospects across mythic, infrastructure, and AI-native styles with Claude.
- Q: Start Task 2: encode domain invariants before codec code
- Q: What approved boundaries govern Task 3's scalar transform oracle?
- check_dependency_policy.py
- cargo-deny
- ci.sh
- Vector Dimension
- Quality CI Job
- Security CI Job
- Q: so right now we have a design spec and an implementation spec right?
- Q: explain in simple terms what spherra should be
- Q: will spherra store its vectors in polar form?
- Q: is the direction a single value or a collection of values?
- Q: how is this different than 767 connected angles? i thought one of the reasons why storing in polar is more efficient is because angles are a fixed range
- Q: ok so if i understand this correctly: we take a vector and split it into its magnitude and its direction. the magnitude is a 16 bit number and the direction is stored as a unit vector. now since we are embedding into 768 dimensions, that unit vector that determines direction also must have 768 components. but since unit vectors are fixed at a magnitude of 1, all of those components that make up the unit vector must fall between -1 and 1. this essentially mimics the fixed range of connected angles, but decoding the unit vector components is easier so searching is faster with this approach. tell me if im wrong

## God Nodes (most connected - your core abstractions)
1. `PolarLSM + PolarRouter Approved Production Design` - 19 edges
2. `evaluate()` - 15 edges
3. `Spherra project guide` - 14 edges
4. `Spherra polar codec and format foundation implementation plan` - 14 edges
5. `AngleTables` - 13 edges
6. `Spherra v1 implementation specification` - 12 edges
7. `TransformSpec` - 11 edges
8. `Task 2: Encode domain invariants before codec code` - 10 edges
9. `DomainError` - 9 edges
10. `ValidatedVector` - 9 edges

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

## Hyperedges (group relationships)
- **Task 3 Scalar Transform Contract** — plans_2026_08_04_polar_codec_format_foundation_task_3_scalar_transform_oracle, plans_2026_08_04_polar_codec_format_foundation_normalized_h128, plans_2026_08_04_polar_codec_format_foundation_transform_identity, plans_2026_08_04_polar_codec_format_foundation_reliable_direction_boundary [EXTRACTED 1.00]

## Communities (37 total, 6 thin omitted)

### Community 0 - "C++ Angle Codec"
Cohesion: 0.18
Nodes (35): AngleTables, angle_count, AngleTables::AngleTables(), cosine, cosines_, set, sine, sines_ (+27 more)

### Community 1 - "transform.rs"
Cohesion: 0.10
Nodes (18): ChaCha20Rng, Self, TransformSpec, apply_forward_round(), apply_hadamard_blocks(), apply_inverse_round(), derive_identity(), derive_round_seed() (+10 more)

### Community 2 - "Spherra project guide"
Cohesion: 0.07
Nodes (28): After every project change, Approved technology stack, Before work, Completed, Current repository state, Current status and next step, Definition of done for any task, During work (+20 more)

### Community 3 - "Spherra polar codec and format foundation implementation plan"
Cohesion: 0.08
Nodes (29): Frozen Vector Representation and Scoring, Task 2 Domain Invariants Complete, Task 3 Exact Scalar Transform Next, Approval record, 768-Dimensional Vector Invariant, Distinct ChunkId and DocumentId, Domain Contract Tests, FP16-Safe Original Radius (+21 more)

### Community 4 - "Spherra v1 implementation specification"
Cohesion: 0.07
Nodes (27): 10. Status and document maintenance, 11. Approval record, 1. Purpose and authority, 2. Starting state, 3. Fixed implementation constraints, 4. Implementation strategy, 5. Workspace and dependency boundaries, 6.1 Codec boundary (+19 more)

### Community 5 - "PutSeq"
Cohesion: 0.15
Nodes (9): ChunkId, DocumentId, Opaque Identifier Contract, Self, 48-bit Put Sequence Contract, PutSeq, Result, Self (+1 more)

### Community 6 - ".new_with_min_norm_epsilon"
Cohesion: 0.11
Nodes (20): DomainError, Result, Direction Reliability Contract, FP16 Radius Contract, FP64 Normalization Contract, ReliableDirection, Result, Self (+12 more)

### Community 7 - "PolarLSM + PolarRouter Approved Production Design"
Cohesion: 0.16
Nodes (23): V1 Quality Performance and Correctness Gates, Adaptive AoS and Tiled-SoA Layouts, Blob Storage and Snapshot Leases, Immutable Cell-Local HNSW, Certified Pruning Error Bounds, Direct Int4 Direction Code, Spherical Direction Cells, PolarLSM + PolarRouter Approved Production Design (+15 more)

### Community 8 - "bench.cpp"
Cohesion: 0.22
Nodes (18): size_t, uint8_t, vector, main(), make_angle_codes(), make_angle_tables(), make_cartesian_codes(), make_query() (+10 more)

### Community 9 - "accuracy.py"
Cohesion: 0.31
Nodes (17): cone_metrics(), decode_scalar(), encode_scalar(), evaluate(), fit_scalar_codec(), from_hyperspherical(), main(), make_dataset() (+9 more)

### Community 10 - "codec_test.cpp"
Cohesion: 0.45
Nodes (10): expect_near(), main(), test_cartesian_int4_score_maps_endpoints_to_unit_range(), test_hyperspherical_decode_matches_known_vector(), test_nibbles_round_trip(), test_packed_scores_match_unpacked_scores(), test_quantized_angle_score_uses_recursive_prefix(), test_recursive_score_matches_decoded_dot_product() (+2 more)

### Community 13 - "Task 3 Scalar Transform Oracle"
Cohesion: 0.33
Nodes (6): Task 3 Scalar Transform Status, Task 3 Transform Boundaries Query, Normalized H128, ReliableDirection Transform Boundary, Task 3 Scalar Transform Oracle, Deterministic Transform Identity

### Community 14 - "Q: yes do that. also set up a comprehensive AGENTS.md that acts as the project guide. AGENTS.md should be set up so that any agent that reads it has an excellent understanding of the project, whats been done, what still needs t be done, what the next step is. there should be a rule in agents.md which states that agents must keep agents.md up to date. also make sure the graphify project graph stays up to date always"
Cohesion: 0.40
Nodes (4): Answer, Outcome, Q: yes do that. also set up a comprehensive AGENTS.md that acts as the project guide. AGENTS.md should be set up so that any agent that reads it has an excellent understanding of the project, whats been done, what still needs t be done, what the next step is. there should be a rule in agents.md which states that agents must keep agents.md up to date. also make sure the graphify project graph stays up to date always, Source Nodes

### Community 15 - "Q: What tech stack should PolarLSM plus PolarRouter use?"
Cohesion: 0.50
Nodes (3): Answer, Outcome, Q: What tech stack should PolarLSM plus PolarRouter use?

### Community 16 - "Q: What should I name this project? Generate prospects across mythic, infrastructure, and AI-native styles with Claude."
Cohesion: 0.50
Nodes (3): Answer, Q: What should I name this project? Generate prospects across mythic, infrastructure, and AI-native styles with Claude., Source Nodes

### Community 17 - "Q: Start Task 2: encode domain invariants before codec code"
Cohesion: 0.50
Nodes (3): Answer, Q: Start Task 2: encode domain invariants before codec code, Source Nodes

### Community 18 - "Q: What approved boundaries govern Task 3's scalar transform oracle?"
Cohesion: 0.50
Nodes (3): Answer, Q: What approved boundaries govern Task 3's scalar transform oracle?, Source Nodes

### Community 19 - "check_dependency_policy.py"
Cohesion: 0.83
Nodes (3): cargo_executable(), main(), normal_or_build_dependencies()

### Community 20 - "cargo-deny"
Cohesion: 0.50
Nodes (4): Advisory Database Snapshot, cargo-deny, Local Quality Gate, Security Advisories

### Community 31 - "Q: so right now we have a design spec and an implementation spec right?"
Cohesion: 0.50
Nodes (3): Answer, Q: so right now we have a design spec and an implementation spec right?, Source Nodes

### Community 32 - "Q: explain in simple terms what spherra should be"
Cohesion: 0.50
Nodes (3): Answer, Q: explain in simple terms what spherra should be, Source Nodes

### Community 33 - "Q: will spherra store its vectors in polar form?"
Cohesion: 0.50
Nodes (3): Answer, Q: will spherra store its vectors in polar form?, Source Nodes

### Community 34 - "Q: is the direction a single value or a collection of values?"
Cohesion: 0.50
Nodes (3): Answer, Q: is the direction a single value or a collection of values?, Source Nodes

### Community 35 - "Q: how is this different than 767 connected angles? i thought one of the reasons why storing in polar is more efficient is because angles are a fixed range"
Cohesion: 0.50
Nodes (3): Answer, Q: how is this different than 767 connected angles? i thought one of the reasons why storing in polar is more efficient is because angles are a fixed range, Source Nodes

### Community 36 - "Q: ok so if i understand this correctly: we take a vector and split it into its magnitude and its direction. the magnitude is a 16 bit number and the direction is stored as a unit vector. now since we are embedding into 768 dimensions, that unit vector that determines direction also must have 768 components. but since unit vectors are fixed at a magnitude of 1, all of those components that make up the unit vector must fall between -1 and 1. this essentially mimics the fixed range of connected angles, but decoding the unit vector components is easier so searching is faster with this approach. tell me if im wrong"
Cohesion: 0.50
Nodes (3): Answer, Q: ok so if i understand this correctly: we take a vector and split it into its magnitude and its direction. the magnitude is a 16 bit number and the direction is stored as a unit vector. now since we are embedding into 768 dimensions, that unit vector that determines direction also must have 768 components. but since unit vectors are fixed at a magnitude of 1, all of those components that make up the unit vector must fall between -1 and 1. this essentially mimics the fixed range of connected angles, but decoding the unit vector components is easier so searching is faster with this approach. tell me if im wrong, Source Nodes

## Knowledge Gaps
- **98 isolated node(s):** `seconds`, `vectors_per_second`, `checksum`, `cosines_`, `sines_` (+93 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **6 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `ReliableDirection` connect `.new_with_min_norm_epsilon` to `transform.rs`, `transform_contract.rs`?**
  _High betweenness centrality (0.036) - this node is a cross-community bridge._
- **Why does `transform()` connect `transform.rs` to `.new_with_min_norm_epsilon`?**
  _High betweenness centrality (0.030) - this node is a cross-community bridge._
- **Why does `Spherra polar codec and format foundation implementation plan` connect `Spherra polar codec and format foundation implementation plan` to `Spherra project guide`?**
  _High betweenness centrality (0.028) - this node is a cross-community bridge._
- **What connects `seconds`, `vectors_per_second`, `checksum` to the rest of the system?**
  _98 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `transform.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.09682539682539683 - nodes in this community are weakly interconnected._
- **Should `Spherra project guide` be split into smaller, more focused modules?**
  _Cohesion score 0.06854838709677419 - nodes in this community are weakly interconnected._
- **Should `Spherra polar codec and format foundation implementation plan` be split into smaller, more focused modules?**
  _Cohesion score 0.0812807881773399 - nodes in this community are weakly interconnected._