//! Small, hash-pinned dot-product retrieval qualification over external factors.
use super::{BenchError, Options, write_output};
use crate::local_index::{allowed, number};
use serde_json::json;
use spherra::{CreateOptions, Index, IndexBuilder, SearchOptions, Vector};
use spherra_testkit::{
    machine::{MachineProfile, SourceRevision, timestamp_rfc3339_utc},
    results::validate_against_schema,
};
use std::{fs, io::Read, path::Path, time::Instant};

fn rows(path: &str, count: usize, hash: &str) -> Result<Vec<Vector>, BenchError> {
    let mut file = fs::File::open(path).map_err(BenchError::harness)?;
    if file.metadata().map_err(BenchError::harness)?.len() != count as u64 * 768 * 4 {
        return Err(BenchError::harness("vector byte length mismatch"));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(count as u64 * 768 * 4 + 1)
        .read_to_end(&mut bytes)
        .map_err(BenchError::harness)?;
    if bytes.len() != count * 768 * 4 || blake3::hash(&bytes).to_hex().as_str() != hash {
        return Err(BenchError::harness("vector hash/length mismatch"));
    }
    let rows: Vec<Vector> = bytes
        .chunks_exact(768 * 4)
        .map(|row| {
            std::array::from_fn(|i| {
                f32::from_le_bytes(row[i * 4..i * 4 + 4].try_into().expect("four bytes"))
            })
        })
        .collect();
    for row in &rows {
        if spherra_domain::ValidatedVector::new(row.to_vec())
            .map_err(BenchError::harness)?
            .direction_unreliable()
        {
            return Err(BenchError::harness("unreliable input vector"));
        }
    }
    Ok(rows)
}
fn truth(q: &Vector, x: &Vector) -> f64 {
    q.iter()
        .zip(x)
        .fold(0.0, |s, (&q, &x)| f64::from(q).mul_add(f64::from(x), s))
}
pub(super) fn run(o: &Options) -> Result<(), BenchError> {
    allowed(
        o,
        &[
            "indexed",
            "rows",
            "indexed-blake3",
            "training",
            "training-rows",
            "training-blake3",
            "queries",
            "query-count",
            "queries-blake3",
            "index-dir",
            "output",
            "seed",
            "k",
            "candidate-budget",
        ],
    )?;
    let n = number(o, "rows", 0_usize)?;
    let training_count = number(o, "training-rows", 0_usize)?;
    let query_count = number(o, "query-count", 0_usize)?;
    let k = number(o, "k", 10_usize)?;
    let budget = number(o, "candidate-budget", 200_usize)?;
    let seed = number(o, "seed", 20260804_u64)?;
    if !(1..=20000).contains(&n)
        || !(342..=32768).contains(&training_count)
        || !(1..=2000).contains(&query_count)
        || k == 0
        || budget < k
    {
        return Err(BenchError::harness(
            "invalid or oversized qualification workload",
        ));
    }
    let output = Path::new(o.require("output")?);
    if output.exists() {
        return Err(BenchError::harness("output already exists"));
    }
    let source = SourceRevision::capture();
    let indexed = rows(o.require("indexed")?, n, o.require("indexed-blake3")?)?;
    let training = rows(
        o.require("training")?,
        training_count,
        o.require("training-blake3")?,
    )?;
    let queries = rows(
        o.require("queries")?,
        query_count,
        o.require("queries-blake3")?,
    )?;
    let dir = Path::new(o.require("index-dir")?);
    let build = Instant::now();
    let mut builder = IndexBuilder::create(
        dir,
        &training,
        CreateOptions {
            seed,
            validation_rows: None,
        },
    )
    .map_err(BenchError::harness)?;
    for row in &indexed {
        builder.push(row).map_err(BenchError::harness)?;
    }
    builder.commit().map_err(BenchError::harness)?;
    let build_seconds = build.elapsed().as_secs_f64();
    let index = Index::open(dir).map_err(BenchError::harness)?;
    let current = blake3::hash(&fs::read(dir.join("CURRENT")).map_err(BenchError::harness)?)
        .to_hex()
        .to_string();
    let mut records = Vec::new();
    let (mut dot_found, mut cosine_found, mut violations) = (0_usize, 0_usize, 0_usize);
    for (ordinal, q) in queries.iter().enumerate() {
        let mut exact: Vec<_> = indexed
            .iter()
            .enumerate()
            .map(|(i, x)| (i, truth(q, x)))
            .collect();
        exact.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let exact_rows: Vec<_> = exact[..k.min(n)].iter().map(|x| x.0 as u64).collect();
        let start = Instant::now();
        let dot = index
            .search_dot_product(
                q,
                SearchOptions {
                    k,
                    candidate_budget: Some(budget),
                },
            )
            .map_err(BenchError::harness)?;
        let dot_ms = start.elapsed().as_secs_f64() * 1000.0;
        let start = Instant::now();
        let cosine = index
            .search(
                q,
                SearchOptions {
                    k,
                    candidate_budget: Some(budget),
                },
            )
            .map_err(BenchError::harness)?;
        let cosine_ms = start.elapsed().as_secs_f64() * 1000.0;
        if dot.rows_scanned() != n as u64
            || dot.rows_refined() != budget.min(n) as u64
            || dot.hits().len() != k.min(n)
        {
            return Err(BenchError::harness("dot search workload mismatch"));
        }
        let mut hits = Vec::new();
        let mut found = 0;
        for hit in dot.hits() {
            let r = hit.row().get();
            let expected = truth(q, &indexed[r as usize]);
            if !(hit.interval().0 <= expected && expected <= hit.interval().1) {
                violations += 1;
            }
            if exact_rows.contains(&r) {
                found += 1;
            }
            hits.push(json!({"row":r,"score":hit.score(),"lower":hit.interval().0,"upper":hit.interval().1,"truth":expected,"stored_magnitude":hit.stored_magnitude()}));
        }
        let cosine_rows: Vec<_> = cosine.hits().iter().map(|h| h.row().get()).collect();
        let cosine_matches = cosine_rows
            .iter()
            .filter(|r| exact_rows.contains(r))
            .count();
        dot_found += found;
        cosine_found += cosine_matches;
        records.push(json!({"query":ordinal,"exact_rows":exact_rows,"dot_hits":hits,"cosine_rows":cosine_rows,"dot_matches":found,"cosine_matches":cosine_matches,"dot_ms":dot_ms,"cosine_ms":cosine_ms}));
    }
    let end = SourceRevision::capture();
    let value = json!({"schema_version":1,"kind":"dot-product","timestamp":timestamp_rfc3339_utc(),
        "git_commit":source.commit,"dirty_worktree":source.dirty || end.dirty || source.commit!=end.commit,
        "machine":MachineProfile::capture(),"command":format!("spherra-bench dot-product {}",o.0.iter().map(|(k,v)|format!("--{k} {v}")).collect::<Vec<_>>().join(" ")),
        "indexed_blake3":o.require("indexed-blake3")?,"training_blake3":o.require("training-blake3")?,"queries_blake3":o.require("queries-blake3")?,
        "index_current_blake3":current,"rows":n,"training_rows":training_count,"query_count":query_count,"seed":seed,"k":k,"candidate_budget":budget.min(n),
        "build_seconds":build_seconds,"dot_recall_at_k":dot_found as f64/(query_count*k.min(n)) as f64,
        "cosine_recall_against_dot_at_k":cosine_found as f64/(query_count*k.min(n)) as f64,"enclosure_failures":violations,"queries":records});
    let schema = serde_json::from_str(include_str!(
        "../../../docs/benchmarks/dot-product.schema.json"
    ))
    .map_err(BenchError::serialize)?;
    validate_against_schema(&schema, &value).map_err(BenchError::harness)?;
    write_output(output, &value)?;
    if violations != 0 {
        return Err(BenchError::harness(
            "dot product enclosure failure; report saved",
        ));
    }
    Ok(())
}
