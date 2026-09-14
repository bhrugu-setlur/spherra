//! Public candidate-pool search followed by positional FP32 original reranking.
//! This is a benchmark-only upper-cost prototype, not a serving contract change.
use crate::local_index::{
    ac_power, allowed, build_index, finish_revision, hash_file, hash_rows, model_metadata, number,
};
use crate::{BenchError, Options, write_output};
use serde_json::{Value, json};
use spherra::{Index, SearchOptions};
use spherra_codec::{dot_f64, normalize_fp64};
use spherra_testkit::{
    GeneratedChunks,
    machine::{MachineProfile, SourceRevision, timestamp_rfc3339_utc},
};
use std::{
    fs,
    io::{BufWriter, Write},
    os::unix::fs::FileExt,
    path::Path,
    time::Instant,
};
fn bad(s: &str) -> BenchError {
    BenchError::harness(s)
}
fn rerank(
    index: &Index,
    file: &fs::File,
    query: &[f32; 768],
    budget: usize,
) -> Result<Vec<(u64, f64)>, BenchError> {
    let pool = index
        .search(
            query,
            SearchOptions {
                k: budget,
                candidate_budget: Some(budget),
            },
        )
        .map_err(BenchError::harness)?;
    let mut ids = pool
        .hits()
        .iter()
        .map(|h| h.row().get())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    let q = normalize_fp64(query).map_err(BenchError::harness)?;
    let mut scored = Vec::with_capacity(ids.len());
    for row in ids {
        let mut bytes = [0_u8; 768 * 4];
        file.read_exact_at(&mut bytes, row * 768 * 4)
            .map_err(BenchError::harness)?;
        let original = std::array::from_fn(|c| {
            f32::from_le_bytes(bytes[c * 4..c * 4 + 4].try_into().unwrap())
        });
        let truth = dot_f64(&q, &normalize_fp64(&original).map_err(BenchError::harness)?);
        scored.push((row, truth));
    }
    scored.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    scored.truncate(10);
    Ok(scored)
}
pub(crate) fn run(o: &Options) -> Result<(), BenchError> {
    allowed(
        o,
        &[
            "rows",
            "queries",
            "warmup",
            "seed",
            "training-rows",
            "candidate-budget",
            "index-dir",
            "originals",
            "output",
        ],
    )?;
    let n = number(o, "rows", 1_000_000_u64)?;
    let count = number(o, "queries", 1000_usize)?;
    let warmup = number(o, "warmup", 50_usize)?;
    let budget = number(o, "candidate-budget", 200_usize)?;
    if !(10..=1_000_000).contains(&n)
        || !(1..=1000).contains(&count)
        || warmup > 100
        || !(10..=1600).contains(&budget)
    {
        return Err(bad("invalid original-rerank workload"));
    }
    let seed = o.require_parsed("seed")?;
    let source = GeneratedChunks::new(n, 1_000_000, seed).map_err(BenchError::harness)?;
    let revision = SourceRevision::capture();
    let originals = Path::new(o.require("originals")?);
    let dir = Path::new(o.require("index-dir")?);
    let mut options = o.0.clone();
    options.insert("reuse".to_owned(), "true".to_owned());
    let build = build_index(
        &Options(options),
        &source,
        dir,
        number(o, "training-rows", 4096_usize)?,
    )?;
    let index = Index::open(dir).map_err(BenchError::harness)?;
    if index.len() != n {
        return Err(bad("original/index row count mismatch"));
    }
    // Generated original bytes are independently hash-matched against the same
    // pinned build sidecar as the index. File creation is outside timed regions.
    let rows = source.chunk(0).map_err(BenchError::harness)?;
    let corpus_hash = hash_rows(&rows);
    if !originals.exists() {
        if let Some(parent) = originals.parent() {
            fs::create_dir_all(parent).map_err(BenchError::harness)?;
        }
        let mut writer = BufWriter::new(
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(originals)
                .map_err(BenchError::harness)?,
        );
        for row in &rows {
            for value in row {
                writer
                    .write_all(&value.to_le_bytes())
                    .map_err(BenchError::harness)?;
            }
        }
        writer.flush().map_err(BenchError::harness)?;
        writer.get_ref().sync_all().map_err(BenchError::harness)?;
    }
    if fs::metadata(originals).map_err(BenchError::harness)?.len() != n * 768 * 4
        || hash_file(originals)? != corpus_hash
    {
        return Err(bad("original vector file hash or length mismatch"));
    }
    drop(rows);
    if build["corpus_hash"] != corpus_hash {
        return Err(bad("original/index corpus mismatch"));
    }
    let file = fs::File::open(originals).map_err(BenchError::harness)?;
    let queries = source.queries(count + warmup);
    let mut on_ac = ac_power()?;
    for q in &queries[count..] {
        index
            .search(
                q,
                SearchOptions {
                    k: 10,
                    candidate_budget: Some(budget),
                },
            )
            .map_err(BenchError::harness)?;
        rerank(&index, &file, q, budget)?;
    }
    let mut normal = Vec::new();
    let mut exact = Vec::new();
    let mut result_hasher = blake3::Hasher::new();
    for (i, q) in queries[..count].iter().enumerate() {
        // Alternate order to avoid always giving the second path warmer pages.
        let mut run_normal = || -> Result<(), BenchError> {
            let start = Instant::now();
            let hits = index
                .search(
                    q,
                    SearchOptions {
                        k: 10,
                        candidate_budget: Some(budget),
                    },
                )
                .map_err(BenchError::harness)?;
            std::hint::black_box(hits);
            normal.push(start.elapsed().as_secs_f64() * 1000.0);
            Ok(())
        };
        let mut run_exact = || -> Result<(), BenchError> {
            let start = Instant::now();
            let hits = rerank(&index, &file, q, budget)?;
            exact.push(start.elapsed().as_secs_f64() * 1000.0);
            for (id, score) in hits {
                result_hasher.update(&id.to_le_bytes());
                result_hasher.update(&score.to_le_bytes());
            }
            Ok(())
        };
        if i % 2 == 0 {
            run_normal()?;
            run_exact()?;
        } else {
            run_exact()?;
            run_normal()?;
        }
        if (i + 1) % 100 == 0 {
            on_ac &= ac_power()?;
            eprintln!("original-rerank {}/{} queries", i + 1, count);
        }
    }
    on_ac &= ac_power()?;
    let percentile = |samples: &[f64], p: usize| {
        let mut s = samples.to_vec();
        s.sort_unstable_by(f64::total_cmp);
        s[(p * s.len()).div_ceil(100) - 1]
    };
    let mut value = json!({"schema_version":1,"kind":"original-rerank-prototype","command":format!("spherra-bench original-rerank {}",o.0.iter().map(|(k,v)|format!("--{k} {v}")).collect::<Vec<_>>().join(" ")),"ac_power":on_ac,"git_commit":revision.commit,"dirty_worktree":revision.dirty,"timestamp":timestamp_rfc3339_utc(),"machine":MachineProfile::capture(),"source":source.descriptor(),"corpus_hash":corpus_hash,"query_hash":hash_rows(&queries[..count]),"model":model_metadata(dir)?,"index_current_blake3":hash_file(&dir.join("CURRENT"))?,"build_source_commit":build["git_commit"],"build_dirty_worktree":build["dirty_worktree"],"originals_path":originals,"originals_bytes":n*768*4,"candidate_budget":budget.min(n as usize),"query_count":count,"warmup_queries":warmup,"cache_state":"warm-after-explicit-queries-and-full-file-hash","scope":"public k=B search including PQ reranking followed by sorted positional original reads and FP64 reranking; benchmark only; no latency gate claim","normal_p50_ms":percentile(&normal,50),"normal_p99_ms":percentile(&normal,99),"original_p50_ms":percentile(&exact,50),"original_p99_ms":percentile(&exact,99),"normal_samples_ms":normal,"original_samples_ms":exact,"original_results_blake3":result_hasher.finalize().to_hex().to_string()});
    finish_revision(&mut value);
    let schema: Value = serde_json::from_str(include_str!(
        "../../../docs/benchmarks/original-rerank.schema.json"
    ))
    .map_err(BenchError::serialize)?;
    spherra_testkit::results::validate_against_schema(&schema, &value)
        .map_err(BenchError::harness)?;
    write_output(Path::new(o.require("output")?), &value)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn originals_preserve_exact_truth_and_short_reads_fail() {
        let dir = tempfile::tempdir().unwrap();
        let source = GeneratedChunks::new(400, 1_000_000, 20260804).unwrap();
        let rows = source.chunk(0).unwrap();
        let training = source.training(344);
        let mut b = spherra::IndexBuilder::create(
            &dir.path().join("index"),
            &training,
            spherra::CreateOptions {
                seed: 20260804,
                validation_rows: None,
            },
        )
        .unwrap();
        for row in &rows {
            b.push(row).unwrap();
        }
        b.commit().unwrap();
        let index = Index::open(&dir.path().join("index")).unwrap();
        let path = dir.path().join("originals");
        let mut bytes = Vec::new();
        for r in &rows {
            for x in r {
                bytes.extend_from_slice(&x.to_le_bytes());
            }
        }
        fs::write(&path, &bytes).unwrap();
        let q = source.queries(1)[0];
        let file = fs::File::open(&path).unwrap();
        let actual = rerank(&index, &file, &q, 400).unwrap();
        let norm = normalize_fp64(&q).unwrap();
        let mut expected = rows
            .iter()
            .enumerate()
            .map(|(i, r)| (i as u64, dot_f64(&norm, &normalize_fp64(r).unwrap())))
            .collect::<Vec<_>>();
        expected.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        expected.truncate(10);
        assert_eq!(actual, expected);
        fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
        assert!(rerank(&index, &file, &q, 400).is_err());
    }
}
