//! Quality through the public Index API. The independent reference uses public
//! checked format readers, checked scalar scores, and full sorts; it never calls
//! the serving tile kernel, worker heaps, or private index implementation.
use crate::local_index::{
    allowed, build_index, finish_revision, hash_file, hash_rows, model_metadata, number,
};
use crate::{BenchError, Options, write_output};
use serde_json::{Value, json};
use spherra::{CreateOptions, Index, IndexBuilder, SearchOptions, Vector};
use spherra_codec::{
    CODEC_ID, DirectCode, FixedPointScorer, Pq96Code, Pq96Codebook, QuantizerTable, TransformPlan,
    dot_f64, normalize_fp64,
};
use spherra_format::{
    LayoutId, PairedResidualReader, PairedSegmentReaders, PrimaryFileReader, ResidualFileReader,
    SegmentExpectations, SegmentHeader,
};
use spherra_testkit::{
    CorpusDescriptor, GeneratedChunks, Neighbor, StreamingOracle,
    machine::{MachineProfile, SourceRevision, timestamp_rfc3339_utc},
    results::validate_against_schema,
};
use std::{collections::HashSet, fs, io::Read, path::Path, time::Instant};

fn checked_json(path: &Path, schema: &str) -> Result<Value, BenchError> {
    if fs::metadata(path).map_err(BenchError::harness)?.len() > 16 * 1024 * 1024 {
        return Err(BenchError::harness("oversized reference report"));
    }
    let value: Value = serde_json::from_slice(&fs::read(path).map_err(BenchError::harness)?)
        .map_err(BenchError::serialize)?;
    let schema: Value = serde_json::from_str(schema).map_err(BenchError::serialize)?;
    validate_against_schema(&schema, &value).map_err(BenchError::harness)?;
    Ok(value)
}

fn decode_reference(
    bytes: &[u8],
    rows: u64,
    queries: usize,
) -> Result<Vec<Vec<Neighbor>>, BenchError> {
    let count = rows.min(100) as usize;
    let length = queries
        .checked_mul(4 + count * 16)
        .and_then(|n| n.checked_add(24));
    let bad = || BenchError::harness("invalid oracle reference bytes");
    if length != Some(bytes.len())
        || bytes.len() < 24
        || &bytes[..8] != b"SPHROR01"
        || u64::from_le_bytes(bytes[8..16].try_into().unwrap()) != rows
        || u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize != queries
        || u32::from_le_bytes(bytes[20..24].try_into().unwrap()) != 100
    {
        return Err(bad());
    }
    let mut cursor = 24;
    let mut rankings = Vec::with_capacity(queries);
    for _ in 0..queries {
        if u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize != count {
            return Err(bad());
        }
        cursor += 4;
        let mut ranking: Vec<Neighbor> = Vec::with_capacity(count);
        let mut seen = HashSet::new();
        for _ in 0..count {
            let row = u64::from_le_bytes(bytes[cursor..cursor + 8].try_into().unwrap());
            let score = f64::from_le_bytes(bytes[cursor + 8..cursor + 16].try_into().unwrap());
            cursor += 16;
            if row >= rows || row > u64::from(u32::MAX) || !score.is_finite() || !seen.insert(row) {
                return Err(bad());
            }
            if let Some(previous) = ranking.last() {
                if previous.score.total_cmp(&score).is_lt()
                    || (previous.score.total_cmp(&score).is_eq() && u64::from(previous.row) >= row)
                {
                    return Err(bad());
                }
            }
            ranking.push(Neighbor {
                row: row as u32,
                score,
            });
        }
        rankings.push(ranking);
    }
    Ok(rankings)
}

fn pinned_oracle(
    path: &Path,
    source: &GeneratedChunks,
    queries: &[Vector],
) -> Result<(Vec<Vec<Neighbor>>, Value, String), BenchError> {
    let report = checked_json(
        path,
        include_str!("../../../docs/benchmarks/local-oracle-reference.schema.json"),
    )?;
    if report["source"] != json!(source.descriptor())
        || report["query_hash"] != hash_rows(queries)
        || report["query_count"] != queries.len()
        || report["rows_scored"] != source.descriptor().vector_count
        || report["k"] != 100
    {
        return Err(BenchError::harness(
            "oracle reference does not match the source and query bytes",
        ));
    }
    let artifact = Path::new(report["artifact_path"].as_str().unwrap());
    let expected_bytes =
        24 + queries.len() as u64 * (4 + source.descriptor().vector_count.min(100) * 16);
    if report["artifact_bytes"] != expected_bytes
        || fs::metadata(artifact).map_err(BenchError::harness)?.len() != expected_bytes
    {
        return Err(BenchError::harness("oracle reference length mismatch"));
    }
    let bytes = fs::read(artifact).map_err(BenchError::harness)?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    if report["artifact_blake3"] != hash {
        return Err(BenchError::harness("oracle reference BLAKE3 mismatch"));
    }
    let rankings = decode_reference(&bytes, source.descriptor().vector_count, queries.len())?;
    Ok((
        rankings,
        json!({"kind":"pinned","report_path":path,"report_blake3":hash_file(path)?,"rankings_blake3":hash,"git_commit":report["git_commit"],"dirty_worktree":report["dirty_worktree"]}),
        report["corpus_hash"].as_str().unwrap().to_owned(),
    ))
}

fn historical_hit_count(recall: f64, total: u64) -> Result<u64, BenchError> {
    let hits = recall * total as f64;
    // Historical files averaged per-query fractions in FP64. Recover the
    // integer hit count, allowing only their sub-millionth-of-one-hit roundoff.
    if !(0.0..=1.0).contains(&recall)
        || !hits.is_finite()
        || (hits - hits.round()).abs() > 0.000_001
    {
        return Err(BenchError::harness(
            "historical reference recall is not an integer hit count",
        ));
    }
    Ok(hits.round() as u64)
}
fn historical_passes(baseline: u64, actual: u64, total: u64) -> bool {
    u128::from(baseline) * 100 <= u128::from(actual) * 100 + u128::from(total)
}
fn history(
    o: &Options,
    corpus: &spherra_testkit::CorpusSplits,
    seed: u64,
    budget: usize,
) -> Result<Value, BenchError> {
    let Some(path) = o.get("historical-reference") else {
        return Ok(Value::Null);
    };
    let path = Path::new(path);
    let report = checked_json(
        path,
        include_str!("../../../docs/benchmarks/codec-format-baseline.schema.json"),
    )?;
    let matching: Vec<_> = report
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["candidate_budget"] == budget)
        .collect();
    let bad = || {
        BenchError::harness(
            "historical reference does not match corpus bytes, queries, seed, budget, and scorer",
        )
    };
    if matching.len() != 1 {
        return Err(bad());
    }
    let entry = matching[0];
    if entry["corpus_hash"] != corpus.hash()
        || entry["corpus_name"] != corpus.name()
        || entry["vector_count"] != corpus.indexed().len()
        || entry["query_count"] != corpus.queries().len()
        || entry["seed"] != seed
        || entry["scorer_version"] != 1
        || entry["codec_id"] != spherra_testkit::harness::codec_id_hex()
        || entry["primary_bound_violation_count"] != 0
        || entry["refined_bound_violation_count"] != 0
    {
        return Err(bad());
    }
    let total = corpus.queries().len() as u64 * 10;
    let recall = entry["recall_at_10"].as_f64().unwrap();
    let count = historical_hit_count(recall, total)?;
    Ok(
        json!({"report_path":path,"report_blake3":hash_file(path)?,"git_commit":entry["git_commit"],"dirty_worktree":entry["dirty_worktree"],"recall_at_10":recall,"matching_hits":count,"hits_total":total,"drop":0.0,"gate_passed":true}),
    )
}

struct ReferenceSegment {
    first: usize,
    residual: PairedResidualReader,
}
struct Reference {
    plan: TransformPlan,
    table: QuantizerTable,
    book: Pq96Codebook,
    codes: Vec<DirectCode>,
    segments: Vec<ReferenceSegment>,
}
impl Reference {
    fn open(dir: &Path, seed: u64, index: &Index) -> Result<Self, BenchError> {
        let paths = fs::read_dir(dir)
            .map_err(BenchError::harness)?
            .map(|e| e.map(|e| e.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(BenchError::harness)?;
        let paths: Vec<_> = paths
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e == "primary"))
            .collect();
        if paths.len() != index.segment_count() as usize {
            return Err(BenchError::harness(
                "reference segment set differs from opened index",
            ));
        }
        let plan = TransformPlan::from_seed(seed);
        let mut expected = None;
        let mut model = None;
        let mut readers = Vec::new();
        for path in paths {
            let mut bytes = [0; spherra_format::HEADER_LEN];
            fs::File::open(&path)
                .map_err(BenchError::harness)?
                .read_exact(&mut bytes)
                .map_err(BenchError::harness)?;
            let header = SegmentHeader::decode(&bytes).map_err(BenchError::harness)?;
            let expected = expected.get_or_insert(SegmentExpectations {
                codec_id: *CODEC_ID,
                scorer_version: 1,
                transform_id: *plan.identity(),
                quantizer_id: header.identity.quantizer_id,
                pq_codebook_id: header.identity.pq_codebook_id,
                layout: LayoutId::TiledSoa32,
            });
            let primary =
                PrimaryFileReader::open_path(&path, expected).map_err(BenchError::harness)?;
            let residual =
                ResidualFileReader::open_path(&path.with_extension("residual"), expected)
                    .map_err(BenchError::harness)?;
            if model.is_none() {
                let table = QuantizerTable::from_centers(
                    &primary.quantizer_table().map_err(BenchError::harness)?,
                )
                .map_err(BenchError::harness)?;
                let book = Pq96Codebook::from_centroids(
                    &residual.pq_codebook().map_err(BenchError::harness)?,
                )
                .map_err(BenchError::harness)?;
                if table.identity() != &expected.quantizer_id
                    || book.codebook_id() != &expected.pq_codebook_id
                {
                    return Err(BenchError::harness(
                        "reference restored model identity mismatch",
                    ));
                }
                model = Some((table, book));
            }
            let first = usize::try_from(
                primary
                    .row(0)
                    .map_err(BenchError::harness)?
                    .chunk_id
                    .as_u128(),
            )
            .map_err(BenchError::harness)?;
            readers.push((
                first,
                PairedSegmentReaders::open(primary, residual).map_err(BenchError::harness)?,
            ));
        }
        readers.sort_by_key(|r| r.0);
        let mut codes = Vec::with_capacity(index.len() as usize);
        let mut segments = Vec::new();
        for (first, paired) in readers {
            if first != codes.len() {
                return Err(BenchError::harness(
                    "reference row ranges are not contiguous",
                ));
            }
            let count = paired.primary().row_count() as usize;
            for ordinal in 0..count.div_ceil(32) {
                let tile = paired
                    .primary()
                    .primary_tile(ordinal as u32)
                    .map_err(BenchError::harness)?;
                for lane in 0..(count - ordinal * 32).min(32) {
                    let local = ordinal * 32 + lane;
                    if paired
                        .primary()
                        .row(local as u32)
                        .map_err(BenchError::harness)?
                        .chunk_id
                        .as_u128()
                        != (first + local) as u128
                    {
                        return Err(BenchError::harness("reference physical row ID mismatch"));
                    }
                    codes.push(
                        DirectCode::from_nibbles(std::array::from_fn(|c| {
                            (tile[c * 16 + lane / 2] >> ((lane % 2) * 4)) & 15
                        }))
                        .map_err(BenchError::harness)?,
                    );
                }
            }
            segments.push(ReferenceSegment {
                first,
                residual: paired.into_residual(),
            });
        }
        if codes.len() as u64 != index.len() {
            return Err(BenchError::harness("reference physical row count mismatch"));
        }
        let (table, book) = model.ok_or_else(|| BenchError::harness("empty reference model"))?;
        Ok(Self {
            plan,
            table,
            book,
            codes,
            segments,
        })
    }
    fn search(&self, raw: &Vector, budget: usize) -> Result<Vec<(usize, i64)>, BenchError> {
        let scorer = FixedPointScorer::new();
        let query = scorer
            .prepare_query(&self.plan, raw, &self.table, &self.book)
            .map_err(BenchError::harness)?;
        let scale = query.lookup_scale_measurement();
        // Hit::score is public; raw is deliberately private. In this measured
        // range Q24 -> f64 -> Q24 is injective, so comparing recovered integers
        // proves the same raw-score equality without a benchmark-only Index API.
        if i128::from(scale.maximum_primary_lookup_entry()) * 768
            + i128::from(scale.maximum_residual_lookup_entry()) * 96
            > (1_i128 << 53)
        {
            return Err(BenchError::harness(
                "reference raw scores exceed exact public FP64 conversion range",
            ));
        }
        let mut ranked: Vec<_> = self
            .codes
            .iter()
            .enumerate()
            .map(|(row, code)| (row, scorer.score_primary(&query, code).raw()))
            .collect();
        ranked.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ranked.truncate(budget.min(ranked.len()));
        for (row, score) in &mut ranked {
            let segment = &self.segments[self.segments.partition_point(|s| s.first <= *row) - 1];
            let residual = Pq96Code::from_bytes(
                segment
                    .residual
                    .residual_code((*row - segment.first) as u32)
                    .map_err(BenchError::harness)?,
            );
            *score = scorer
                .score_refined(&query, &self.codes[*row], &residual)
                .raw();
        }
        ranked.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ranked.truncate(10);
        Ok(ranked)
    }
}

struct QueryOutcome {
    value: Value,
    matches: u64,
    differences: u64,
    violations: u64,
}
fn qualify_query(
    index: &Index,
    reference: &Reference,
    rows: &[Vector],
    query: &Vector,
    exact: &[Neighbor],
    ordinal: usize,
    budget: usize,
) -> Result<QueryOutcome, BenchError> {
    let expected = reference.search(query, budget)?;
    let result = index
        .search(
            query,
            SearchOptions {
                k: 10,
                candidate_budget: Some(budget),
            },
        )
        .map_err(BenchError::harness)?;
    if result.hits().len() != 10
        || result.rows_scanned() != rows.len() as u64
        || result.rows_refined() != budget.min(rows.len()) as u64
        || result.candidate_budget() != budget.min(rows.len())
        || result.generation() != index.generation()
    {
        return Err(BenchError::harness(
            "public search reported an incomplete quality workload",
        ));
    }
    let normalized = normalize_fp64(query).map_err(BenchError::harness)?;
    let mut hits = Vec::new();
    let mut matches = 0;
    let mut differences = 0;
    let mut violations = 0;
    for (hit, (expected_row, expected_raw)) in result.hits().iter().zip(expected) {
        let row = usize::try_from(hit.row().get()).map_err(BenchError::harness)?;
        let original = rows.get(row).ok_or_else(|| {
            BenchError::harness("public search returned a row outside the corpus")
        })?;
        let scaled = hit.score() * FixedPointScorer::new().metadata().comparison_scale() as f64;
        if !scaled.is_finite() || scaled.abs() > (1_u64 << 53) as f64 || scaled.fract() != 0.0 {
            return Err(BenchError::harness(
                "public score is not an exact Q24 integer",
            ));
        }
        let raw = scaled as i64;
        differences += u64::from(row != expected_row || raw != expected_raw);
        let truth = dot_f64(
            &normalized,
            &normalize_fp64(original).map_err(BenchError::harness)?,
        );
        let (lower, upper) = hit.interval();
        violations +=
            u64::from(!lower.is_finite() || !upper.is_finite() || lower > truth || truth > upper);
        matches += u64::from(exact.iter().take(10).any(|n| n.row as usize == row));
        hits.push(json!({"row":row,"raw":raw,"expected_row":expected_row,"expected_raw":expected_raw,"score":hit.score(),"lower":lower,"upper":upper,"truth":truth}));
    }
    Ok(QueryOutcome {
        value: json!({"query":ordinal,"exact_matches":matches,"hits":hits}),
        matches,
        differences,
        violations,
    })
}

pub(super) fn run(o: &Options) -> Result<(), BenchError> {
    allowed(
        o,
        &[
            "corpus",
            "rows",
            "queries",
            "seed",
            "training-rows",
            "candidate-budget",
            "index-dir",
            "output",
            "reuse",
            "oracle-reference",
            "historical-reference",
        ],
    )?;
    let seed = o.require_parsed("seed")?;
    let query_count = number(o, "queries", 200_usize)?;
    let budget = number(o, "candidate-budget", 200_usize)?;
    if !(1..=1000).contains(&query_count) || budget < 10 {
        return Err(BenchError::harness("invalid quality query count or budget"));
    }
    let start = Instant::now();
    let revision = SourceRevision::capture();
    let mut value = json!({"schema_version":1,"kind":"index","timestamp":timestamp_rfc3339_utc(),"git_commit":revision.commit,"dirty_worktree":revision.dirty,"machine":MachineProfile::capture(),"command":format!("spherra-bench index {}",o.0.iter().map(|(k,v)|format!("--{k} {v}")).collect::<Vec<_>>().join(" ")),"durability_mode":"file-and-directory-sync"});
    let dir = Path::new(o.require("index-dir")?);
    let (rows, queries, exact, oracle, mut historical, build) = if let Some(name) = o.get("corpus")
    {
        if o.get("rows").is_some()
            || o.get("training-rows").is_some()
            || o.get("oracle-reference").is_some()
            || number(o, "reuse", false)?
        {
            return Err(BenchError::harness(
                "legacy corpus quality requires a fresh index and its own calibration split",
            ));
        }
        let descriptor = CorpusDescriptor::resolve(name).map_err(BenchError::corpus)?;
        let source = match &descriptor {
            CorpusDescriptor::Generated(d) if d.vector_count <= 1_000_000 => {
                json!({"kind":"generated","descriptor":d})
            }
            CorpusDescriptor::FileBacked(d) if d.row_count <= 1_000_000 => {
                json!({"kind":"file-backed","descriptor":d})
            }
            _ => {
                return Err(BenchError::harness(
                    "quality qualification is bounded to 1M source rows",
                ));
            }
        };
        let corpus = descriptor
            .load(seed, query_count)
            .map_err(BenchError::corpus)?;
        if corpus.indexed().len() < 10 {
            return Err(BenchError::harness(
                "quality corpus needs at least ten indexed rows",
            ));
        }
        let historical = history(o, &corpus, seed, budget)?;
        value["source"] = source;
        value["corpus_name"] = json!(corpus.name());
        value["corpus_hash"] = json!(corpus.hash());
        value["training_rows"] = json!(corpus.calibration().len());
        let mut oracle =
            StreamingOracle::new(corpus.queries(), 100).map_err(BenchError::harness)?;
        oracle
            .extend(corpus.indexed())
            .map_err(BenchError::harness)?;
        let oracle_info = json!({"kind":"fresh-streaming","report_path":null,"report_blake3":null,"rankings_blake3":blake3::hash(&oracle.reference_bytes()).to_hex().to_string(),"git_commit":value["git_commit"],"dirty_worktree":value["dirty_worktree"]});
        let mut builder = IndexBuilder::create(
            dir,
            corpus.calibration(),
            CreateOptions {
                seed,
                validation_rows: None,
            },
        )
        .map_err(BenchError::harness)?;
        for row in corpus.indexed() {
            builder.push(row).map_err(BenchError::harness)?;
        }
        let report = builder.commit().map_err(BenchError::harness)?;
        if !report.cleanup_complete() {
            return Err(BenchError::harness("quality build cleanup incomplete"));
        }
        let mut build =
            json!({"git_commit":value["git_commit"],"dirty_worktree":value["dirty_worktree"]});
        finish_revision(&mut build);
        (
            corpus.indexed().to_vec(),
            corpus.queries().to_vec(),
            oracle.top_k(),
            oracle_info,
            historical,
            build,
        )
    } else {
        if o.get("historical-reference").is_some() {
            return Err(BenchError::harness(
                "chunked quality has no historical reference",
            ));
        }
        let count = number(o, "rows", 1_000_000_u64)?;
        if !(10..=1_000_000).contains(&count) {
            return Err(BenchError::harness(
                "chunked quality requires 10..=1000000 rows",
            ));
        }
        let source = GeneratedChunks::new(count, 1_000_000, seed).map_err(BenchError::harness)?;
        let queries = source.queries(query_count);
        let (exact, oracle, hash) =
            pinned_oracle(Path::new(o.require("oracle-reference")?), &source, &queries)?;
        let training = number(o, "training-rows", 4096_usize)?;
        let build = build_index(o, &source, dir, training)?;
        let rows = source.chunk(0).map_err(BenchError::harness)?;
        if build["corpus_hash"] != hash || hash_rows(&rows) != hash {
            return Err(BenchError::harness(
                "oracle corpus hash differs from index input bytes",
            ));
        }
        value["source"] = json!({"kind":"chunked","descriptor":source.descriptor()});
        value["corpus_name"] = json!(format!("generated-correlated-chunked-768x{count}"));
        value["corpus_hash"] = json!(hash);
        value["training_rows"] = json!(training);
        (rows, queries, exact, oracle, Value::Null, build)
    };
    let index = Index::open(dir).map_err(BenchError::harness)?;
    if index.len() != rows.len() as u64 {
        return Err(BenchError::harness("quality index row count mismatch"));
    }
    let reference = Reference::open(dir, seed, &index)?;
    let width = queries.len().div_ceil(6);
    let outcomes = std::thread::scope(|scope| {
        let handles: Vec<_> = queries
            .chunks(width)
            .enumerate()
            .map(|(batch, chunk)| {
                let (index, reference, rows, exact) = (&index, &reference, &rows, &exact);
                scope.spawn(move || {
                    chunk
                        .iter()
                        .enumerate()
                        .map(|(local, q)| {
                            qualify_query(
                                index,
                                reference,
                                rows,
                                q,
                                &exact[batch * width + local],
                                batch * width + local,
                                budget,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .map_err(|_| BenchError::harness("quality worker panicked"))?
            })
            .collect::<Result<Vec<_>, _>>()
    })?
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let matches: u64 = outcomes.iter().map(|q| q.matches).sum();
    let differences: u64 = outcomes.iter().map(|q| q.differences).sum();
    let violations: u64 = outcomes.iter().map(|q| q.violations).sum();
    let total = queries.len() as u64 * 10;
    if !historical.is_null() {
        let previous = historical["matching_hits"].as_u64().unwrap();
        historical["drop"] = json!((previous as f64 - matches as f64) / total as f64);
        historical["gate_passed"] = json!(historical_passes(previous, matches, total));
    }
    let passed = differences == 0
        && violations == 0
        && (historical.is_null() || historical["gate_passed"] == true);
    value["query_hash"] = json!(hash_rows(&queries));
    value["seed"] = json!(seed);
    value["vector_count"] = json!(rows.len());
    value["query_count"] = json!(queries.len());
    value["k"] = json!(10);
    value["candidate_budget"] = json!(budget);
    value["generation"] = json!(index.generation());
    value["segment_count"] = json!(index.segment_count());
    value["model"] = model_metadata(dir)?;
    value["index_current_blake3"] = json!(hash_file(&dir.join("CURRENT"))?);
    value["oracle_reference"] = oracle;
    value["historical_reference"] = historical;
    value["queries_checked"] = json!(outcomes.len());
    value["hits_checked"] = json!(total);
    value["equality_differences"] = json!(differences);
    value["enclosure_failures"] = json!(violations);
    value["recall_at_10"] = json!(matches as f64 / total as f64);
    value["query_results"] = json!(outcomes.into_iter().map(|q| q.value).collect::<Vec<_>>());
    value["elapsed_seconds"] = json!(start.elapsed().as_secs_f64());
    value["build_source_commit"] = build["git_commit"].clone();
    value["build_dirty_worktree"] = build["dirty_worktree"].clone();
    value["reused_index"] = json!(number(o, "reuse", false)?);
    finish_revision(&mut value);
    let reference_clean = if value["source"]["kind"] == "chunked" {
        rows.len() == 1_000_000 && value["oracle_reference"]["dirty_worktree"] == false
    } else {
        value["historical_reference"]["dirty_worktree"] == false
    };
    value["gate_eligible"] = json!(
        query_count == 200
            && value["dirty_worktree"] == false
            && value["build_dirty_worktree"] == false
            && value["machine"]["cargo_profile"] == "release"
            && reference_clean
    );
    value["gate_passed"] = json!(passed);
    let schema: Value = serde_json::from_str(include_str!(
        "../../../docs/benchmarks/local-index-quality.schema.json"
    ))
    .map_err(BenchError::serialize)?;
    validate_against_schema(&schema, &value).map_err(BenchError::harness)?;
    write_output(Path::new(o.require("output")?), &value)?;
    if !passed {
        return Err(BenchError::harness(
            "index quality gate failed; measured evidence was written",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn historical_floor_is_exactly_one_percent_of_integer_hits() {
        assert!(super::historical_passes(1874, 1854, 2000));
        assert!(!super::historical_passes(1874, 1853, 2000));
        assert!(super::historical_passes(1874, 1900, 2000));
        assert_eq!(
            super::historical_hit_count(0.9370000000000018, 2000).unwrap(),
            1874
        );
        assert!(super::historical_hit_count(0.93725, 2000).is_err());
    }
    #[test]
    fn reference_decoder_rejects_counts_order_duplicates_and_trailing_data() {
        let mut oracle = spherra_testkit::StreamingOracle::new(&[[1.0; 768]], 100).unwrap();
        oracle.extend(&[[1.0; 768]; 2]).unwrap();
        let bytes = oracle.reference_bytes();
        assert_eq!(super::decode_reference(&bytes, 2, 1).unwrap()[0].len(), 2);
        let mut bad = bytes.clone();
        bad.push(0);
        assert!(super::decode_reference(&bad, 2, 1).is_err());
        let mut bad = bytes.clone();
        bad[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(super::decode_reference(&bad, 2, 1).is_err());
        let mut bad = bytes.clone();
        bad[44..52].copy_from_slice(&0_u64.to_le_bytes());
        assert!(super::decode_reference(&bad, 2, 1).is_err());
        let mut bad = bytes.clone();
        bad[36..44].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(super::decode_reference(&bad, 2, 1).is_err());
        let mut bad = bytes.clone();
        bad[36..44].copy_from_slice(&0.0_f64.to_le_bytes());
        assert!(super::decode_reference(&bad, 2, 1).is_err());
        assert!(super::decode_reference(&bytes[..bytes.len() - 1], 2, 1).is_err());
    }
}
