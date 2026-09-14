"""Research-only DPR dot-product retrieval fixture, not a relevance benchmark.

Requires numpy==1.26.4, pyarrow==17.0.0, blake3==1.0.8, torch==2.5.1 and
transformers==4.46.3. Inputs stay local under their upstream licenses; the script
neither downloads nor redistributes passages, questions, models or vectors.

Passage vectors are the published DPR single-NQ context embeddings (native 768D,
trained for unnormalized dot-product retrieval). Queries are NQ-open validation
questions encoded locally with the matching DPR question encoder.
"""
import argparse
import hashlib
import json
import platform
from pathlib import Path

import blake3
import numpy as np
import pyarrow as pa
import pyarrow.compute as pc
import pyarrow.parquet as pq

WIKI_DPR_REVISION = "0ae2454140a2d6864475c83f26e6dc9cd4ab9ce4"
# Hugging Face LFS SHA256 values for data/psgs_w100/nq at the revision above.
SHARDS = {
    "train-00000-of-00157.parquet": "cf04d22ef173533b97d4376637d2e2f090ccf5453c91c750c8c55845f6b1730c",
    "train-00020-of-00157.parquet": "f4ba64c1cb314ba47a109acf46f37d5f1763bd36127578178066685c9749d534",
    "train-00040-of-00157.parquet": "9691ad7be66f6e140271b0ee8a674a7dffab4294bd65eb19b4d31951b7221e06",
    "train-00060-of-00157.parquet": "8337f7a2a5304fd3146637db2c0a58f2fe7b60b5d7451d333c4be21606e87aba",
    "train-00080-of-00157.parquet": "d25416e11aaf8cee4b4794f2eb927d87d915d963bb831d958ed1b9915cd2d1c0",
    "train-00100-of-00157.parquet": "7cab2536e0fcb586ce568074b065706d8bff8d356704dc97448e59ec11c06933",
    "train-00120-of-00157.parquet": "fc778e3578c33ba94e14044f28b2cba272138b8ecb9c68134f3c0acd39ad5451",
    "train-00140-of-00157.parquet": "094ace2f8681ae0fe18c3a88d99d33297be70a6ed334ee79248475196867378b",
}
NQ_OPEN_REVISION = "5dd9790a83002ad084ddeb7c420dc716852c6f28"
NQ_VALIDATION_SHA256 = "b074bed0bccb56fa1551a8ac1c9c51ce89bc11c7fbb6a9c713b2c33a98531e12"
QUESTION_ENCODER = "facebook/dpr-question_encoder-single-nq-base"
QUESTION_ENCODER_WEIGHTS_SHA256 = "7fd8074bf164ea506267e1894b4b7579d25c56858d4890d707272557b7c7cc00"


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 24), b""):
            h.update(block)
    return h.hexdigest()


def order(keys, namespace):
    return sorted(range(len(keys)),
                  key=lambda i: blake3.blake3(f"spherra.dpr.dot.v1:{namespace}:{keys[i]}".encode()).digest())


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--shards-dir", type=Path, required=True)
    p.add_argument("--questions", type=Path, required=True)
    p.add_argument("--question-encoder", type=Path, required=True)
    p.add_argument("--output-dir", type=Path, required=True)
    p.add_argument("--rows", type=int, default=1_000_000)
    p.add_argument("--training-rows", type=int, default=4096)
    p.add_argument("--queries", type=int, default=1000)
    a = p.parse_args()
    if a.output_dir.exists():
        raise ValueError("output directory already exists")
    for name, digest in SHARDS.items():
        if sha256(a.shards_dir / name) != digest:
            raise ValueError(f"shard SHA256 mismatch: {name}")
    if sha256(a.questions) != NQ_VALIDATION_SHA256:
        raise ValueError("NQ-open validation SHA256 mismatch")
    if sha256(a.question_encoder / "pytorch_model.bin") != QUESTION_ENCODER_WEIGHTS_SHA256:
        raise ValueError("question encoder weights SHA256 mismatch")

    ids, blocks = [], []
    for name in SHARDS:
        table = pq.read_table(a.shards_dir / name, columns=["id", "embeddings"])
        column = table.column("embeddings").combine_chunks()
        assert pc.all(pc.equal(pc.list_value_length(column), 768)).as_py()
        blocks.append(np.asarray(column.flatten(), dtype="<f4").reshape(-1, 768))
        ids.extend(table.column("id").to_pylist())
    passages = np.concatenate(blocks)
    del blocks
    assert len(set(ids)) == len(ids) == len(passages)
    needed = a.training_rows + a.rows
    if needed > len(passages):
        raise ValueError("not enough passages")
    # Codec training and indexed rows are disjoint; row ID is the output position.
    chosen = order(ids, "passage")[:needed]
    splits = {"training": passages[chosen[:a.training_rows]], "indexed": passages[chosen[a.training_rows:]]}
    del passages

    import torch
    from transformers import DPRQuestionEncoder, DPRQuestionEncoderTokenizer
    torch.manual_seed(0)
    torch.use_deterministic_algorithms(True)
    questions = pq.read_table(a.questions).column("question").to_pylist()
    picked = order(list(range(len(questions))), "question")[:a.queries]
    tokenizer = DPRQuestionEncoderTokenizer.from_pretrained(a.question_encoder)
    model = DPRQuestionEncoder.from_pretrained(a.question_encoder).eval()
    encoded = []
    with torch.no_grad():
        # One question per forward pass: no padding, so vectors do not depend on batching.
        for i in picked:
            tokens = tokenizer(questions[i], return_tensors="pt", truncation=True, max_length=256)
            encoded.append(model(**tokens).pooler_output[0].to(torch.float32).numpy())
    splits["queries"] = np.stack(encoded).astype("<f4")

    a.output_dir.mkdir(parents=True)
    descriptor = {
        "dataset": "DPR Wikipedia passages (psgs_w100, single-NQ context embeddings) with NQ-open validation questions",
        "sources": {
            "passages": f"https://huggingface.co/datasets/facebook/wiki_dpr@{WIKI_DPR_REVISION} data/psgs_w100/nq",
            "shard_sha256": SHARDS,
            "questions": f"https://huggingface.co/datasets/google-research-datasets/nq_open@{NQ_OPEN_REVISION} nq_open/validation",
            "questions_sha256": NQ_VALIDATION_SHA256,
            "question_encoder": QUESTION_ENCODER,
            "question_encoder_weights_sha256": QUESTION_ENCODER_WEIGHTS_SHA256,
        },
        "licenses": "wiki_dpr and DPR models CC-BY-NC-4.0; NQ-open CC-BY-SA-3.0; local research use, no redistribution",
        "builder": "tools/build_dpr_dot_corpus.py",
        "model": "native 768D DPR single-NQ encoders trained for dot-product retrieval; no normalization",
        "purpose": "original-vector dot-product retrieval accuracy, not question-answering relevance",
        "split": f"BLAKE3 ordering v1 over passage IDs from 8 shards: {a.training_rows} codec training rows, "
                 f"then {a.rows} indexed rows; {a.queries} NQ-open validation questions",
        "python": platform.python_version(), "numpy": np.__version__, "pyarrow": pa.__version__,
        "torch": torch.__version__, "transformers": __import__("transformers").__version__,
        "splits": {},
    }
    for name, rows in splits.items():
        data = np.ascontiguousarray(rows, dtype="<f4").tobytes()
        (a.output_dir / (name + ".f32")).write_bytes(data)
        norms = np.linalg.norm(rows.astype(np.float64), axis=1)
        assert np.isfinite(rows).all() and norms.min() >= 1e-12 and norms.max() <= 65504
        descriptor["splits"][name] = {
            "file": name + ".f32", "rows": len(rows), "blake3": blake3.blake3(data).hexdigest(),
            "norm_min": float(norms.min()), "norm_p01": float(np.percentile(norms, 1)),
            "norm_median": float(np.median(norms)), "norm_p99": float(np.percentile(norms, 99)),
            "norm_max": float(norms.max()),
        }
    (a.output_dir / "descriptor.json").write_text(json.dumps(descriptor, indent=2) + "\n")
    print(json.dumps(descriptor, indent=2))


if __name__ == "__main__":
    main()
