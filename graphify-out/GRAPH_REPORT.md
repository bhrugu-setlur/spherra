# Graph Report - spherra  (2026-08-04)

## Corpus Check
- 15 files · ~20,276 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 200 nodes · 368 edges · 12 communities (11 shown, 1 thin omitted)
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 2 edges (avg confidence: 0.85)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- PolarLSM + PolarRouter Approved Production Design
- bench.cpp
- accuracy.py
- codec_test.cpp
- codec.cpp
- Spherra v1 implementation specification
- AccuracyTest
- Spherra project guide
- Spherra polar codec and format foundation implementation plan
- Q: What tech stack should PolarLSM plus PolarRouter use?
- Q: yes do that. also set up a comprehensive AGENTS.md that acts as the project guide. AGENTS.md should be set up so that any agent that reads it has an excellent understanding of the project, whats been done, what still needs t be done, what the next step is. there should be a rule in agents.md which states that agents must keep agents.md up to date. also make sure the graphify project graph stays up to date always
- Q: What should I name this project? Generate prospects across mythic, infrastructure, and AI-native styles with Claude.

## God Nodes (most connected - your core abstractions)
1. `PolarLSM + PolarRouter Approved Production Design` - 19 edges
2. `evaluate()` - 15 edges
3. `Spherra project guide` - 14 edges
4. `Spherra polar codec and format foundation implementation plan` - 14 edges
5. `AngleTables` - 13 edges
6. `Spherra v1 implementation specification` - 12 edges
7. `main()` - 9 edges
8. `7. Milestones` - 9 edges
9. `main()` - 8 edges
10. `scan_packed_angles_soa()` - 8 edges

## Surprising Connections (you probably didn't know these)
- `Polar Codec Experiment README` --references--> `V1 Quality Performance and Correctness Gates`  [EXTRACTED]
  experiments/codec_bench/README.md → docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md
- `Polar Codec Experiment README` --references--> `Adaptive AoS and Tiled-SoA Layouts`  [INFERRED]
  experiments/codec_bench/README.md → docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md
- `Polar Codec Experiment README` --references--> `Recursive Hyperspherical Angles`  [EXTRACTED]
  experiments/codec_bench/README.md → docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md
- `Polar Codec Experiment README` --references--> `Direct Int4 Direction Code`  [EXTRACTED]
  experiments/codec_bench/README.md → docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md
- `Polar Codec Experiment README` --references--> `Prototype Codec and Layout Benchmark Evidence`  [EXTRACTED]
  experiments/codec_bench/README.md → docs/superpowers/specs/2026-08-04-polar-lsm-router-design.md

## Import Cycles
- None detected.

## Communities (12 total, 1 thin omitted)

### Community 0 - "PolarLSM + PolarRouter Approved Production Design"
Cohesion: 0.16
Nodes (23): V1 Quality Performance and Correctness Gates, Adaptive AoS and Tiled-SoA Layouts, Blob Storage and Snapshot Leases, Immutable Cell-Local HNSW, Certified Pruning Error Bounds, Direct Int4 Direction Code, Spherical Direction Cells, PolarLSM + PolarRouter Approved Production Design (+15 more)

### Community 1 - "bench.cpp"
Cohesion: 0.22
Nodes (18): size_t, uint8_t, vector, main(), make_angle_codes(), make_angle_tables(), make_cartesian_codes(), make_query() (+10 more)

### Community 2 - "accuracy.py"
Cohesion: 0.31
Nodes (17): cone_metrics(), decode_scalar(), encode_scalar(), evaluate(), fit_scalar_codec(), from_hyperspherical(), main(), make_dataset() (+9 more)

### Community 3 - "codec_test.cpp"
Cohesion: 0.36
Nodes (11): vector, expect_near(), main(), test_cartesian_int4_score_maps_endpoints_to_unit_range(), test_hyperspherical_decode_matches_known_vector(), test_nibbles_round_trip(), test_packed_scores_match_unpacked_scores(), test_quantized_angle_score_uses_recursive_prefix() (+3 more)

### Community 4 - "codec.cpp"
Cohesion: 0.20
Nodes (34): AngleTables, angle_count, AngleTables::AngleTables(), cosine, cosines_, set, sine, sines_ (+26 more)

### Community 5 - "Spherra v1 implementation specification"
Cohesion: 0.07
Nodes (27): 10. Status and document maintenance, 11. Approval record, 1. Purpose and authority, 2. Starting state, 3. Fixed implementation constraints, 4. Implementation strategy, 5. Workspace and dependency boundaries, 6.1 Codec boundary (+19 more)

### Community 7 - "Spherra project guide"
Cohesion: 0.09
Nodes (23): After every project change, Approved technology stack, Before work, Completed, Current repository state, Current status and next step, Definition of done for any task, During work (+15 more)

### Community 8 - "Spherra polar codec and format foundation implementation plan"
Cohesion: 0.10
Nodes (18): Approval record, Plan self-review record, Planned file map, Scope and exit gate, Spherra polar codec and format foundation implementation plan, Task 1: Establish version control and the pinned Rust workspace, Task 2: Encode domain invariants before codec code, Task 3: Implement the exact scalar transform oracle (+10 more)

### Community 9 - "Q: What tech stack should PolarLSM plus PolarRouter use?"
Cohesion: 0.50
Nodes (3): Answer, Outcome, Q: What tech stack should PolarLSM plus PolarRouter use?

### Community 10 - "Q: yes do that. also set up a comprehensive AGENTS.md that acts as the project guide. AGENTS.md should be set up so that any agent that reads it has an excellent understanding of the project, whats been done, what still needs t be done, what the next step is. there should be a rule in agents.md which states that agents must keep agents.md up to date. also make sure the graphify project graph stays up to date always"
Cohesion: 0.40
Nodes (4): Answer, Outcome, Q: yes do that. also set up a comprehensive AGENTS.md that acts as the project guide. AGENTS.md should be set up so that any agent that reads it has an excellent understanding of the project, whats been done, what still needs t be done, what the next step is. there should be a rule in agents.md which states that agents must keep agents.md up to date. also make sure the graphify project graph stays up to date always, Source Nodes

### Community 11 - "Q: What should I name this project? Generate prospects across mythic, infrastructure, and AI-native styles with Claude."
Cohesion: 0.50
Nodes (3): Answer, Q: What should I name this project? Generate prospects across mythic, infrastructure, and AI-native styles with Claude., Source Nodes

## Knowledge Gaps
- **72 isolated node(s):** `seconds`, `vectors_per_second`, `checksum`, `cosines_`, `sines_` (+67 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **1 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Spherra v1 implementation specification` connect `Spherra v1 implementation specification` to `Spherra polar codec and format foundation implementation plan`?**
  _High betweenness centrality (0.073) - this node is a cross-community bridge._
- **Why does `Spherra project guide` connect `Spherra project guide` to `Spherra polar codec and format foundation implementation plan`?**
  _High betweenness centrality (0.066) - this node is a cross-community bridge._
- **What connects `seconds`, `vectors_per_second`, `checksum` to the rest of the system?**
  _72 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Spherra v1 implementation specification` be split into smaller, more focused modules?**
  _Cohesion score 0.07407407407407407 - nodes in this community are weakly interconnected._
- **Should `Spherra project guide` be split into smaller, more focused modules?**
  _Cohesion score 0.08695652173913043 - nodes in this community are weakly interconnected._
- **Should `Spherra polar codec and format foundation implementation plan` be split into smaller, more focused modules?**
  _Cohesion score 0.1038961038961039 - nodes in this community are weakly interconnected._