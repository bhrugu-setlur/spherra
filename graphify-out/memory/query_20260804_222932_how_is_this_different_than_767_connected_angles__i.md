---
type: "query"
date: "2026-08-04T22:29:32.685623+00:00"
question: "how is this different than 767 connected angles? i thought one of the reasons why storing in polar is more efficient is because angles are a fixed range"
contributor: "graphify"
source_nodes: ["Direct Int4 Direction Code", "Recursive Hyperspherical Angles", "test_cartesian_int4_score_maps_endpoints_to_unit_range()", "test_quantized_angle_score_uses_recursive_prefix", "Prototype Codec and Layout Benchmark Evidence"]
---

# Q: how is this different than 767 connected angles? i thought one of the reasons why storing in polar is more efficient is because angles are a fixed range

## Answer

Expanded from original query via graph vocabulary: [recursive, hyperspherical, angles, direct, int, direction, fixed, range, quantized, cartesian, polar, performance]. Polar storage only requires separating magnitude r from unit direction u; u does not have to be encoded as angles. Recursive hyperspherical encoding stores 767 angles, where later reconstructed coordinates depend on products of sine values from earlier angles. Direct encoding stores 768 transformed unit-direction components independently. The direct components are also bounded because every component of a unit vector lies in [-1,1], so they can use the same fixed-width four-bit quantization idea. At four bits each, 767 angles require 3068 bits or 383.5 bytes and pad to 384 bytes, while 768 direct components require exactly 384 bytes, so there is no physical byte saving. Connected angles make decoding and scoring sequential, allow early angle error to affect many later coordinates, and are harder to accelerate. Direct codes support a straightforward independent multiply-and-add score. Spherra therefore gets its main compression from replacing FP32 components with four-bit codes, not from removing one coordinate. Its prototype found no meaningful retrieval advantage for recursive angles at the same bit budget, while direct codes were faster.

## Source Nodes

- Direct Int4 Direction Code
- Recursive Hyperspherical Angles
- test_cartesian_int4_score_maps_endpoints_to_unit_range()
- test_quantized_angle_score_uses_recursive_prefix
- Prototype Codec and Layout Benchmark Evidence