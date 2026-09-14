//! Bench-only loss attribution. No serving API, stored format or scoring changes.
use super::*;
use std::io::{BufWriter, Write};

#[derive(Clone)]
struct Candidate {
    row: usize,
    primary_rank: usize,
    primary_raw: i64,
    raw: i64,
    floating: f64,
    // Experiment only: `floating` divided by the reconstruction length |p + e|.
    renormalized: f64,
    // Experiment only: option 2 simulations, see `stored_corrections`.
    stored: [f64; 3],
    alignment: f64,
    truth: f64,
}
fn overlap(rows: impl Iterator<Item = usize>, exact: &[Neighbor]) -> u64 {
    rows.filter(|row| exact.iter().take(10).any(|n| n.row as usize == *row))
        .count() as u64
}
fn raw_score(score: f64) -> Result<i64, BenchError> {
    let raw = score * (1_u64 << 24) as f64;
    if !raw.is_finite() || raw.abs() > (1_u64 << 53) as f64 || raw.fract() != 0.0 {
        return Err(BenchError::harness(
            "public score is not an exact Q24 integer",
        ));
    }
    Ok(raw as i64)
}
/// FP64 length of the reconstruction p + e, as used by the renormalization experiment.
pub(crate) fn reconstruction_length(p: &[f32; 768], e: &[f32; 768]) -> f64 {
    p.iter()
        .zip(e)
        .map(|(&p, &e)| {
            let v = f64::from(p) + f64::from(e);
            v * v
        })
        .sum::<f64>()
        .sqrt()
}
/// Floor of the simulated one-byte alignment-ratio code.
const U8_ALIGNMENT_FLOOR: f64 = 0.5;
/// Nearest FP16 value for a positive normal-range input (1024 steps per power of two).
fn nearest_half(x: f64) -> f64 {
    let step = 2.0_f64.powi(x.log2().floor() as i32 - 10);
    (x / step).round() * step
}
/// Option 2 simulation. `alignment` is dot(T(normalize(original)), p + e), which
/// only the builder knows. Returns the score divided by: the exact alignment;
/// the alignment stored as FP16 (2 bytes); and |p + e| times a one-byte code of
/// alignment / |p + e| over [0.5, 1] (1 byte). Also returns that ratio.
pub(crate) fn stored_corrections(
    score: f64,
    original: &[f32; 768],
    p: &[f32; 768],
    e: &[f32; 768],
    length: f64,
) -> ([f64; 3], f64) {
    let alignment = (0..768)
        .map(|c| f64::from(original[c]) * (f64::from(p[c]) + f64::from(e[c])))
        .sum::<f64>();
    let ratio = alignment / length;
    let span = 1.0 - U8_ALIGNMENT_FLOOR;
    let code = ((ratio.clamp(U8_ALIGNMENT_FLOOR, 1.0) - U8_ALIGNMENT_FLOOR) / span * 255.0).round();
    let byte = (U8_ALIGNMENT_FLOOR + code / 255.0 * span) * length;
    (
        [
            score / alignment,
            score / nearest_half(alignment),
            score / byte,
        ],
        ratio,
    )
}
fn require(condition: bool, message: &str) -> Result<(), BenchError> {
    if condition {
        Ok(())
    } else {
        Err(BenchError::harness(message))
    }
}
fn analyze(
    index: &Index,
    reference: &Reference,
    rows: &[Vector],
    raw: &Vector,
    exact: &[Neighbor],
    budgets: &[usize],
    ordinal: usize,
) -> Result<(Value, Value), BenchError> {
    let scorer = FixedPointScorer::new();
    let query = scorer
        .prepare_query(&reference.plan, raw, &reference.table, &reference.book)
        .map_err(BenchError::harness)?;
    let scale = query.lookup_scale_measurement();
    require(
        i128::from(scale.maximum_primary_lookup_entry()) * 768
            + i128::from(scale.maximum_residual_lookup_entry()) * 96
            <= (1_i128 << 53),
        "raw reference scores exceed exact public conversion range",
    )?;
    let normalized = normalize_fp64(raw).map_err(BenchError::harness)?;
    let mut ranked: Vec<_> = reference
        .codes
        .iter()
        .enumerate()
        .map(|(row, code)| (row, scorer.score_primary(&query, code).raw()))
        .collect();
    ranked.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut true_ranks = [0; 10];
    let mut true_primary = [0; 10];
    for (rank, (row, score)) in ranked.iter().enumerate() {
        if let Some(i) = exact.iter().take(10).position(|n| n.row as usize == *row) {
            true_ranks[i] = rank + 1;
            true_primary[i] = *score;
        }
    }
    require(
        true_ranks.iter().all(|r| *r > 0),
        "oracle neighbors are missing from physical index",
    )?;
    let maximum = budgets.iter().copied().max().unwrap().min(rows.len());
    let mut candidates = Vec::with_capacity(maximum);
    for (rank, (row, primary_raw)) in ranked.into_iter().take(maximum).enumerate() {
        let segment =
            &reference.segments[reference.segments.partition_point(|s| s.first <= row) - 1];
        let code = Pq96Code::from_bytes(
            segment
                .residual
                .residual_code((row - segment.first) as u32)
                .map_err(BenchError::harness)?,
        );
        let refined = scorer
            .score_refined(&query, &reference.codes[row], &code)
            .raw();
        let p = reference.table.decode(&reference.codes[row]);
        let e = reference.book.decode(&code);
        // FP64 products of the exact serving inputs, with no reconstructed-vector
        // renormalization. Difference from Q24 isolates lookup rounding/reduction.
        let floating = (0..768)
            .map(|c| f64::from(query.transformed()[c]) * (f64::from(p[c]) + f64::from(e[c])))
            .sum::<f64>();
        let length = reconstruction_length(&p, &e);
        let renormalized = floating / length;
        let original = scorer
            .prepare_query(
                &reference.plan,
                &rows[row],
                &reference.table,
                &reference.book,
            )
            .map_err(BenchError::harness)?;
        let (stored, alignment) =
            stored_corrections(floating, original.transformed(), &p, &e, length);
        let truth = dot_f64(
            &normalized,
            &normalize_fp64(&rows[row]).map_err(BenchError::harness)?,
        );
        candidates.push(Candidate {
            row,
            primary_rank: rank + 1,
            primary_raw,
            raw: refined,
            floating,
            renormalized,
            stored,
            alignment,
            truth,
        });
    }
    // All candidates appear once in a hash-bound compact trace, rather than
    // duplicating a large pool in every budget's human-readable report.
    let trace = json!({"query":ordinal,"columns":["row","primary_rank","primary_raw","refined_raw","floating_refined","truth"],"candidates":candidates.iter().map(|c|json!([c.row,c.primary_rank,c.primary_raw,c.raw,c.floating,c.truth])).collect::<Vec<_>>()});
    let mut reports = Vec::new();
    for &requested in budgets {
        let budget = requested.min(rows.len());
        let pool = &candidates[..budget];
        let mut refined: Vec<_> = pool.iter().collect();
        refined.sort_unstable_by(|a, b| b.raw.cmp(&a.raw).then_with(|| a.row.cmp(&b.row)));
        let mut precise = refined.clone();
        precise
            .sort_unstable_by(|a, b| b.truth.total_cmp(&a.truth).then_with(|| a.row.cmp(&b.row)));
        let mut floating = refined.clone();
        floating.sort_unstable_by(|a, b| {
            b.floating
                .total_cmp(&a.floating)
                .then_with(|| a.row.cmp(&b.row))
        });
        let mut renormalized = refined.clone();
        renormalized.sort_unstable_by(|a, b| {
            b.renormalized
                .total_cmp(&a.renormalized)
                .then_with(|| a.row.cmp(&b.row))
        });
        let stored_matches = (0..3)
            .map(|i| {
                let mut order = refined.clone();
                order.sort_unstable_by(|a, b| {
                    b.stored[i]
                        .total_cmp(&a.stored[i])
                        .then_with(|| a.row.cmp(&b.row))
                });
                overlap(order.iter().take(10).map(|c| c.row), exact)
            })
            .collect::<Vec<_>>();
        let result = index
            .search(
                raw,
                SearchOptions {
                    k: budget,
                    candidate_budget: Some(requested),
                },
            )
            .map_err(BenchError::harness)?;
        let normal = index
            .search(
                raw,
                SearchOptions {
                    k: 10,
                    candidate_budget: Some(requested),
                },
            )
            .map_err(BenchError::harness)?;
        for (r, k) in [(&result, budget), (&normal, 10)] {
            require(
                r.hits().len() == k
                    && r.rows_scanned() == rows.len() as u64
                    && r.rows_refined() == budget as u64
                    && r.generation() == index.generation(),
                "public diagnostic workload differs",
            )?;
            for (hit, c) in r.hits().iter().zip(&refined) {
                require(
                    hit.row().get() == c.row as u64 && raw_score(hit.score())? == c.raw,
                    "public/scalar candidate row or score disagreement",
                )?;
                let (lower, upper) = hit.interval();
                require(
                    lower.is_finite() && upper.is_finite() && lower <= c.truth && c.truth <= upper,
                    "candidate certificate enclosure failure",
                )?;
            }
        }
        let covered = overlap(pool.iter().map(|c| c.row), exact);
        let delivered = overlap(refined.iter().take(10).map(|c| c.row), exact);
        let exact_matches = overlap(precise.iter().take(10).map(|c| c.row), exact);
        require(
            exact_matches == covered && delivered <= covered,
            "inconsistent neighbor-loss accounting",
        )?;
        let neighbors=exact.iter().take(10).enumerate().map(|(i,n)|{
            let r=refined.iter().position(|c|c.row==n.row as usize);
            let c=r.map(|j|refined[j]);
            json!({"row":n.row,"exact_rank":i+1,"truth":n.score,"primary_rank":true_ranks[i],"primary_raw":true_primary[i],"primary_error":true_primary[i] as f64/(1_u64<<24) as f64-n.score,"refined_rank":r.map(|j|j+1),"refined_raw":c.map(|c|c.raw),"refined_error":c.map(|c|c.raw as f64/(1_u64<<24) as f64-n.score),"loss":if true_ranks[i]>budget {"selection"} else if r.unwrap()>=10 {"ranking"} else {"returned"}})
        }).collect::<Vec<_>>();
        let hits=normal.hits().iter().zip(refined.iter()).map(|(h,c)|{
            let (lower,upper)=h.interval();
            json!({"row":c.row,"raw":c.raw,"public_raw":raw_score(h.score()).unwrap(),"truth":c.truth,"floating":c.floating,"lower":lower,"upper":upper})
        }).collect::<Vec<_>>();
        reports.push(json!({"budget":budget,"requested_budget":requested,"candidate_count":pool.len(),"candidate_matches":covered,"delivered_matches":delivered,"exact_rerank_matches":exact_matches,"floating_matches":overlap(floating.iter().take(10).map(|c|c.row),exact),"renormalized_matches":overlap(renormalized.iter().take(10).map(|c|c.row),exact),"stored_exact_matches":stored_matches[0],"stored_fp16_matches":stored_matches[1],"stored_u8_matches":stored_matches[2],"minimum_alignment":pool.iter().map(|c|c.alignment).fold(f64::INFINITY,f64::min),"floating_top10_differences":floating.iter().take(10).zip(refined.iter()).filter(|(a,b)|a.row!=b.row).count(),"maximum_fixed_point_error":pool.iter().map(|c|(c.raw as f64/(1_u64<<24) as f64-c.floating).abs()).fold(0.0_f64,f64::max),"selection_losses":10-covered,"ranking_losses":covered-delivered,"neighbors":neighbors,"hits":hits}));
    }
    Ok((json!({"query":ordinal,"budgets":reports}), trace))
}

pub(crate) fn run(o: &Options) -> Result<(), BenchError> {
    allowed(
        o,
        &[
            "rows",
            "dataset",
            "query-split",
            "queries",
            "seed",
            "training-rows",
            "budgets",
            "index-dir",
            "oracle-reference",
            "output",
            "reuse",
        ],
    )?;
    let budgets = o
        .get("budgets")
        .unwrap_or("200,400,800,1600")
        .split(',')
        .map(|v| v.parse::<usize>().map_err(BenchError::harness))
        .collect::<Result<Vec<_>, _>>()?;
    require(
        !budgets.is_empty()
            && budgets.len() <= 16
            && budgets.iter().all(|b| (10..=100_000).contains(b))
            && budgets.windows(2).all(|b| b[0] < b[1]),
        "budgets must be unique ascending values from10 through100000",
    )?;
    let count = number(o, "rows", 1_000_000_u64)?;
    let query_count = number(o, "queries", 200_usize)?;
    require(
        (10..=1_000_000).contains(&count) && (1..=1000).contains(&query_count),
        "diagnostic supports10..1M rows and1..1000 queries",
    )?;
    let seed = o.require_parsed("seed")?;
    let training = number(o, "training-rows", 4096_usize)?;
    let revision = SourceRevision::capture();
    let start = Instant::now();
    let dir = Path::new(o.require("index-dir")?);
    let (rows, queries, exact, oracle, corpus_hash, build, source_value) =
        if o.get("dataset").is_some() {
            require(
                o.get("rows").is_none(),
                "dataset and generated rows cannot be combined",
            )?;
            let data = super::dataset::Dataset::load(o)?;
            let (exact, oracle) = data.oracle(Path::new(o.require("oracle-reference")?))?;
            let build = data.build(o, dir)?;
            (
                data.rows,
                data.queries,
                exact,
                oracle,
                data.corpus_hash,
                build,
                data.source,
            )
        } else {
            require(
                o.get("query-split").is_none(),
                "query-split requires a real dataset",
            )?;
            let source =
                GeneratedChunks::new(count, 1_000_000, seed).map_err(BenchError::harness)?;
            let queries = source.queries(query_count);
            let (exact, oracle, corpus_hash) =
                pinned_oracle(Path::new(o.require("oracle-reference")?), &source, &queries)?;
            let build = build_index(o, &source, dir, training)?;
            let rows = source.chunk(0).map_err(BenchError::harness)?;
            require(
                hash_rows(&rows) == corpus_hash && build["corpus_hash"] == corpus_hash,
                "diagnostic original/index corpus mismatch",
            )?;
            (
                rows,
                queries,
                exact,
                oracle,
                corpus_hash,
                build,
                json!(source.descriptor()),
            )
        };
    let index = Index::open(dir).map_err(BenchError::harness)?;
    let reference = Reference::open(dir, seed, &index)?;
    let output = Path::new(o.require("output")?);
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(BenchError::harness)?;
    let trace_path = output.with_extension("candidates.jsonl");
    require(
        !trace_path.exists(),
        "diagnostic trace output already exists",
    )?;
    let mut trace = BufWriter::new(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&trace_path)
            .map_err(BenchError::harness)?,
    );
    let mut query_results = Vec::new();
    // Batches bound both traces and full-sort reference scratch memory. This
    // throughput is deliberately not a serving latency measurement.
    for first in (0..queries.len()).step_by(6) {
        let end = (first + 6).min(queries.len());
        let batch = std::thread::scope(|scope| {
            let handles = (first..end)
                .map(|i| {
                    let (index, reference, rows, queries, exact, budgets) =
                        (&index, &reference, &rows, &queries, &exact, &budgets);
                    scope.spawn(move || {
                        analyze(index, reference, rows, &queries[i], &exact[i], budgets, i)
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|h| {
                    h.join()
                        .map_err(|_| BenchError::harness("diagnostic worker panicked"))?
                })
                .collect::<Result<Vec<_>, BenchError>>()
        })?;
        for (value, line) in batch {
            serde_json::to_writer(&mut trace, &line).map_err(BenchError::serialize)?;
            writeln!(&mut trace).map_err(BenchError::harness)?;
            query_results.push(value);
        }
        eprintln!("diagnosed{end}/{} queries", queries.len());
    }
    trace.flush().map_err(BenchError::harness)?;
    drop(trace);
    let summaries=(0..budgets.len()).map(|i|{
        let sum=|key:&str|query_results.iter().map(|q|q["budgets"][i][key].as_u64().unwrap()).sum::<u64>();
        let total=(queries.len()*10) as f64;
        json!({"budget":budgets[i].min(rows.len()),"candidate_recall_at_10":sum("candidate_matches") as f64/total,"recall_at_10":sum("delivered_matches") as f64/total,"floating_recall_at_10":sum("floating_matches") as f64/total,"renormalized_recall_at_10":sum("renormalized_matches") as f64/total,"stored_exact_recall_at_10":sum("stored_exact_matches") as f64/total,"stored_fp16_recall_at_10":sum("stored_fp16_matches") as f64/total,"stored_u8_recall_at_10":sum("stored_u8_matches") as f64/total,"minimum_alignment":query_results.iter().map(|q|q["budgets"][i]["minimum_alignment"].as_f64().unwrap()).fold(f64::INFINITY,f64::min),"selection_losses":sum("selection_losses"),"ranking_losses":sum("ranking_losses"),"floating_top10_differences":sum("floating_top10_differences")})
    }).collect::<Vec<_>>();
    let mut value = json!({"schema_version":1,"kind":"index-diagnose","timestamp":timestamp_rfc3339_utc(),"git_commit":revision.commit,"dirty_worktree":revision.dirty,"machine":MachineProfile::capture(),"command":format!("spherra-bench index-diagnose {}",o.0.iter().map(|(k,v)|format!("--{k} {v}")).collect::<Vec<_>>().join(" ")),"source":source_value,"corpus_hash":corpus_hash,"query_hash":hash_rows(&queries),"seed":seed,"vector_count":rows.len(),"query_count":queries.len(),"training_rows":training,"validation_rows":(training/4).min(4096),"model":model_metadata(dir)?,"index_current_blake3":hash_file(&dir.join("CURRENT"))?,"build_source_commit":build["git_commit"],"build_dirty_worktree":build["dirty_worktree"],"oracle_reference":oracle,"trace":{"path":trace_path,"blake3":hash_file(&trace_path)?,"bytes":fs::metadata(&trace_path).map_err(BenchError::harness)?.len()},"checks_passed":true,"summaries":summaries,"query_results":query_results,"elapsed_seconds":start.elapsed().as_secs_f64()});
    finish_revision(&mut value);
    let schema: Value = serde_json::from_str(include_str!(
        "../../../../docs/benchmarks/local-index-loss.schema.json"
    ))
    .map_err(BenchError::serialize)?;
    validate_against_schema(&schema, &value).map_err(BenchError::harness)?;
    write_output(output, &value)
}

#[cfg(test)]
mod correction_tests {
    use super::*;
    #[test]
    fn stored_corrections_divide_by_alignment_and_its_encodings() {
        // Reconstruction 0.8 * u + 0.6 * w: length 1, alignment 0.8, ratio 0.8.
        let (mut u, mut p, e) = ([0.0_f32; 768], [0.0_f32; 768], [0.0_f32; 768]);
        u[0] = 1.0;
        p[0] = 0.8;
        p[1] = 0.6;
        let length = reconstruction_length(&p, &e);
        let (scores, ratio) = stored_corrections(0.4, &u, &p, &e, length);
        assert!((length - 1.0).abs() < 1e-6 && (ratio - 0.8).abs() < 1e-6);
        assert!((scores[0] - 0.5).abs() < 1e-6);
        assert!((scores[1] - 0.5).abs() < 1e-3);
        assert!((scores[2] - 0.5).abs() < 2e-3);
        assert_eq!(nearest_half(0.8), (0.8 * 2048.0_f64).round() / 2048.0);
    }
}
