---
type: "query"
date: "2026-08-04T22:26:56.443732+00:00"
question: "is the direction a single value or a collection of values?"
contributor: "graphify"
source_nodes: ["Direct Int4 Direction Code", "Vector representation and scoring", "PolarCode"]
---

# Q: is the direction a single value or a collection of values?

## Answer

Expanded from original query via graph vocabulary: [direction, vector, direct, code, int, polar, representation]. The direction is a collection of values, not one value. For a 768-dimensional vector, Spherra stores 768 transformed direction components. Each component is compressed to four bits, so two components fit in one byte and the primary direction occupies 384 bytes. The separate magnitude is one FP16 value. Although a unit direction has 767 mathematical degrees of freedom, Spherra deliberately stores 768 direct components rather than 767 recursive angles.

## Source Nodes

- Direct Int4 Direction Code
- Vector representation and scoring
- PolarCode