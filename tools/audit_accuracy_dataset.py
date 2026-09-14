"""Checks actual published dataset separation and data/label provenance."""

import json, sys
from pathlib import Path
import blake3

p = Path(sys.argv[1])
d = json.loads(p.read_text())


def digest(path):
    h = blake3.blake3()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def norm(t):
    return " ".join(t.casefold().split())


records = {}
for name in ["indexed", "calibration", "tuning", "test"]:
    v = d[name]
    assert digest(v["path"]) == v["blake3"]
    assert Path(v["path"]).stat().st_size == v["row_count"] * 768 * 4 == v["byte_len"]
    assert digest(v["records_path"]) == v["records_blake3"]
    records[name] = [json.loads(l) for l in open(v["records_path"])]
    assert len(records[name]) == v["row_count"]
    assert len({r["id"] for r in records[name]}) == len(records[name])
for a, b in [("indexed", "calibration"), ("tuning", "test")]:
    assert not {r["id"] for r in records[a]} & {r["id"] for r in records[b]}
    assert not {norm(r["text"]) for r in records[a]} & {
        norm(r["text"]) for r in records[b]
    }
for query_split in ["tuning", "test"]:
    for passage_split in ["indexed", "calibration"]:
        assert not {norm(r["text"]) for r in records[query_split]} & {
            norm(r["text"]) for r in records[passage_split]
        }

rels = [json.loads(l) for l in open(d["qrels"]["path"])]
assert len(rels) == d["qrels"]["records"]
assert digest(d["qrels"]["path"]) == d["qrels"]["blake3"]
ids = {r["id"] for r in records["indexed"]}
queries = {r["id"] for n in ["tuning", "test"] for r in records[n]}
assert all(r["document_id"] in ids and r["query_id"] in queries for r in rels)
assert {r["query_id"] for r in rels} == queries
print(
    "All split lengths/hashes, ID uniqueness, cross-split document/text disjointness and relevance coverage pass."
)
print("Truncated inputs:", {name: d[name]["truncated_inputs"] for name in records})
