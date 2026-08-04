# Graph Report - .  (2026-08-04)

## Corpus Check
- 1 files · ~21,008 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 222 nodes · 381 edges · 23 communities (21 shown, 2 thin omitted)
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 2 edges (avg confidence: 0.85)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- C++ Codec Kernels
- V1 Implementation Spec
- Project Guide Policy
- Production Architecture
- Codec Foundation Plan
- C++ Benchmark Harness
- Python Accuracy Experiment
- C++ Codec Tests
- Python Accuracy Tests
- Rust Bootstrap and CI
- Project Guide Request
- Tech Stack Query
- Naming Query
- Dependency Policy Script
- Local CI Script

## God Nodes (most connected - your core abstractions)
1. `PolarLSM + PolarRouter Approved Production Design` - 19 edges
2. `evaluate()` - 15 edges
3. `Spherra project guide` - 14 edges
4. `Spherra polar codec and format foundation implementation plan` - 14 edges
5. `AngleTables` - 13 edges
6. `Spherra v1 implementation specification` - 12 edges
7. `7. Milestones` - 9 edges
8. `main()` - 9 edges
9. `6. Stable project-owned interfaces` - 8 edges
10. `main()` - 8 edges

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

## Communities (23 total, 2 thin omitted)

### Community 0 - "C++ Codec Kernels"
Cohesion: 0.20
Nodes (34): AngleTables, angle_count, AngleTables::AngleTables(), cosine, cosines_, set, sine, sines_ (+26 more)

### Community 1 - "V1 Implementation Spec"
Cohesion: 0.07
Nodes (27): 10. Status and document maintenance, 11. Approval record, 1. Purpose and authority, 2. Starting state, 3. Fixed implementation constraints, 4. Implementation strategy, 5. Workspace and dependency boundaries, 6.1 Codec boundary (+19 more)

### Community 2 - "Project Guide Policy"
Cohesion: 0.08
Nodes (24): After every project change, Approved technology stack, Before work, Completed, Current repository state, Current status and next step, Definition of done for any task, During work (+16 more)

### Community 3 - "Production Architecture"
Cohesion: 0.16
Nodes (23): V1 Quality Performance and Correctness Gates, Adaptive AoS and Tiled-SoA Layouts, Blob Storage and Snapshot Leases, Immutable Cell-Local HNSW, Certified Pruning Error Bounds, Direct Int4 Direction Code, Spherical Direction Cells, PolarLSM + PolarRouter Approved Production Design (+15 more)

### Community 4 - "Codec Foundation Plan"
Cohesion: 0.10
Nodes (18): Approval record, Plan self-review record, Planned file map, Scope and exit gate, Spherra polar codec and format foundation implementation plan, Task 1: Establish version control and the pinned Rust workspace, Task 2: Encode domain invariants before codec code, Task 3: Implement the exact scalar transform oracle (+10 more)

### Community 5 - "C++ Benchmark Harness"
Cohesion: 0.22
Nodes (18): size_t, uint8_t, vector, main(), make_angle_codes(), make_angle_tables(), make_cartesian_codes(), make_query() (+10 more)

### Community 6 - "Python Accuracy Experiment"
Cohesion: 0.31
Nodes (17): cone_metrics(), decode_scalar(), encode_scalar(), evaluate(), fit_scalar_codec(), from_hyperspherical(), main(), make_dataset() (+9 more)

### Community 7 - "C++ Codec Tests"
Cohesion: 0.36
Nodes (11): vector, expect_near(), main(), test_cartesian_int4_score_maps_endpoints_to_unit_range(), test_hyperspherical_decode_matches_known_vector(), test_nibbles_round_trip(), test_packed_scores_match_unpacked_scores(), test_quantized_angle_score_uses_recursive_prefix() (+3 more)

### Community 9 - "Rust Bootstrap and CI"
Cohesion: 0.40
Nodes (5): Rust Workspace Bootstrap, Task 2 Next Step, Task 1 Pinned Rust Workspace, Quality CI Job, Security CI Job

### Community 10 - "Project Guide Request"
Cohesion: 0.40
Nodes (4): Answer, Outcome, Q: yes do that. also set up a comprehensive AGENTS.md that acts as the project guide. AGENTS.md should be set up so that any agent that reads it has an excellent understanding of the project, whats been done, what still needs t be done, what the next step is. there should be a rule in agents.md which states that agents must keep agents.md up to date. also make sure the graphify project graph stays up to date always, Source Nodes

### Community 11 - "Tech Stack Query"
Cohesion: 0.50
Nodes (3): Answer, Outcome, Q: What tech stack should PolarLSM plus PolarRouter use?

### Community 12 - "Naming Query"
Cohesion: 0.50
Nodes (3): Answer, Q: What should I name this project? Generate prospects across mythic, infrastructure, and AI-native styles with Claude., Source Nodes

### Community 13 - "Dependency Policy Script"
Cohesion: 0.83
Nodes (3): cargo_executable(), main(), normal_or_build_dependencies()

## Knowledge Gaps
- **78 isolated node(s):** `Mandatory living-document rule`, `Start here`, `Project in one paragraph`, `Naming contract`, `Product and hardware contract` (+73 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **2 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Spherra v1 implementation specification` connect `V1 Implementation Spec` to `Codec Foundation Plan`?**
  _High betweenness centrality (0.060) - this node is a cross-community bridge._
- **Why does `Spherra project guide` connect `Project Guide Policy` to `Codec Foundation Plan`?**
  _High betweenness centrality (0.056) - this node is a cross-community bridge._
- **What connects `Mandatory living-document rule`, `Start here`, `Project in one paragraph` to the rest of the system?**
  _78 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `V1 Implementation Spec` be split into smaller, more focused modules?**
  _Cohesion score 0.07407407407407407 - nodes in this community are weakly interconnected._
- **Should `Project Guide Policy` be split into smaller, more focused modules?**
  _Cohesion score 0.08333333333333333 - nodes in this community are weakly interconnected._
- **Should `Codec Foundation Plan` be split into smaller, more focused modules?**
  _Cohesion score 0.1038961038961039 - nodes in this community are weakly interconnected._