from __future__ import annotations

import argparse
import json
from dataclasses import dataclass

import numpy as np


@dataclass(frozen=True)
class ScalarCodec:
    edges: np.ndarray
    centers: np.ndarray


def normalize(vectors: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    norms = np.linalg.norm(vectors, axis=1)
    safe = np.where(norms == 0.0, 1.0, norms)
    return (vectors / safe[:, None]).astype(np.float32), norms.astype(np.float32)


def to_hyperspherical(vectors: np.ndarray) -> np.ndarray:
    if vectors.ndim != 2 or vectors.shape[1] < 2:
        raise ValueError("vectors must have shape (count, dimension>=2)")
    unit, _ = normalize(vectors.astype(np.float64, copy=False))
    count, dimension = unit.shape
    angles = np.empty((count, dimension - 1), dtype=np.float32)
    if dimension > 2:
        tail_squared = np.cumsum((unit[:, ::-1] ** 2), axis=1)[:, ::-1]
        ratios = unit[:, :-2] / np.sqrt(np.maximum(tail_squared[:, :-2], 1e-30))
        angles[:, :-1] = np.arccos(np.clip(ratios, -1.0, 1.0)).astype(np.float32)
    angles[:, -1] = np.mod(
        np.arctan2(unit[:, -1], unit[:, -2]), 2.0 * np.pi
    ).astype(np.float32)
    return angles


def from_hyperspherical(angles: np.ndarray) -> np.ndarray:
    if angles.ndim != 2 or angles.shape[1] < 1:
        raise ValueError("angles must have shape (count, dimension-1)")
    count, angle_count = angles.shape
    decoded = np.empty((count, angle_count + 1), dtype=np.float32)
    prefix = np.ones(count, dtype=np.float32)
    for index in range(angle_count - 1):
        decoded[:, index] = prefix * np.cos(angles[:, index])
        prefix *= np.sin(angles[:, index])
    decoded[:, -2] = prefix * np.cos(angles[:, -1])
    decoded[:, -1] = prefix * np.sin(angles[:, -1])
    return decoded


def fit_scalar_codec(values: np.ndarray, bins: int = 16) -> ScalarCodec:
    if values.ndim != 2 or bins < 2:
        raise ValueError("values must be a matrix and bins must be at least two")
    quantiles = np.arange(1, bins, dtype=np.float64) / bins
    edges = np.quantile(values, quantiles, axis=0).T.astype(np.float32)
    centers = np.empty((values.shape[1], bins), dtype=np.float32)
    for dimension in range(values.shape[1]):
        codes = np.searchsorted(edges[dimension], values[:, dimension], side="right")
        for code in range(bins):
            selected = values[codes == code, dimension]
            if selected.size:
                centers[dimension, code] = float(np.mean(selected))
            elif code == 0:
                centers[dimension, code] = edges[dimension, 0]
            elif code == bins - 1:
                centers[dimension, code] = edges[dimension, -1]
            else:
                centers[dimension, code] = 0.5 * (
                    edges[dimension, code - 1] + edges[dimension, code]
                )
    return ScalarCodec(edges=edges, centers=centers)


def encode_scalar(values: np.ndarray, codec: ScalarCodec) -> np.ndarray:
    if values.shape[1] != codec.centers.shape[0]:
        raise ValueError("value dimensions do not match codec")
    codes = np.empty(values.shape, dtype=np.uint8)
    for dimension in range(values.shape[1]):
        codes[:, dimension] = np.searchsorted(
            codec.edges[dimension], values[:, dimension], side="right"
        )
    return codes


def decode_scalar(codes: np.ndarray, codec: ScalarCodec) -> np.ndarray:
    if codes.shape[1] != codec.centers.shape[0]:
        raise ValueError("code dimensions do not match codec")
    decoded = np.empty(codes.shape, dtype=np.float32)
    for dimension in range(codes.shape[1]):
        decoded[:, dimension] = codec.centers[dimension, codes[:, dimension]]
    return decoded


def topk_indices(scores: np.ndarray, k: int) -> np.ndarray:
    candidates = np.argpartition(scores, -k, axis=1)[:, -k:]
    candidate_scores = np.take_along_axis(scores, candidates, axis=1)
    order = np.argsort(candidate_scores, axis=1)[:, ::-1]
    return np.take_along_axis(candidates, order, axis=1)


def recall_at_k(exact: np.ndarray, approximate: np.ndarray, k: int) -> float:
    exact_k = exact[:, :k]
    approximate_k = approximate[:, :k]
    total = 0
    for exact_row, approximate_row in zip(exact_k, approximate_k, strict=True):
        total += len(set(exact_row.tolist()) & set(approximate_row.tolist()))
    return total / (exact.shape[0] * k)


def make_dataset(
    kind: str,
    count: int,
    dimension: int,
    random: np.random.Generator,
    correlated_projection: np.ndarray | None = None,
    noise_std: float = 0.05,
) -> np.ndarray:
    if kind == "gaussian":
        raw = random.normal(size=(count, dimension)).astype(np.float32)
    elif kind == "correlated":
        if correlated_projection is None:
            latent_dimension = min(64, dimension)
            correlated_projection = random.normal(
                size=(latent_dimension, dimension)
            ).astype(np.float32)
        elif correlated_projection.ndim != 2 or correlated_projection.shape[1] != dimension:
            raise ValueError("correlated projection has the wrong shape")
        latent_dimension = correlated_projection.shape[0]
        latent = random.normal(size=(count, latent_dimension)).astype(np.float32)
        raw = latent @ correlated_projection
        if noise_std:
            raw += noise_std * random.normal(size=raw.shape).astype(np.float32)
    else:
        raise ValueError(f"unknown dataset kind: {kind}")
    unit, _ = normalize(raw)
    return unit


def score_decoded_corpus(queries: np.ndarray, decoded_corpus: np.ndarray) -> np.ndarray:
    if queries.ndim != 2 or decoded_corpus.ndim != 2:
        raise ValueError("queries and corpus must be matrices")
    if queries.shape[1] != decoded_corpus.shape[1]:
        raise ValueError("query and corpus dimensions do not match")
    return queries @ decoded_corpus.T


def reconstruction_metrics(original: np.ndarray, reconstructed: np.ndarray) -> dict[str, float]:
    unit_reconstructed, _ = normalize(reconstructed)
    cosines = np.sum(original * unit_reconstructed, axis=1)
    return {
        "mean_reconstruction_cosine": float(np.mean(cosines)),
        "p01_reconstruction_cosine": float(np.quantile(cosines, 0.01)),
    }


def cone_metrics(
    exact_scores: np.ndarray,
    approximate_scores: np.ndarray,
    selectivity: float = 0.001,
) -> dict[str, float]:
    recalls: list[float] = []
    precisions: list[float] = []
    for exact_row, approximate_row in zip(exact_scores, approximate_scores, strict=True):
        threshold = float(np.quantile(exact_row, 1.0 - selectivity))
        exact_set = set(np.flatnonzero(exact_row >= threshold).tolist())
        approximate_set = set(np.flatnonzero(approximate_row >= threshold).tolist())
        intersection = len(exact_set & approximate_set)
        recalls.append(intersection / max(1, len(exact_set)))
        precisions.append(intersection / max(1, len(approximate_set)))
    return {
        "cone_recall": float(np.mean(recalls)),
        "cone_precision": float(np.mean(precisions)),
    }


def evaluate(args: argparse.Namespace) -> list[dict[str, float | str | int]]:
    random = np.random.default_rng(args.seed)
    results: list[dict[str, float | str | int]] = []
    for dataset_kind in ("gaussian", "correlated"):
        correlated_projection = None
        if dataset_kind == "correlated":
            correlated_projection = random.normal(
                size=(min(64, args.dimension), args.dimension)
            ).astype(np.float32)
        calibration = make_dataset(
            dataset_kind, args.calibration, args.dimension, random, correlated_projection
        )
        corpus = make_dataset(
            dataset_kind, args.corpus, args.dimension, random, correlated_projection
        )
        queries = make_dataset(
            dataset_kind, args.queries, args.dimension, random, correlated_projection
        )

        exact_scores = queries @ corpus.T
        exact_topk = topk_indices(exact_scores, args.k)

        cartesian_codec = fit_scalar_codec(calibration, bins=args.bins)
        cartesian_corpus = decode_scalar(
            encode_scalar(corpus, cartesian_codec), cartesian_codec
        )
        cartesian_corpus, _ = normalize(cartesian_corpus)

        angle_calibration = to_hyperspherical(calibration)
        angle_codec = fit_scalar_codec(angle_calibration, bins=args.bins)
        angle_corpus = from_hyperspherical(
            decode_scalar(encode_scalar(to_hyperspherical(corpus), angle_codec), angle_codec)
        )
        angle_corpus, _ = normalize(angle_corpus)

        for name, decoded_corpus in (
            ("cartesian_int4", cartesian_corpus),
            ("recursive_angles_int4", angle_corpus),
        ):
            approximate_scores = score_decoded_corpus(queries, decoded_corpus)
            approximate_topk = topk_indices(approximate_scores, args.k)
            result: dict[str, float | str | int] = {
                "dataset": dataset_kind,
                "codec": name,
                "corpus": args.corpus,
                "queries": args.queries,
                "dimension": args.dimension,
                "bits_per_component": int(np.log2(args.bins)),
                "recall_at_k": recall_at_k(exact_topk, approximate_topk, args.k),
            }
            result.update(reconstruction_metrics(corpus, decoded_corpus))
            result.update(cone_metrics(exact_scores, approximate_scores))
            results.append(result)
    return results


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--calibration", type=int, default=10_000)
    parser.add_argument("--corpus", type=int, default=20_000)
    parser.add_argument("--queries", type=int, default=200)
    parser.add_argument("--dimension", type=int, default=768)
    parser.add_argument("--bins", type=int, default=16)
    parser.add_argument("--k", type=int, default=10)
    parser.add_argument("--seed", type=int, default=20260804)
    args = parser.parse_args()
    print(json.dumps(evaluate(args), indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
