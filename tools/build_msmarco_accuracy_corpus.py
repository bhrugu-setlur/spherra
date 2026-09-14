"""Pinned MS MARCO real-query experimental corpus.
This is a constructed 100k-passage retrieval workload, not the full benchmark.
"""

import argparse, heapq, json, time, sys, csv, importlib.metadata
from pathlib import Path
import blake3

DATA = "a918e0d11a77ed33f42f29d98340b655593b96ad"
QRELS = "253fbf8a3f8d4a0932b63882b5162bedc84779f5"
MODEL = "e8c3b32edf5434bc2275fc9bab85f82640a19130"


def key(namespace, ident):
    return int.from_bytes(
        blake3.blake3((namespace + "\0" + str(ident)).encode()).digest()[:16], "big"
    )


def retain(heap, limit, ident, text):
    priority = key("msmarco-document-sample-v1", ident)
    entry = (-priority, str(ident), text)
    if len(heap) < limit:
        heapq.heappush(heap, entry)
    elif entry > heap[0]:
        heapq.heapreplace(heap, entry)


def normalized_text(text):
    return " ".join(text.casefold().split())


def digest(path):
    h = blake3.blake3()
    with Path(path).open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def main():
    from huggingface_hub import hf_hub_download
    import pyarrow.parquet as pq
    import numpy as np, torch
    from sentence_transformers import SentenceTransformer

    p = argparse.ArgumentParser()
    p.add_argument("--output-dir", required=True)
    p.add_argument("--rows", type=int, default=100000)
    p.add_argument("--training-rows", type=int, default=32768)
    p.add_argument("--queries-per-split", type=int, default=200)
    p.add_argument("--batch-size", type=int, default=32)
    p.add_argument("--device", default="mps")
    a = p.parse_args()
    if (
        a.rows < 10
        or not 344 <= a.training_rows <= 32768
        or not 1 <= a.queries_per_split <= 1000
    ):
        raise ValueError("invalid sizes")
    out = Path(a.output_dir)
    out.mkdir(parents=True, exist_ok=False)
    corpus = hf_hub_download(
        "BeIR/msmarco",
        "corpus/corpus-00000-of-00001.parquet",
        repo_type="dataset",
        revision=DATA,
    )
    queries = hf_hub_download(
        "BeIR/msmarco",
        "queries/queries-00000-of-00001.parquet",
        repo_type="dataset",
        revision=DATA,
    )
    qrels = hf_hub_download(
        "BeIR/msmarco-qrels", "dev.tsv", repo_type="dataset", revision=QRELS
    )
    with open(qrels) as f:
        rel = list(csv.DictReader(f, delimiter="\t"))
    print("qrels fields", list(rel[0]), flush=True)
    query_id = lambda r: str(r["query-id"])
    doc_id = lambda r: str(r["corpus-id"])
    eligible = {query_id(r) for r in rel}
    query_text = {}
    for batch in pq.ParquetFile(queries).iter_batches(batch_size=65536):
        for r in batch.to_pylist():
            if str(r["_id"]) in eligible:
                query_text[str(r["_id"])] = r["text"]
    qids = []
    question_texts = set()
    for ident in sorted(eligible, key=lambda x: (key("msmarco-query-split-v1", x), x)):
        if ident not in query_text:
            raise ValueError("query IDs missing from source")
        text = normalized_text(query_text[ident])
        if text in question_texts:
            continue
        question_texts.add(text)
        qids.append(ident)
        if len(qids) == 2 * a.queries_per_split:
            break
    if len(qids) != 2 * a.queries_per_split:
        raise ValueError("not enough distinct queries")
    selected = set(qids)
    rels = [r for r in rel if query_id(r) in selected and float(r["score"]) > 0]
    positive = {doc_id(r) for r in rels}
    if len(positive) > a.rows:
        raise ValueError("too many relevance-preserving passages for requested corpus")
    heap = []
    positives = {}
    seen = 0
    for batch in pq.ParquetFile(corpus).iter_batches(batch_size=32768):
        for r in batch.to_pylist():
            ident = str(r["_id"])
            text = ((r.get("title") or "") + "\n" + r["text"]).strip()
            if ident in positive:
                positives[ident] = text
            else:
                retain(
                    heap, 2 * (a.rows - len(positive) + a.training_rows), ident, text
                )
            seen += 1
        if seen % 1048576 == 0:
            print("sampled source rows", seen, flush=True)
    if set(positives) != positive:
        raise ValueError("relevant passage IDs missing")
    sampled = sorted(heap, key=lambda e: (-e[0], e[1]))
    # Preserve every labeled passage ID, including synonymous labels, but exclude
    # their text groups from calibration and random distractor selection.
    seen_text = {normalized_text(t) for t in positives.values()}
    unique = []
    for entry in sampled:
        fingerprint = normalized_text(entry[2])
        if fingerprint in seen_text:
            continue
        seen_text.add(fingerprint)
        unique.append(entry)
        if len(unique) == a.rows - len(positive) + a.training_rows:
            break
    if len(unique) != a.rows - len(positive) + a.training_rows:
        raise ValueError("insufficient distinct sampled passages")
    calibration = [(r[1], r[2]) for r in unique[: a.training_rows]]
    indexed = list(positives.items()) + [
        (r[1], r[2]) for r in unique[a.training_rows :]
    ]
    assert not {normalized_text(r[1]) for r in calibration} & {
        normalized_text(r[1]) for r in indexed
    }
    indexed.sort(key=lambda r: (key("msmarco-index-order-v1", r[0]), r[0]))
    assert len(indexed) == a.rows and not {r[0] for r in indexed} & {
        r[0] for r in calibration
    }
    groups = {
        "indexed": indexed,
        "calibration": calibration,
        "tuning": [(i, query_text[i]) for i in qids[: a.queries_per_split]],
        "test": [(i, query_text[i]) for i in qids[a.queries_per_split :]],
    }
    torch.set_num_threads(6)
    model = SentenceTransformer(
        "sentence-transformers/all-mpnet-base-v2", revision=MODEL, device=a.device
    )
    model.max_seq_length = 384
    descriptor = {
        "schema_version": 1,
        "name": f"msmarco-real-queries-{a.rows}-mpnet-768-v1",
        "dimension": 768,
        "provenance": {
            "dataset_repository": "BeIR/msmarco",
            "dataset_revision": DATA,
            "qrels_repository": "BeIR/msmarco-qrels",
            "qrels_revision": QRELS,
            "model_repository": "sentence-transformers/all-mpnet-base-v2",
            "model_revision": MODEL,
            "source_corpus_blake3": digest(corpus),
            "source_queries_blake3": digest(queries),
            "source_qrels_blake3": digest(qrels),
            "source_rows": seen,
            "device": a.device,
            "dependencies": {
                x: importlib.metadata.version(x)
                for x in [
                    "sentence-transformers",
                    "huggingface-hub",
                    "pyarrow",
                    "blake3",
                    "torch",
                    "numpy",
                ]
            },
            "preprocessing": "title + newline + passage; strip; mean pooling per pinned model; normalized FP32; max384 word pieces; truncation counts recorded",
            "sampling": "v1 BLAKE3-ranked development queries split tuning/test; all their labeled relevant passages plus deterministic sampled distractors; disjoint calibration document/text groups; case/whitespace-normalized exact duplicates excluded across splits; reservoir oversamples2x before text deduplication; experimental subset, not full MS MARCO score",
            "relevant_documents_for_queries": len(positive),
        },
    }
    for kind, records in groups.items():
        path = out / f"{kind}.f32"
        ids = out / f"{kind}.jsonl"
        started = time.monotonic()
        truncated = 0
        with path.open("wb") as f, ids.open("w") as textout:
            for first in range(0, len(records), 256):
                block = records[first : first + 256]
                texts = [r[1] for r in block]
                tokens = model.tokenizer(
                    texts, truncation=False, add_special_tokens=True, verbose=False
                )["input_ids"]
                truncated += sum(len(t) > 384 for t in tokens)
                values = model.encode(
                    texts,
                    batch_size=a.batch_size,
                    convert_to_numpy=True,
                    normalize_embeddings=True,
                    show_progress_bar=False,
                )
                values = np.ascontiguousarray(values, dtype="<f4")
                if values.shape != (len(block), 768) or not np.isfinite(values).all():
                    raise ValueError("bad embedding shape/values")
                f.write(values.tobytes())
                for ident, text in block:
                    textout.write(
                        json.dumps({"id": ident, "text": text}, ensure_ascii=False)
                        + "\n"
                    )
                if first == 0 or (first + len(block)) % 4096 == 0:
                    print(
                        kind,
                        first + len(block),
                        "/",
                        len(records),
                        "elapsed",
                        round(time.monotonic() - started, 2),
                        flush=True,
                    )
        descriptor[kind] = {
            "path": str(path),
            "row_count": len(records),
            "byte_len": path.stat().st_size,
            "blake3": digest(path),
            "records_path": str(ids),
            "records_blake3": digest(ids),
            "truncated_inputs": truncated,
            "embedding_seconds": time.monotonic() - started,
        }
        path.chmod(0o444)
        (out / "progress.json").write_text(json.dumps(descriptor, indent=2) + "\n")
    relpath = out / "qrels.jsonl"
    with relpath.open("w") as f:
        for r in rels:
            f.write(
                json.dumps(
                    {
                        "query_id": query_id(r),
                        "document_id": doc_id(r),
                        "relevance": int(r["score"]),
                    }
                )
                + "\n"
            )
    descriptor["qrels"] = {
        "path": str(relpath),
        "blake3": digest(relpath),
        "records": len(rels),
    }
    (out / "dataset.json").write_text(json.dumps(descriptor, indent=2) + "\n")
    print("COMPLETE", out / "dataset.json", flush=True)


if __name__ == "__main__":
    main()
