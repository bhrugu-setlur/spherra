"""Research-only MovieLens dot-product retrieval fixture, not a relevance benchmark.

Requires numpy==1.26.4 and blake3==1.0.8. Inputs stay local under their upstream
license; the script neither downloads nor redistributes ratings or factors.
"""
import argparse
import hashlib
import io
import json
import platform
import zipfile
from pathlib import Path

import blake3
import numpy as np

ARCHIVE_SHA256 = "50d2a982c66986937beb9ffb3aa76efe955bf3d5c6b761f4e3a7cd717c6a3229"


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--archive", type=Path, required=True)
    p.add_argument("--output-dir", type=Path, required=True)
    a = p.parse_args()
    archive = a.archive.read_bytes()
    if hashlib.sha256(archive).hexdigest() != ARCHIVE_SHA256:
        raise ValueError("MovieLens archive SHA256 mismatch")
    if a.output_dir.exists():
        raise ValueError("output directory already exists")
    with zipfile.ZipFile(io.BytesIO(archive)) as z:
        raw = z.read("ml-100k/u.data")
    ratings = np.loadtxt(io.BytesIO(raw), dtype=np.int64)
    assert ratings.shape == (100000, 4)
    assert len(set(map(tuple, ratings[:, :2]))) == 100000
    assert ratings[:, 0].min() == ratings[:, 1].min() == 1
    assert ratings[:, 0].max() == 943 and ratings[:, 1].max() == 1682
    assert ratings[:, 2].min() == 1 and ratings[:, 2].max() == 5
    matrix = np.zeros((943, 1682), dtype=np.float64)
    matrix[ratings[:, 0] - 1, ratings[:, 1] - 1] = ratings[:, 2]
    # Fixed, untuned rank-64 truncated SVD of the zero-filled rating matrix.
    # Query/item dot products approximate this matrix; row normalization changes
    # that objective. This does not evaluate held-out rating prediction.
    u, singular, vt = np.linalg.svd(matrix, full_matrices=False)
    u, singular, vt = u[:, :64], singular[:64], vt[:64]
    for i in range(64):
        if vt[i, np.argmax(np.abs(vt[i]))] < 0:
            vt[i] *= -1
            u[:, i] *= -1
    users = np.zeros((943, 768), dtype='<f4')
    items = np.zeros((1682, 768), dtype='<f4')
    users[:, :64] = u * np.sqrt(singular)
    items[:, :64] = vt.T * np.sqrt(singular)
    def order(n, namespace):
        return sorted(range(n), key=lambda i: blake3.blake3(f"spherra.movielens.dot.v1:{namespace}:{i+1}".encode()).digest())
    item_order, user_order = order(1682, 'item'), order(943, 'user')
    splits = {'training': items[item_order[:512]], 'indexed': items[item_order[512:]],
              'queries': users[user_order[:200]]}
    a.output_dir.mkdir(parents=True)
    descriptor = {'dataset': 'MovieLens 100K', 'source': 'https://grouplens.org/datasets/movielens/100k/',
                  'archive_sha256': ARCHIVE_SHA256, 'ratings_sha256': hashlib.sha256(raw).hexdigest(),
                  'builder': 'tools/build_movielens_dot_corpus.py',
                  'model': 'zero-filled ratings rank-64 SVD, symmetric square-root singular-value factors, zero-padded to 768; no normalization',
                  'purpose': 'original-factor dot-product retrieval, not held-out recommendation relevance',
                  'split': '512 disjoint item factors for codec calibration, 1170 indexed items, 200 user-factor queries; BLAKE3 ordering v1',
                  'python': platform.python_version(), 'numpy': np.__version__, 'splits': {}}
    for name, rows in splits.items():
        data = rows.astype('<f4').tobytes()
        (a.output_dir / (name + '.f32')).write_bytes(data)
        norms = np.linalg.norm(rows.astype(np.float64), axis=1)
        assert np.isfinite(rows).all() and norms.min() >= 1e-12 and norms.max() <= 65504
        descriptor['splits'][name] = {'file': name + '.f32', 'rows': len(rows),
            'blake3': blake3.blake3(data).hexdigest(), 'norm_min': float(norms.min()),
            'norm_median': float(np.median(norms)), 'norm_max': float(norms.max())}
    (a.output_dir / 'descriptor.json').write_text(json.dumps(descriptor, indent=2) + '\n')
    print(json.dumps(descriptor, indent=2))


if __name__ == '__main__':
    main()
