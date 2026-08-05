---
type: "query"
date: "2026-08-04T22:41:51.160034+00:00"
question: "ok so if i understand this correctly: we take a vector and split it into its magnitude and its direction. the magnitude is a 16 bit number and the direction is stored as a unit vector. now since we are embedding into 768 dimensions, that unit vector that determines direction also must have 768 components. but since unit vectors are fixed at a magnitude of 1, all of those components that make up the unit vector must fall between -1 and 1. this essentially mimics the fixed range of connected angles, but decoding the unit vector components is easier so searching is faster with this approach. tell me if im wrong"
contributor: "graphify"
source_nodes: ["Vector representation and scoring", "TurboQuant-Style Orthogonal Preconditioner", "Direct Int4 Direction Code", "PQ96x8 Residual Refinement", "Recursive Hyperspherical Angles"]
---

# Q: ok so if i understand this correctly: we take a vector and split it into its magnitude and its direction. the magnitude is a 16 bit number and the direction is stored as a unit vector. now since we are embedding into 768 dimensions, that unit vector that determines direction also must have 768 components. but since unit vectors are fixed at a magnitude of 1, all of those components that make up the unit vector must fall between -1 and 1. this essentially mimics the fixed range of connected angles, but decoding the unit vector components is easier so searching is faster with this approach. tell me if im wrong

## Answer

Expanded from original query via graph vocabulary: [vector, direction, representation, direct, int, transform, preconditioner, residual, recursive, angles]. The understanding is correct at a high level. Spherra stores the original norm as FP16 and normalizes the 768-dimensional direction. Because the direction has unit norm, every component lies in [-1,1]. Before permanent compression, Spherra applies an orthogonal transform that preserves the unit norm and then quantizes the 768 transformed components to four bits each. Therefore the permanent direction code represents an approximation to the transformed unit vector, not exact floating-point components of the raw unit vector. Quantization can also make the reconstructed primary direction slightly different from unit length, so a candidate-only residual improves final scoring and Spherra deliberately does not renormalize the reconstruction. The bounded direct components provide the same fixed-range quantization benefit as bounded angles while avoiding recursive decoding, making scans faster.

## Source Nodes

- Vector representation and scoring
- TurboQuant-Style Orthogonal Preconditioner
- Direct Int4 Direction Code
- PQ96x8 Residual Refinement
- Recursive Hyperspherical Angles