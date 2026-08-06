#!/usr/bin/env python3
"""Build the pinned BEIR SciFact FP32 corpus and its Spherra descriptor.

SciFact is a *smoke-scale* real corpus. It proves the measurement harness works
on real embeddings; it is not sufficient evidence for a 10M format freeze. See
`docs/benchmarks/README.md`.

The builder refuses to produce unpinned evidence: if the immutable upstream
dataset or model revision cannot be resolved, it exits nonzero rather than
writing a corpus whose provenance cannot be reproduced later.

Usage:

    uv run --python 3.12 \
      --with mteb --with sentence-transformers --with huggingface-hub \
      --with numpy --with blake3 \
      python tools/build_scifact_corpus.py --output-dir corpora/scifact
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

DATASET_REPO = "mteb/scifact"
MODEL_REPO = "sentence-transformers/all-mpnet-base-v2"
DIMENSION = 768
LICENSE = "CC BY-NC 2.0 (BEIR SciFact); model Apache-2.0"


def resolve_revisions(
    dataset_revision: str | None = None,
    model_revision: str | None = None,
) -> tuple[str, str]:
    """Return the immutable dataset and model commit hashes, or exit nonzero."""
    if (dataset_revision is None) != (model_revision is None):
        sys.exit("--dataset-revision and --model-revision must be supplied together")
    if dataset_revision is not None and model_revision is not None:
        for label, revision in (
            ("dataset", dataset_revision),
            ("model", model_revision),
        ):
            if re.fullmatch(r"[0-9a-fA-F]{40,64}", revision) is None:
                sys.exit(f"{label} revision must be a full immutable commit hash")
        return dataset_revision.lower(), model_revision.lower()

    try:
        from huggingface_hub import HfApi
    except ImportError as error:  # pragma: no cover - environment problem
        sys.exit(f"huggingface_hub is required to pin revisions: {error}")

    api = HfApi()
    try:
        dataset_revision = api.dataset_info(DATASET_REPO).sha
        model_revision = api.model_info(MODEL_REPO).sha
    except Exception as error:  # noqa: BLE001 - any failure is disqualifying
        sys.exit(f"cannot resolve immutable upstream revisions: {error}")

    if not dataset_revision or not model_revision:
        sys.exit(
            "upstream did not report a commit hash for the dataset or the model; "
            "refusing to write unpinned evidence"
        )
    return dataset_revision, model_revision


def load_corpus_texts(dataset_revision: str) -> list[str]:
    """Load SciFact corpus passages at the pinned dataset revision."""
    try:
        from datasets import load_dataset
    except ImportError as error:  # pragma: no cover - environment problem
        sys.exit(f"the mteb dataset loader requires `datasets`: {error}")

    corpus = load_dataset(
        DATASET_REPO,
        "corpus",
        split="corpus",
        revision=dataset_revision,
    )

    texts: list[str] = []
    for record in corpus:
        title = (record.get("title") or "").strip()
        body = (record.get("text") or "").strip()
        texts.append(f"{title}\n{body}".strip() if title else body)

    if not texts:
        sys.exit("the pinned SciFact revision produced no corpus passages")
    return texts


def embed(texts: list[str], model_revision: str, batch_size: int):
    try:
        import numpy as np
        from sentence_transformers import SentenceTransformer
    except ImportError as error:  # pragma: no cover - environment problem
        sys.exit(f"sentence-transformers and numpy are required: {error}")

    model = SentenceTransformer(MODEL_REPO, revision=model_revision)
    embeddings = model.encode(
        texts,
        batch_size=batch_size,
        convert_to_numpy=True,
        normalize_embeddings=True,
        show_progress_bar=True,
    )
    embeddings = np.ascontiguousarray(embeddings, dtype=np.float32)

    if embeddings.ndim != 2 or embeddings.shape[1] != DIMENSION:
        sys.exit(
            f"expected {DIMENSION}-dimensional embeddings, got shape {embeddings.shape}"
        )
    if not np.isfinite(embeddings).all():
        sys.exit("the embedding model produced a non-finite value")
    return embeddings


def blake3_hex(path: Path) -> str:
    try:
        from blake3 import blake3
    except ImportError as error:  # pragma: no cover - environment problem
        sys.exit(f"the blake3 package is required to pin the corpus: {error}")

    hasher = blake3()
    with path.open("rb") as handle:
        while chunk := handle.read(1 << 20):
            hasher.update(chunk)
    return hasher.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--batch-size", type=int, default=32)
    parser.add_argument("--dataset-revision")
    parser.add_argument("--model-revision")
    arguments = parser.parse_args()

    dataset_revision, model_revision = resolve_revisions(
        arguments.dataset_revision,
        arguments.model_revision,
    )
    texts = load_corpus_texts(dataset_revision)
    embeddings = embed(texts, model_revision, arguments.batch_size)

    arguments.output_dir.mkdir(parents=True, exist_ok=True)
    vectors_path = arguments.output_dir / "scifact-mpnet-768.f32"
    descriptor_path = arguments.output_dir / "scifact-mpnet-768.json"

    # Row-major little-endian FP32, exactly what the Rust reader expects.
    with vectors_path.open("wb") as handle:
        handle.write(embeddings.astype("<f4", copy=False).tobytes(order="C"))

    byte_len = vectors_path.stat().st_size
    expected = embeddings.shape[0] * DIMENSION * 4
    if byte_len != expected:
        sys.exit(f"wrote {byte_len} bytes but expected {expected}")

    resolved_vectors_path = vectors_path.resolve()
    try:
        descriptor_vectors_path = resolved_vectors_path.relative_to(Path.cwd().resolve())
    except ValueError:
        descriptor_vectors_path = resolved_vectors_path

    descriptor = {
        "name": "beir-scifact-mpnet-768",
        "path": str(descriptor_vectors_path),
        "byte_len": byte_len,
        "blake3": blake3_hex(vectors_path),
        "row_count": int(embeddings.shape[0]),
        "dimension": DIMENSION,
        "normalization": "l2-unit",
        "source_dataset_revision": f"{DATASET_REPO}@{dataset_revision}",
        "embedding_model_revision": f"{MODEL_REPO}@{model_revision}",
        "license": LICENSE,
    }
    descriptor_path.write_text(json.dumps(descriptor, indent=2) + "\n", encoding="utf-8")

    print(f"wrote {descriptor['row_count']} vectors to {vectors_path}")
    print(f"wrote descriptor to {descriptor_path}")
    print("SciFact is smoke-scale evidence only; see docs/benchmarks/README.md")
    return 0


if __name__ == "__main__":
    sys.exit(main())
