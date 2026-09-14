"""Paired query bootstrap, score errors and sparse-label metrics; no model fitting.
Run dataset and candidate-trace audits before summarizing the hash-pinned reports."""

import json, random, struct, sys
import blake3
from pathlib import Path


def recall_rows(d):
    return [q["budgets"][0]["delivered_matches"] / 10 for q in d["query_results"]]


def bootstrap(a, b):
    delta = [y - x for x, y in zip(a, b)]
    r = random.Random(20260804)
    n = len(delta)
    samples = sorted(
        sum(delta[r.randrange(n)] for _ in range(n)) / n for _ in range(10000)
    )
    return {
        "difference": sum(delta) / n,
        "paired_query_bootstrap_95_percent": [samples[249], samples[9749]],
        "improved_queries": sum(x > 0 for x in delta),
        "worsened_queries": sum(x < 0 for x in delta),
        "unchanged_queries": sum(x == 0 for x in delta),
    }


def oracle_rows(path):
    report = json.load(open(path))
    raw = Path(report["artifact_path"]).read_bytes()
    assert blake3.blake3(raw).hexdigest() == report["artifact_blake3"]
    assert raw[:8] == b"SPHROR01"
    rows, n, k = struct.unpack_from("<QII", raw, 8)
    pos = 24
    result = []
    for _ in range(n):
        count = struct.unpack_from("<I", raw, pos)[0]
        pos += 4
        hits = []
        for j in range(count):
            row, score = struct.unpack_from("<Qd", raw, pos)
            pos += 16
            if j < 10:
                hits.append(row)
        result.append(hits)
    assert pos == len(raw)
    return result


def relevance(d, oracle_path):
    desc = d["source"]["descriptor"]
    split = d["source"]["query_split"]
    documents = [json.loads(x)["id"] for x in open(desc["indexed"]["records_path"])]
    queries = [json.loads(x)["id"] for x in open(desc[split]["records_path"])][
        : d["query_count"]
    ]
    rel = {q: set() for q in queries}
    for line in open(desc["qrels"]["path"]):
        x = json.loads(line)
        if x["query_id"] in rel and x["relevance"] > 0:
            rel[x["query_id"]].add(x["document_id"])

    def evaluate(rankings):
        scores = []
        for q, ranked in zip(queries, rankings):
            positions = [i + 1 for i, r in enumerate(ranked) if documents[r] in rel[q]]
            scores.append(1 / min(positions) if positions else 0)
        return {
            "mrr_at_10": sum(scores) / len(scores),
            "labeled_hit_at_10": sum(x > 0 for x in scores) / len(scores),
        }

    return {
        "compressed": evaluate(
            [[h["row"] for h in q["budgets"][0]["hits"]] for q in d["query_results"]]
        ),
        "exact": evaluate(oracle_rows(oracle_path)),
    }


def percentile(values, p):
    s = sorted(values)
    return s[(p * len(s) + 99) // 100 - 1]


def score_errors(d):
    oracle = json.load(open(d["oracle_reference"]["report_path"]))
    raw = Path(oracle["artifact_path"]).read_bytes()
    n = struct.unpack_from("<I", raw, 16)[0]
    pos = 24
    gaps = []
    for _ in range(n):
        k = struct.unpack_from("<I", raw, pos)[0]
        pos += 4
        scores = [struct.unpack_from("<Qd", raw, pos + 16 * j)[1] for j in range(k)]
        pos += 16 * k
        gaps.append(scores[9] - scores[10])
    errors = [
        abs(v["refined_error"])
        for q in d["query_results"]
        for v in q["budgets"][0]["neighbors"]
        if v["refined_error"] is not None
    ]
    return {
        "exact_top10_score_error_abs_p50": percentile(errors, 50),
        "exact_top10_score_error_abs_p95": percentile(errors, 95),
        "exact_10_to_11_gap_p50": percentile(gaps, 50),
        "exact_10_to_11_gap_p10": percentile(gaps, 10),
        "max_fixed_vs_float_error": max(
            q["budgets"][0]["maximum_fixed_point_error"] for q in d["query_results"]
        ),
    }


if __name__ == "__main__":
    ds = [json.load(open(p)) for p in sys.argv[1:]]
    base = ds[0]
    for d in ds:
        assert (
            d["query_hash"] == base["query_hash"]
            and d["corpus_hash"] == base["corpus_hash"]
        )
        assert (
            len(d["query_results"]) == len(base["query_results"]) and d["checks_passed"]
        )
        assert not d["dirty_worktree"] and not d["build_dirty_worktree"]
    print(
        json.dumps(
            [
                {
                    "path": p,
                    "training_inputs": d["training_rows"],
                    "summaries": d["summaries"],
                    "score_errors": score_errors(d),
                    "vs_baseline": bootstrap(recall_rows(base), recall_rows(d)),
                    "recall_bootstrap_95_percent": bootstrap(
                        [0] * len(d["query_results"]), recall_rows(d)
                    )["paired_query_bootstrap_95_percent"],
                    "relevance": relevance(d, d["oracle_reference"]["report_path"])
                    if d["source"].get("kind") == "real-query"
                    else None,
                    "max_exact_neighbor_primary_rank": max(
                        n["primary_rank"]
                        for q in d["query_results"]
                        for n in q["budgets"][0]["neighbors"]
                    ),
                }
                for p, d in zip(sys.argv[1:], ds)
            ],
            indent=2,
        )
    )
