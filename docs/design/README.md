# Design documents

## Current

| Document | What it covers |
|---|---|
| [Local index design](2026-09-13-local-index-design.md) | Architecture, public API, storage, search and score ranges |
| [Implementation specification](2026-09-13-local-index-implementation-spec.md) | Build order, tests and acceptance gates |
| [Stored-magnitude amendment](2026-09-14-stored-magnitude-amendment.md) | Keeping each vector's FP16 length |
| [Dot-product search amendment](2026-09-14-dot-product-search-amendment.md) | Length-aware search and its score ranges |
| [Reconstruction-length amendment](2026-09-14-reconstruction-length-amendment.md) | Dividing refined scores by the rebuilt vector's length |

## Archive

Earlier design for a distributed vector database, stopped in favor of the local
index. Kept for context.

| Document | What it covered |
|---|---|
| [Distributed index design](archive/2026-08-04-polar-lsm-router-design.md) | Log-structured storage, routing and replication |
| [Distributed implementation specification](archive/2026-08-04-polar-v1-implementation-spec.md) | Milestones for that design |
