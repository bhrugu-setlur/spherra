//! Public-API local-index measurements. Memory peaks come from a fresh child
//! under macOS `/usr/bin/time -l`, not a sample taken after allocations drop.
use super::{BenchError, Options, write_output};
use serde_json::{Value, json};
use spherra::{CreateOptions, Index, IndexBuilder, MAX_TRAINING_ROWS, SearchOptions, Vector};
use spherra_testkit::{
    CanonicalRowHasher, GeneratedChunks, StreamingOracle,
    machine::{MachineProfile, SourceRevision, timestamp_rfc3339_utc},
    results::validate_against_schema,
};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Instant,
};
const CHUNK_ROWS: usize = 1_000_000;
const GIB: u64 = 1024 * 1024 * 1024;
fn number<T: std::str::FromStr>(o: &Options, key: &str, default: T) -> Result<T, BenchError> {
    match o.get(key) {
        Some(v) => v
            .parse()
            .map_err(|_| BenchError::UnparsableOption(key.to_owned())),
        None => Ok(default),
    }
}
fn allowed(o: &Options, names: &[&str]) -> Result<(), BenchError> {
    for name in o.0.keys() {
        if !names.contains(&name.as_str()) {
            return Err(BenchError::UnexpectedArgument(format!("--{name}")));
        }
    }
    Ok(())
}
fn source(o: &Options, default_rows: u64) -> Result<GeneratedChunks, BenchError> {
    GeneratedChunks::new(
        number(o, "rows", default_rows)?,
        CHUNK_ROWS,
        o.require_parsed("seed")?,
    )
    .map_err(BenchError::harness)
}
fn base(kind: &str, o: &Options, source: &GeneratedChunks) -> Value {
    let revision = SourceRevision::capture();
    json!({"schema_version":1,"kind":kind,"timestamp":timestamp_rfc3339_utc(),"git_commit":revision.commit,"dirty_worktree":revision.dirty,"machine":MachineProfile::capture(),"command":format!("spherra-bench {kind} {}",o.0.iter().map(|(k,v)|format!("--{k} {v}")).collect::<Vec<_>>().join(" ")),"source":source.descriptor(),"durability_mode":if kind=="oracle-reference" {"not-applicable"} else {"file-and-directory-sync"}})
}
fn finish_revision(value: &mut Value) {
    let end = SourceRevision::capture();
    if end.dirty || value["git_commit"] != end.commit {
        value["dirty_worktree"] = json!(true);
    }
}
fn hash_rows(rows: &[Vector]) -> String {
    let mut h = CanonicalRowHasher::default();
    h.update(rows);
    h.hash()
}
fn hash_file(path: &Path) -> Result<String, BenchError> {
    Ok(blake3::hash(&fs::read(path).map_err(BenchError::harness)?)
        .to_hex()
        .to_string())
}
fn check_schema(kind: &str, value: &Value) -> Result<(), BenchError> {
    let text = match kind {
        "oracle-reference" => {
            include_str!("../../../docs/benchmarks/local-oracle-reference.schema.json")
        }
        "latency" => include_str!("../../../docs/benchmarks/local-latency.schema.json"),
        "build-memory" => include_str!("../../../docs/benchmarks/local-build-memory.schema.json"),
        _ => unreachable!(),
    };
    let schema: Value = serde_json::from_str(text).map_err(BenchError::serialize)?;
    validate_against_schema(&schema, value).map_err(|e| BenchError::Schema(e.to_string()))
}
pub(super) fn oracle_reference(o: &Options) -> Result<(), BenchError> {
    allowed(o, &["rows", "queries", "seed", "output"])?;
    let source = source(o, 1_000_000)?;
    let queries = source.queries(number(o, "queries", 200_usize)?);
    let mut value = base("oracle-reference", o, &source);
    let start = Instant::now();
    let mut oracle = StreamingOracle::new(&queries, 100).map_err(BenchError::harness)?;
    for chunk in 0..source.descriptor().chunk_seeds.len() {
        let rows = source.chunk(chunk).map_err(BenchError::harness)?;
        oracle.extend(&rows).map_err(BenchError::harness)?;
        eprintln!("oracle: {} rows scored", oracle.row_count());
    }
    let elapsed = start.elapsed().as_secs_f64();
    let bytes = oracle.reference_bytes();
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let output = Path::new(o.require("output")?);
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(BenchError::harness)?;
    let artifact = parent.join(format!("oracle-{hash}.bin"));
    if artifact.exists() {
        if fs::read(&artifact).map_err(BenchError::harness)? != bytes {
            return Err(BenchError::harness("existing reference artifact differs"));
        }
    } else {
        fs::write(&artifact, &bytes).map_err(BenchError::harness)?;
        let mut permissions = fs::metadata(&artifact)
            .map_err(BenchError::harness)?
            .permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&artifact, permissions).map_err(BenchError::harness)?;
    }
    value["corpus_hash"] = json!(oracle.corpus_hash());
    value["query_hash"] = json!(hash_rows(&queries));
    value["query_count"] = json!(queries.len());
    value["k"] = json!(100);
    value["artifact_path"] = json!(artifact);
    value["artifact_bytes"] = json!(bytes.len());
    value["artifact_blake3"] = json!(hash);
    value["elapsed_seconds"] = json!(elapsed);
    value["rows_scored"] = json!(oracle.row_count());
    value["workers"] = json!(6);
    finish_revision(&mut value);
    check_schema("oracle-reference", &value)?;
    write_output(output, &value)
}
fn resident_bytes() -> Result<u64, BenchError> {
    let out = Command::new("/bin/ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .map_err(BenchError::harness)?;
    if !out.status.success() {
        return Err(BenchError::harness("ps RSS query failed"));
    }
    let kib = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u64>()
        .map_err(BenchError::harness)?;
    kib.checked_mul(1024)
        .ok_or_else(|| BenchError::harness("RSS overflow"))
}
fn ac_power() -> Result<bool, BenchError> {
    let out = Command::new("/usr/bin/pmset")
        .args(["-g", "batt"])
        .output()
        .map_err(BenchError::harness)?;
    if !out.status.success() {
        return Err(BenchError::harness("pmset power check failed"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).contains("Now drawing from 'AC Power'"))
}
fn descriptors() -> Result<usize, BenchError> {
    Ok(fs::read_dir("/dev/fd")
        .map_err(BenchError::harness)?
        .count())
}
fn qualified_machine(value: &Value) -> bool {
    value["machine"]["cpu"]
        .as_str()
        .is_some_and(|s| s.contains("M1 Pro"))
        && value["machine"]["architecture"] == "aarch64"
        && value["machine"]["physical_memory_bytes"] == 32 * GIB
        && value["machine"]["cargo_profile"] == "release"
        && value["dirty_worktree"] == false
}
fn model_metadata(dir: &Path) -> Result<Value, BenchError> {
    let path = fs::read_dir(dir)
        .map_err(BenchError::harness)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "primary"))
        .ok_or_else(|| BenchError::harness("no primary segment"))?;
    let mut header = [0; spherra_format::HEADER_LEN];
    fs::File::open(path)
        .map_err(BenchError::harness)?
        .read_exact(&mut header)
        .map_err(BenchError::harness)?;
    let id = spherra_format::SegmentHeader::decode(&header)
        .map_err(BenchError::harness)?
        .identity;
    let hex = spherra_testkit::harness::hex;
    Ok(
        json!({"transform_id":hex(&id.transform_id),"quantizer_id":hex(&id.quantizer_id),"pq_codebook_id":hex(&id.pq_codebook_id),"codec_id":hex(&id.codec_id),"scorer_version":id.scorer_version,"layout":"tiled-soa-32"}),
    )
}
fn build_index(
    o: &Options,
    source: &GeneratedChunks,
    dir: &Path,
    training_rows: usize,
) -> Result<Value, BenchError> {
    let sidecar = dir.join("benchmark-build.json");
    if number(o, "reuse", false)? {
        let metadata = fs::metadata(&sidecar).map_err(BenchError::harness)?;
        if metadata.len() > 1024 * 1024 {
            return Err(BenchError::harness("oversized build provenance"));
        }
        let value: Value =
            serde_json::from_slice(&fs::read(&sidecar).map_err(BenchError::harness)?)
                .map_err(BenchError::serialize)?;
        if value["source"] != json!(source.descriptor())
            || value["training_rows"] != training_rows
            || value["index_current_blake3"] != hash_file(&dir.join("CURRENT"))?
        {
            return Err(BenchError::harness(
                "reused index does not match recorded build provenance",
            ));
        }
        return Ok(value);
    }
    let revision = SourceRevision::capture();
    let start = Instant::now();
    eprintln!("build: training on {training_rows} inputs");
    let training = source.training(training_rows);
    let mut builder = Some(
        IndexBuilder::create(
            dir,
            &training,
            CreateOptions {
                seed: source.descriptor().seed,
                validation_rows: None,
            },
        )
        .map_err(BenchError::harness)?,
    );
    drop(training);
    let mut hasher = CanonicalRowHasher::default();
    let mut total = 0_u64;
    let mut drift_warnings = 0;
    for chunk in 0..source.descriptor().chunk_seeds.len() {
        let rows = source.chunk(chunk).map_err(BenchError::harness)?;
        hasher.update(&rows);
        let mut active = match builder.take() {
            Some(b) => b,
            None => IndexBuilder::append(dir).map_err(BenchError::harness)?,
        };
        for row in &rows {
            let id = active.push(row).map_err(BenchError::harness)?;
            if id.get() != total {
                return Err(BenchError::harness("builder row ordinal mismatch"));
            }
            total += 1;
            if total % 65536 == 0 {
                eprintln!("build: {total} rows staged");
            }
        }
        let report = active.commit().map_err(BenchError::harness)?;
        if report.rows_added() != rows.len() as u64 || !report.cleanup_complete() {
            return Err(BenchError::harness(
                "incomplete benchmark commit or cleanup",
            ));
        }
        drift_warnings += u64::from(report.drift().warned());
        eprintln!(
            "build: generation {} committed, {total} rows",
            report.generation()
        );
    }
    let seconds = start.elapsed().as_secs_f64();
    let value = json!({"source":source.descriptor(),"training_rows":training_rows,"corpus_hash":hasher.hash(),"index_current_blake3":hash_file(&dir.join("CURRENT"))?,"git_commit":revision.commit,"dirty_worktree":revision.dirty,"build_seconds":seconds,"build_rows_per_second":total as f64/seconds,"drift_warnings":drift_warnings});
    let mut value = value;
    finish_revision(&mut value);
    write_output(&sidecar, &value)?;
    Ok(value)
}
pub(super) fn latency_child(o: &Options) -> Result<(), BenchError> {
    allowed(
        o,
        &[
            "rows",
            "queries",
            "warmup",
            "training-rows",
            "seed",
            "index-dir",
            "output",
            "reuse",
        ],
    )?;
    let source = source(o, 1_000_000)?;
    let query_count = number(o, "queries", 1000_usize)?;
    let warmup = number(o, "warmup", 50_usize)?;
    let training_rows = number(o, "training-rows", 4096_usize)?;
    if query_count == 0 || training_rows > MAX_TRAINING_ROWS {
        return Err(BenchError::harness("invalid measurement sizes"));
    }
    let mut value = base("latency", o, &source);
    let dir = Path::new(o.require("index-dir")?);
    let build = build_index(o, &source, dir, training_rows)?;
    let queries = source.queries(
        query_count
            .checked_add(warmup)
            .ok_or_else(|| BenchError::harness("query count overflow"))?,
    );
    let before = descriptors()?;
    let start = Instant::now();
    let index = Index::open(dir).map_err(BenchError::harness)?;
    let open_seconds = start.elapsed().as_secs_f64();
    let held = descriptors()?
        .checked_sub(before)
        .ok_or_else(|| BenchError::harness("descriptor count decreased"))?;
    if index.len() != source.descriptor().vector_count || held != index.segment_count() as usize + 1
    {
        return Err(BenchError::harness(
            "opened index size or descriptor accounting mismatch",
        ));
    }
    let mut on_ac = ac_power()?;
    eprintln!("query: warming with {warmup} queries");
    for query in &queries[query_count..] {
        index
            .search(
                query,
                SearchOptions {
                    k: 10,
                    candidate_budget: None,
                },
            )
            .map_err(BenchError::harness)?;
    }
    let mut times = Vec::with_capacity(query_count);
    for (i, query) in queries[..query_count].iter().enumerate() {
        let start = Instant::now();
        let result = index
            .search(
                query,
                SearchOptions {
                    k: 10,
                    candidate_budget: None,
                },
            )
            .map_err(BenchError::harness)?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        if result.rows_scanned() != index.len()
            || result.rows_refined() != index.len().min(200)
            || result.hits().len() != 10_usize.min(index.len() as usize)
        {
            return Err(BenchError::harness(
                "search measurement did not execute the declared workload",
            ));
        }
        std::hint::black_box(result);
        times.push(elapsed);
        if (i + 1) % 100 == 0 {
            on_ac &= ac_power()?;
            eprintln!(
                "query: {}/{} complete, last {:.3} ms",
                i + 1,
                query_count,
                elapsed
            );
        }
    }
    on_ac &= ac_power()?;
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    let p50 = sorted[(query_count * 50).div_ceil(100) - 1];
    let p99 = sorted[(query_count * 99).div_ceil(100) - 1];
    value["corpus_hash"] = build["corpus_hash"].clone();
    value["query_hash"] = json!(hash_rows(&queries[..query_count]));
    value["training_rows"] = json!(training_rows);
    value["query_count"] = json!(query_count);
    value["warmup_queries"] = json!(warmup);
    value["k"] = json!(10);
    value["candidate_budget"] = json!(200);
    value["workers"] = json!(6);
    value["kernel"] = json!("checked-scalar");
    value["cache_state"] = json!("warm-after-explicit-queries");
    value["ac_power"] = json!(on_ac);
    value["build_seconds"] = build["build_seconds"].clone();
    value["build_rows_per_second"] = build["build_rows_per_second"].clone();
    value["build_source_commit"] = build["git_commit"].clone();
    value["build_dirty_worktree"] = build["dirty_worktree"].clone();
    value["reused_index"] = json!(number(o, "reuse", false)?);
    value["open_seconds"] = json!(open_seconds);
    value["descriptors_held"] = json!(held);
    value["generation"] = json!(index.generation());
    value["segment_count"] = json!(index.segment_count());
    value["latencies_ms"] = json!(times);
    value["p50_ms"] = json!(p50);
    value["p99_ms"] = json!(p99);
    value["model"] = model_metadata(dir)?;
    value["index_current_blake3"] = build["index_current_blake3"].clone();
    value["drift_warnings"] = build["drift_warnings"].clone();
    finish_revision(&mut value);
    value["gate_eligible"] = json!(latency_eligible(&value));
    println!(
        "{}",
        serde_json::to_string(&value).map_err(BenchError::serialize)?
    );
    Ok(())
}
struct ProbeDirectory(PathBuf);
impl Drop for ProbeDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub(super) fn memory_child(o: &Options) -> Result<(), BenchError> {
    allowed(o, &["rows", "training-rows", "seed", "output"])?;
    let source = source(o, 65537)?;
    let training_rows = number(o, "training-rows", MAX_TRAINING_ROWS)?;
    if source.descriptor().vector_count > CHUNK_ROWS as u64 || training_rows > MAX_TRAINING_ROWS {
        return Err(BenchError::harness("invalid build-memory size"));
    }
    let mut value = base("build-memory", o, &source);
    let pre = resident_bytes()?;
    let training = source.training(training_rows);
    let rows = source.chunk(0).map_err(BenchError::harness)?;
    let input_bytes = (training.capacity() + rows.capacity()) * std::mem::size_of::<Vector>();
    let post = resident_bytes()?;
    let path = Path::new(o.require("output")?)
        .with_extension(format!("{}-memory-index", std::process::id()));
    fs::create_dir(&path).map_err(BenchError::harness)?;
    let dir = ProbeDirectory(path);
    eprintln!(
        "memory: creating with {training_rows} training inputs, then staging {} rows",
        rows.len()
    );
    let mut builder = IndexBuilder::create(
        &dir.0,
        &training,
        CreateOptions {
            seed: source.descriptor().seed,
            validation_rows: None,
        },
    )
    .map_err(BenchError::harness)?;
    for row in &rows {
        builder.push(row).map_err(BenchError::harness)?;
    }
    let report = builder.commit().map_err(BenchError::harness)?;
    if report.rows_added() != rows.len() as u64 {
        return Err(BenchError::harness("memory probe row count mismatch"));
    }
    // Keep both caller-owned buffers live through staging and commit so the
    // subtraction uses input memory present throughout the measured workload.
    std::hint::black_box((&training, &rows));
    value["pre_input_rss_bytes"] = json!(pre);
    value["post_input_rss_bytes"] = json!(post);
    value["input_bytes"] = json!(input_bytes);
    value["input_logical_bytes"] =
        json!((training.len() + rows.len()) * std::mem::size_of::<Vector>());
    value["training_rows"] = json!(training_rows);
    value["pushed_rows"] = json!(rows.len());
    value["limit_bytes"] = json!(2 * GIB);
    finish_revision(&mut value);
    value["gate_eligible"] = json!(
        qualified_machine(&value) && training_rows == MAX_TRAINING_ROWS && rows.len() == 65537
    );
    println!(
        "{}",
        serde_json::to_string(&value).map_err(BenchError::serialize)?
    );
    Ok(())
}
fn latency_eligible(value: &Value) -> bool {
    qualified_machine(value)
        && value["build_dirty_worktree"] == false
        && value["ac_power"] == true
        && value["query_count"] == 1000
        && value["warmup_queries"] == 50
        && matches!(
            value["source"]["vector_count"].as_u64(),
            Some(1_000_000 | 10_000_000)
        )
}
fn latency_limits_pass(value: &Value, peak: u64) -> bool {
    let (p50, p99, memory) = match value["source"]["vector_count"].as_u64() {
        Some(1_000_000) => (150.0, 300.0, true),
        Some(10_000_000) => (1500.0, 3000.0, peak <= 20 * GIB),
        _ => return false,
    };
    memory
        && value["p50_ms"].as_f64().is_some_and(|n| n <= p50)
        && value["p99_ms"].as_f64().is_some_and(|n| n <= p99)
}
fn peak_from_time(text: &str) -> Result<u64, BenchError> {
    text.lines()
        .find(|line| line.contains("maximum resident set size"))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| BenchError::harness("time -l did not report peak RSS in bytes"))
}
pub(super) fn measured_child(kind: &str, o: &Options) -> Result<(), BenchError> {
    if !cfg!(target_os = "macos") {
        return Err(BenchError::harness(
            "these latency/memory gates require macOS time -l on the qualified host",
        ));
    }
    let output = Path::new(o.require("output")?);
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(BenchError::harness)?;
    }
    let time_log = output.with_extension("time.txt");
    let stderr = fs::File::create(&time_log).map_err(BenchError::harness)?;
    let mut command = Command::new("/usr/bin/time");
    command
        .arg("-l")
        .arg(std::env::current_exe().map_err(BenchError::harness)?)
        .arg(if kind == "latency" {
            "latency-child"
        } else {
            "build-memory-child"
        });
    for (key, value) in &o.0 {
        command.arg(format!("--{key}")).arg(value);
    }
    let result = command
        .stdout(Stdio::piped())
        .stderr(stderr)
        .output()
        .map_err(BenchError::harness)?;
    if !result.status.success() {
        return Err(BenchError::harness(format!(
            "{kind} child failed; see {}",
            time_log.display()
        )));
    }
    let time = fs::read_to_string(&time_log).map_err(BenchError::harness)?;
    let peak = peak_from_time(&time)?;
    let mut value: Value = serde_json::from_slice(&result.stdout).map_err(BenchError::serialize)?;
    value["peak_rss_bytes"] = json!(peak);
    value["time_log"] = json!(time_log);
    let eligible = value["gate_eligible"] == true;
    let passed = if kind == "build-memory" {
        let pre = value["pre_input_rss_bytes"]
            .as_u64()
            .ok_or_else(|| BenchError::harness("missing pre-input RSS"))?;
        let inputs = value["input_bytes"]
            .as_u64()
            .ok_or_else(|| BenchError::harness("missing input bytes"))?;
        let owned = peak
            .checked_sub(pre)
            .and_then(|n| n.checked_sub(inputs))
            .ok_or_else(|| BenchError::harness("negative builder memory estimate"))?;
        value["builder_owned_bytes"] = json!(owned);
        eligible && owned <= 2 * GIB
    } else {
        eligible && latency_limits_pass(&value, peak)
    };
    value["gate_passed"] = json!(passed);
    check_schema(kind, &value)?;
    write_output(output, &value)?;
    if eligible && !passed {
        return Err(BenchError::harness(format!(
            "{kind} gate missed; measured result saved to {}",
            output.display()
        )));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn latency_gates_require_the_full_clean_workload_and_inclusive_limits() {
        let mut v = serde_json::json!({"machine":{"cpu":"Apple M1 Pro","architecture":"aarch64","physical_memory_bytes":32*super::GIB,"cargo_profile":"release"},"dirty_worktree":false,"build_dirty_worktree":false,"ac_power":true,"query_count":1000,"warmup_queries":50,"source":{"vector_count":1_000_000},"p50_ms":150.0,"p99_ms":300.0});
        assert!(super::latency_eligible(&v));
        assert!(super::latency_limits_pass(&v, super::GIB));
        for (field, bad) in [
            ("dirty_worktree", serde_json::json!(true)),
            ("build_dirty_worktree", serde_json::json!(true)),
            ("ac_power", serde_json::json!(false)),
            ("query_count", serde_json::json!(999)),
            ("warmup_queries", serde_json::json!(49)),
        ] {
            let mut changed = v.clone();
            changed[field] = bad;
            assert!(!super::latency_eligible(&changed));
        }
        v["p50_ms"] = serde_json::json!(150.0001);
        assert!(!super::latency_limits_pass(&v, super::GIB));
        v["source"]["vector_count"] = serde_json::json!(10_000_000);
        v["p50_ms"] = serde_json::json!(1500.0);
        v["p99_ms"] = serde_json::json!(3000.0);
        assert!(super::latency_limits_pass(&v, 20 * super::GIB));
        assert!(!super::latency_limits_pass(&v, 20 * super::GIB + 1));
        v["p99_ms"] = serde_json::json!(3000.0001);
        assert!(!super::latency_limits_pass(&v, 20 * super::GIB));
    }

    #[test]
    fn time_peak_requires_the_labeled_byte_field() {
        assert_eq!(
            super::peak_from_time(
                " 12.5 real\n 123456 maximum resident set size\n 42 page reclaims"
            )
            .unwrap(),
            123456
        );
        assert!(super::peak_from_time("123456 other field").is_err());
    }
}
