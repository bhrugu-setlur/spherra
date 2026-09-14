import json, sys, math
from pathlib import Path
import blake3

for path in sys.argv[1:]:
    d = json.load(open(path))
    assert not d["dirty_worktree"] and not d["build_dirty_worktree"]
    assert d["checks_passed"]
    trace = d["trace"]
    h = blake3.blake3()
    size = 0
    lines = []
    with open(trace["path"], "rb") as f:
        for line in f:
            h.update(line)
            size += len(line)
            lines.append(json.loads(line))
    assert h.hexdigest() == trace["blake3"] and size == trace["bytes"]
    assert len(lines) == len(d["query_results"]) == d["query_count"]
    totals = [
        dict(
            candidate_matches=0,
            delivered_matches=0,
            selection_losses=0,
            ranking_losses=0,
            floating_matches=0,
            floating_top10_differences=0,
        )
        for _ in d["summaries"]
    ]
    for q, t in zip(d["query_results"], lines):
        assert q["query"] == t["query"]
        cs = t["candidates"]
        assert len({c[0] for c in cs}) == len(cs)
        assert all(c[1] == i + 1 for i, c in enumerate(cs))
        assert sorted(cs, key=lambda c: (-c[2], c[0])) == cs
        previous = 0
        for i, b in enumerate(q["budgets"]):
            pool = cs[: b["budget"]]
            truth = {n["row"] for n in b["neighbors"]}
            refined = sorted(pool, key=lambda c: (-c[3], c[0]))
            precise = sorted(pool, key=lambda c: (-c[5], c[0]))
            floating = sorted(pool, key=lambda c: (-c[4], c[0]))
            covered = len(truth & {c[0] for c in pool})
            delivered = len(truth & {c[0] for c in refined[:10]})
            assert (
                covered
                == b["candidate_matches"]
                == b["exact_rerank_matches"]
                == len(truth & {c[0] for c in precise[:10]})
            )
            assert covered >= previous
            previous = covered
            assert delivered == b["delivered_matches"]
            assert 10 - covered == b["selection_losses"]
            assert covered - delivered == b["ranking_losses"]
            assert b["floating_matches"] == len(truth & {c[0] for c in floating[:10]})
            assert b["floating_top10_differences"] == sum(
                a[0] != b[0] for a, b in zip(floating[:10], refined[:10])
            )
            for h, c in zip(b["hits"], refined):
                assert h["row"] == c[0] and h["raw"] == h["public_raw"] == c[3]
                assert h["truth"] == c[5]
                assert h["lower"] <= c[5] <= h["upper"]
            for n in b["neighbors"]:
                found = next(
                    (j for j, c in enumerate(refined) if c[0] == n["row"]), None
                )
                assert n["refined_rank"] == (None if found is None else found + 1)
                assert n["loss"] == (
                    "selection"
                    if found is None
                    else "ranking"
                    if found >= 10
                    else "returned"
                )
            for key in totals[i]:
                totals[i][key] += b[key]
    for s, t in zip(d["summaries"], totals):
        assert s["candidate_recall_at_10"] == t["candidate_matches"] / (
            10 * d["query_count"]
        )
        assert s["recall_at_10"] == t["delivered_matches"] / (10 * d["query_count"])
        assert s["floating_recall_at_10"] == t["floating_matches"] / (
            10 * d["query_count"]
        )
        for k in ["selection_losses", "ranking_losses", "floating_top10_differences"]:
            assert s[k] == t[k]
    print(
        Path(path).name,
        "verified",
        d["query_count"],
        "queries",
        sum(len(t["candidates"]) for t in lines),
        "trace candidates",
    )
