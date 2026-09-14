//! Hash-pinned dot-product retrieval qualification over external vectors.
use super::{BenchError, Options, write_output};
use crate::index_quality::{Reference, reconstruction_length, stored_corrections};
use crate::local_index::{allowed, number};
use serde_json::json;
use spherra::{CreateOptions, Index, IndexBuilder, SearchOptions, Vector};
use spherra_codec::{FixedPointScorer, Pq96Code};
use spherra_testkit::{
    machine::{MachineProfile, SourceRevision, timestamp_rfc3339_utc},
    results::validate_against_schema,
};
use std::{fs, io::Read, path::Path, time::Instant};

const MAX_ROWS: usize = 2_000_000;
const EXACT_THREADS: usize = 6;

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
fn better(a: &(f64, usize), b: &(f64, usize)) -> std::cmp::Ordering {
    b.0.total_cmp(&a.0).then(a.1.cmp(&b.1))
}
/// Exhaustive FP64 top-k by descending dot product, then ascending row. Each
/// thread keeps its local top-k, so the merged order equals one full sort.
fn exact_rows(q: &Vector, indexed: &[Vector], k: usize) -> Vec<u64> {
    let chunk = indexed.len().div_ceil(EXACT_THREADS).max(1);
    let mut best: Vec<(f64, usize)> = std::thread::scope(|scope| {
        let handles: Vec<_> = indexed
            .chunks(chunk)
            .enumerate()
            .map(|(c, part)| {
                scope.spawn(move || {
                    let mut scored: Vec<_> = part
                        .iter()
                        .enumerate()
                        .map(|(i, x)| (truth(q, x), c * chunk + i))
                        .collect();
                    if scored.len() > k {
                        scored.select_nth_unstable_by(k - 1, better);
                        scored.truncate(k);
                    }
                    scored
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("exact scan thread"))
            .collect()
    });
    best.sort_unstable_by(better);
    best.truncate(k);
    best.into_iter().map(|x| x.1 as u64).collect()
}
/// Experiment only: rescore the public dot candidate pool, times the stored
/// length. Option 1 divides dot(T(q), p + e) by |p + e|; the option 2
/// simulations divide by the build-time alignment (exact, FP16, one byte).
/// Returns top-k rows for [option 1, exact, FP16, one byte] and the minimum
/// alignment ratio. The common query norm does not affect order.
fn corrected_rows(
    index: &Index,
    reference: &Reference,
    indexed: &[Vector],
    q: &Vector,
    k: usize,
    budget: usize,
) -> Result<([Vec<u64>; 4], f64), BenchError> {
    let pool = index
        .search_dot_product(
            q,
            SearchOptions {
                k: budget.min(index.len() as usize),
                candidate_budget: Some(budget),
            },
        )
        .map_err(BenchError::harness)?;
    let prepared = FixedPointScorer::new()
        .prepare_query(&reference.plan, q, &reference.table, &reference.book)
        .map_err(BenchError::harness)?;
    let scorer = FixedPointScorer::new();
    let mut scored: [Vec<(f64, usize)>; 4] = Default::default();
    let mut minimum = f64::INFINITY;
    let qnorm = truth(q, q).sqrt();
    let mut previous: Option<(f64, usize)> = None;
    for hit in pool.hits() {
        let row = hit.row().get() as usize;
        let segment =
            &reference.segments[reference.segments.partition_point(|s| s.first <= row) - 1];
        let code = Pq96Code::from_bytes(
            segment
                .residual
                .residual_code((row - segment.first) as u32)
                .map_err(BenchError::harness)?,
        );
        let p = reference.table.decode(&reference.codes[row]);
        let e = reference.book.decode(&code);
        let dot = (0..768)
            .map(|c| f64::from(prepared.transformed()[c]) * (f64::from(p[c]) + f64::from(e[c])))
            .sum::<f64>();
        let length = reconstruction_length(&p, &e);
        let magnitude = f64::from(hit.stored_magnitude());
        let raw = scorer
            .score_refined(&prepared, &reference.codes[row], &code)
            .raw();
        let corrected = if length.is_normal() {
            raw as f64 / length
        } else {
            raw as f64
        };
        let key = if magnitude == 0.0 {
            0.0
        } else {
            corrected * (magnitude * 16777216.0)
        };
        let expected = (key / 281474976710656.0) * qnorm;
        if hit.score().to_bits() != expected.to_bits()
            || previous.is_some_and(|p| better(&p, &(key, row)).is_gt())
        {
            return Err(BenchError::harness(
                "corrected dot scalar score/order disagreement",
            ));
        }
        previous = Some((key, row));
        let original = scorer
            .prepare_query(
                &reference.plan,
                &indexed[row],
                &reference.table,
                &reference.book,
            )
            .map_err(BenchError::harness)?;
        let (stored, ratio) = stored_corrections(dot, original.transformed(), &p, &e, length);
        minimum = minimum.min(ratio);
        scored[0].push((dot / length * magnitude, row));
        for (i, score) in stored.into_iter().enumerate() {
            scored[i + 1].push((score * magnitude, row));
        }
    }
    let rows = scored.map(|mut s| {
        s.sort_unstable_by(better);
        s.into_iter().take(k).map(|x| x.1 as u64).collect()
    });
    Ok((rows, minimum))
}
fn sweep_budgets(o: &Options, k: usize) -> Result<Vec<usize>, BenchError> {
    let Some(list) = o.get("sweep-budgets") else {
        return Ok(Vec::new());
    };
    let budgets = list
        .split(',')
        .map(|b| {
            b.parse::<usize>()
                .map_err(|_| BenchError::UnparsableOption("sweep-budgets".to_owned()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if budgets.is_empty() || budgets.iter().any(|&b| b < k) {
        return Err(BenchError::harness("every sweep budget must be at least k"));
    }
    Ok(budgets)
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
            "sweep-budgets",
        ],
    )?;
    let n = number(o, "rows", 0_usize)?;
    let training_count = number(o, "training-rows", 0_usize)?;
    let query_count = number(o, "query-count", 0_usize)?;
    let k = number(o, "k", 10_usize)?;
    let budget = number(o, "candidate-budget", 200_usize)?;
    let seed = number(o, "seed", 20260804_u64)?;
    if !(1..=MAX_ROWS).contains(&n)
        || !(342..=32768).contains(&training_count)
        || !(1..=2000).contains(&query_count)
        || k == 0
        || budget < k
    {
        return Err(BenchError::harness(
            "invalid or oversized qualification workload",
        ));
    }
    let sweep = sweep_budgets(o, k)?;
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
    drop(training);
    for (i, row) in indexed.iter().enumerate() {
        builder.push(row).map_err(BenchError::harness)?;
        if (i + 1) % 65536 == 0 {
            eprintln!("build: {} rows staged", i + 1);
        }
    }
    builder.commit().map_err(BenchError::harness)?;
    let build_seconds = build.elapsed().as_secs_f64();
    let index = Index::open(dir).map_err(BenchError::harness)?;
    let current = blake3::hash(&fs::read(dir.join("CURRENT")).map_err(BenchError::harness)?)
        .to_hex()
        .to_string();
    let mut records = Vec::new();
    let (mut dot_found, mut cosine_found, mut violations) = (0_usize, 0_usize, 0_usize);
    let mut sweep_found = vec![0_usize; sweep.len()];
    let reference = Reference::open(dir, seed, &index)?;
    let mut corrected_found = [0_usize; 4];
    let mut minimum_alignment = f64::INFINITY;
    for (ordinal, q) in queries.iter().enumerate() {
        let exact_rows = exact_rows(q, &indexed, k.min(n));
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
        for (&b, total) in sweep.iter().zip(&mut sweep_found) {
            let result = index
                .search_dot_product(
                    q,
                    SearchOptions {
                        k,
                        candidate_budget: Some(b),
                    },
                )
                .map_err(BenchError::harness)?;
            *total += result
                .hits()
                .iter()
                .filter(|h| exact_rows.contains(&h.row().get()))
                .count();
        }
        let (corrected, ratio) = corrected_rows(&index, &reference, &indexed, q, k, budget)?;
        minimum_alignment = minimum_alignment.min(ratio);
        for (rows, total) in corrected.iter().zip(&mut corrected_found) {
            *total += rows.iter().filter(|r| exact_rows.contains(r)).count();
        }
        dot_found += found;
        cosine_found += cosine_matches;
        records.push(json!({"query":ordinal,"exact_rows":exact_rows,"dot_hits":hits,"cosine_rows":cosine_rows,"dot_matches":found,"cosine_matches":cosine_matches,"dot_ms":dot_ms,"cosine_ms":cosine_ms}));
        if (ordinal + 1) % 50 == 0 {
            eprintln!("query: {}/{query_count} complete", ordinal + 1);
        }
    }
    let end = SourceRevision::capture();
    let checked = (query_count * k.min(n)) as f64;
    let mut value = json!({"schema_version":2,"kind":"dot-product","timestamp":timestamp_rfc3339_utc(),
        "git_commit":source.commit,"dirty_worktree":source.dirty || end.dirty || source.commit!=end.commit,
        "machine":MachineProfile::capture(),"command":format!("spherra-bench dot-product {}",o.0.iter().map(|(k,v)|format!("--{k} {v}")).collect::<Vec<_>>().join(" ")),
        "indexed_blake3":o.require("indexed-blake3")?,"training_blake3":o.require("training-blake3")?,"queries_blake3":o.require("queries-blake3")?,
        "index_current_blake3":current,"rows":n,"training_rows":training_count,"query_count":query_count,"seed":seed,"k":k,"candidate_budget":budget.min(n),
        "build_seconds":build_seconds,"dot_recall_at_k":dot_found as f64/checked,
        "cosine_recall_against_dot_at_k":cosine_found as f64/checked,"renormalized_dot_recall_at_k":corrected_found[0] as f64/checked,"stored_exact_dot_recall_at_k":corrected_found[1] as f64/checked,"stored_fp16_dot_recall_at_k":corrected_found[2] as f64/checked,"stored_u8_dot_recall_at_k":corrected_found[3] as f64/checked,"minimum_alignment":minimum_alignment,"enclosure_failures":violations,"queries":records});
    if !sweep.is_empty() {
        value["budget_sweep"] = sweep
            .iter()
            .zip(&sweep_found)
            .map(|(&b, &found)| json!({"candidate_budget":b.min(n),"dot_recall_at_k":found as f64/checked}))
            .collect();
    }
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
