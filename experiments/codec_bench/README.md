# Spherra polar codec experiment

This native/Python harness tests the architectural question that blocked the database design: should the primary serving code use recursive hyperspherical angles, or store a directly quantized unit direction alongside its radius?

It is intentionally small. It is evidence for choosing the access representation, not a production ANN or SIMD benchmark.

## Run correctness tests

```bash
clang++ -std=c++20 -O3 -mcpu=native -DNDEBUG -Wall -Wextra -Werror \
  experiments/codec_bench/codec_test.cpp \
  experiments/codec_bench/codec.cpp \
  -o /tmp/spherra_codec_test
/tmp/spherra_codec_test

uv run --python 3.12 --with numpy --with scipy \
  python experiments/codec_bench/accuracy_test.py
```

## Run native layout/access benchmark

```bash
clang++ -std=c++20 -O3 -mcpu=native -DNDEBUG -Wall -Wextra -Werror \
  experiments/codec_bench/bench.cpp \
  experiments/codec_bench/codec.cpp \
  -o /tmp/spherra_codec_bench
/tmp/spherra_codec_bench 131072
```

The accepted run on the target M1 Pro measured:

| Representation/layout | Sequential or scan | Random access |
|---|---:|---:|
| Recursive angles, AoS | 0.951 M vec/s | 0.873 M vec/s |
| Recursive angles, full SoA | 1.649 M vec/s | — |
| Direct int4, AoS | 1.378 M vec/s | 1.193 M vec/s |
| Direct int4, full SoA | 2.115 M vec/s | — |
| Direct int4, tiled SoA 32 | 1.709 M vec/s | 1.104 M vec/s |
| Direct int4, tiled SoA 64 | 1.777 M vec/s | 0.982 M vec/s |

## Run synthetic retrieval-quality comparison

```bash
uv run --python 3.12 --with numpy --with scipy \
  python experiments/codec_bench/accuracy.py \
  --calibration 10000 --corpus 20000 --queries 200
```

The experiment keeps queries at high precision and uses one shared latent projection for correlated calibration, corpus, and query samples. At equal 4 bits/component, direct and recursive codes were effectively tied on recall, cone behavior, and reconstruction. The combined result—similar quality but lower dependency and higher serving throughput—supports direct direction codes for the primary representation.

The next implementation phase must replace these scalar kernels with production NEON/LUT kernels and run the real-corpus and residual-rerank gates in the approved design.
