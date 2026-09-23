//! JSON-lines benchmark driver. Keeps primary tiles resident between requests,
//! allowing an external controller to evict only residual file-cache pages.
use super::{BenchError, Options, local_index::*};
use serde_json::{Value, json};
use spherra::{Index, MAX_TRAINING_ROWS};
use spherra_testkit::{
    GeneratedChunks,
    machine::{MachineProfile, SourceRevision, timestamp_rfc3339_utc},
    results::validate_against_schema,
};
use std::{
    io::{self, BufRead, Read, Write},
    path::Path,
    sync::Barrier,
    time::Instant,
};

fn emit(value: Value) -> Result<(), BenchError> {
    let schema = serde_json::from_str(include_str!(
        "../../../docs/benchmarks/search-probe.schema.json"
    ))
    .map_err(BenchError::serialize)?;
    validate_against_schema(&schema, &value).map_err(|e| BenchError::Schema(e.to_string()))?;
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, &value).map_err(BenchError::serialize)?;
    writeln!(out)
        .and_then(|()| out.flush())
        .map_err(BenchError::harness)
}

fn fingerprint(result: &MeasuredResult) -> String {
    let mut hash = blake3::Hasher::new();
    // Both public result types expose the same fields. Hash exact bits and order;
    // formatting floats would hide signed zero and low-bit differences.
    macro_rules! record {
        ($result:expr) => {{
            let r = $result;
            hash.update(&r.generation().to_le_bytes());
            hash.update(&(r.candidate_budget() as u64).to_le_bytes());
            for hit in r.hits() {
                hash.update(&hit.row().get().to_le_bytes());
                hash.update(&hit.segment().to_le_bytes());
                hash.update(&hit.score().to_bits().to_le_bytes());
                hash.update(&hit.interval().0.to_bits().to_le_bytes());
                hash.update(&hit.interval().1.to_bits().to_le_bytes());
                hash.update(&hit.stored_magnitude().to_bits().to_le_bytes());
            }
        }};
    }
    match result {
        MeasuredResult::Cosine(r) => record!(r),
        MeasuredResult::Dot(r) => record!(r),
    }
    hash.finalize().to_hex().to_string()
}

pub(super) fn run(o: &Options) -> Result<(), BenchError> {
    allowed(
        o,
        &[
            "rows",
            "queries",
            "seed",
            "training-rows",
            "index-dir",
            "reuse",
        ],
    )?;
    let rows = number(o, "rows", 1_000_000_u64)?;
    let count = number(o, "queries", 256_usize)?;
    let training = number(o, "training-rows", 4096_usize)?;
    if count == 0 || count > 100_000 || training > MAX_TRAINING_ROWS {
        return Err(BenchError::harness("invalid probe sizes"));
    }
    let source = GeneratedChunks::new(rows, 1_000_000, o.require_parsed("seed")?)
        .map_err(BenchError::harness)?;
    let revision = SourceRevision::capture();
    let dir = Path::new(o.require("index-dir")?);
    let build = build_index(o, &source, dir, training)?;
    let queries = source.queries(count);
    let index = Index::open(dir).map_err(BenchError::harness)?;
    if index.len() != rows {
        return Err(BenchError::harness("probe row count mismatch"));
    }
    emit(
        json!({"kind":"ready", "timestamp":timestamp_rfc3339_utc(), "revision":revision,
        "machine":MachineProfile::capture(), "command":std::env::args().collect::<Vec<_>>(),
        "source":source.descriptor(), "build":build, "model":model_metadata(dir)?,
        "query_hash":hash_rows(&queries), "query_count":count, "rows":rows,
        "generation":index.generation(), "segment_count":index.segment_count(), "pid":std::process::id()}),
    )?;
    let mut input = io::stdin().lock();
    loop {
        let mut line = String::new();
        let read = (&mut input)
            .take(4097)
            .read_line(&mut line)
            .map_err(BenchError::harness)?;
        if read == 0 {
            break;
        }
        if read > 4096 {
            return Err(BenchError::harness("oversized probe request"));
        }
        let request: Value = serde_json::from_str(&line).map_err(BenchError::serialize)?;
        let object = request
            .as_object()
            .ok_or_else(|| BenchError::harness("expected probe object"))?;
        if object.len() != 2 {
            return Err(BenchError::harness("unexpected probe fields"));
        }
        let dot = match request["metric"].as_str() {
            Some("cosine") => false,
            Some("dot") => true,
            _ => return Err(BenchError::harness("expected cosine or dot metric")),
        };
        let ids = request["queries"]
            .as_array()
            .ok_or_else(|| BenchError::harness("expected query array"))?;
        if ids.is_empty() || ids.len() > 8 {
            return Err(BenchError::harness("expected 1 to 8 callers"));
        }
        let ids = ids
            .iter()
            .map(|v| {
                v.as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|&n| n < count)
                    .ok_or_else(|| BenchError::harness("query index out of range"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let barrier = Barrier::new(ids.len() + 1);
        let (results, batch_ns) = std::thread::scope(|scope| {
            let threads = ids
                .iter()
                .map(|&id| {
                    let (index, query, barrier) = (&index, &queries[id], &barrier);
                    scope.spawn(move || {
                        barrier.wait();
                        let start = Instant::now();
                        let result = measured_search(index, query, dot);
                        (id, start.elapsed().as_nanos() as u64, result)
                    })
                })
                .collect::<Vec<_>>();
            let start = Instant::now();
            barrier.wait();
            let results = threads
                .into_iter()
                .map(|thread| {
                    thread
                        .join()
                        .map_err(|_| BenchError::harness("probe caller panicked"))
                })
                .collect::<Result<Vec<_>, _>>();
            (results, start.elapsed().as_nanos() as u64)
        });
        let results = results?
            .into_iter()
            .map(|(id, elapsed, result)| {
                let result = result?;
                let (scanned, refined, hits) = result.counts();
                if scanned != rows || refined != rows.min(200) || hits != rows.min(10) as usize {
                    return Err(BenchError::harness("incomplete probe search"));
                }
                Ok(
                    json!({"query":id, "elapsed_ns":elapsed, "rows_scanned":scanned,
                "rows_refined":refined, "hits":hits, "fingerprint":fingerprint(&result)}),
                )
            })
            .collect::<Result<Vec<_>, BenchError>>()?;
        emit(
            json!({"kind":"batch", "metric":request["metric"], "batch_ns":batch_ns, "results":results}),
        )?;
    }
    let end = SourceRevision::capture();
    emit(json!({"kind":"end", "revision":end}))?;
    if end != revision {
        return Err(BenchError::harness("source changed during probe"));
    }
    Ok(())
}
