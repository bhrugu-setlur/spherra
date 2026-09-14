//! Pinned real documents, independent query splits, and reusable exact references.
use super::*;
use spherra_testkit::CanonicalRowHasher;
use std::io::BufReader;

pub(super) struct Dataset {
    pub rows: Vec<Vector>,
    pub training: Vec<Vector>,
    pub queries: Vec<Vector>,
    pub source: Value,
    pub hash: String,
    pub corpus_hash: String,
    pub query_hash: String,
    pub calibration_hash: String,
}
fn bad(message: &str) -> BenchError {
    BenchError::harness(message)
}
fn read_vectors(v: &Value, limit: usize, take: usize) -> Result<Vec<Vector>, BenchError> {
    let count = v["row_count"]
        .as_u64()
        .ok_or_else(|| bad("missing vector count"))?;
    if count == 0 || count > limit as u64 || v["byte_len"] != count * 768 * 4 {
        return Err(bad("invalid real vector count or byte length"));
    }
    let path = Path::new(
        v["path"]
            .as_str()
            .ok_or_else(|| bad("missing vector path"))?,
    );
    if fs::metadata(path).map_err(BenchError::harness)?.len() != count * 768 * 4 {
        return Err(bad("real vector file length mismatch"));
    }
    let mut reader = BufReader::new(fs::File::open(path).map_err(BenchError::harness)?);
    let mut rows = Vec::with_capacity((count as usize).min(take));
    let mut hash = CanonicalRowHasher::default();
    for i in 0..count as usize {
        let mut bytes = [0; 768 * 4];
        reader.read_exact(&mut bytes).map_err(BenchError::harness)?;
        let row = std::array::from_fn(|c| {
            f32::from_le_bytes(bytes[c * 4..c * 4 + 4].try_into().unwrap())
        });
        let valid =
            spherra_domain::ValidatedVector::new(row.to_vec()).map_err(BenchError::harness)?;
        if valid.direction_unreliable() {
            return Err(bad("unreliable real vector"));
        }
        hash.update(std::slice::from_ref(&row));
        if i < take {
            rows.push(row);
        }
    }
    if hash.hash() != v["blake3"] {
        return Err(bad("real vector BLAKE3 mismatch"));
    }
    if rows.len() != take.min(count as usize) {
        return Err(bad("incomplete real vectors"));
    }
    Ok(rows)
}
impl Dataset {
    pub fn load(o: &Options) -> Result<Self, BenchError> {
        let path = Path::new(o.require("dataset")?);
        let descriptor = checked_json(
            path,
            include_str!("../../../../docs/benchmarks/real-query-dataset.schema.json"),
        )?;
        let split = o.get("query-split").unwrap_or("tuning");
        if !["tuning", "test"].contains(&split) {
            return Err(bad("query-split must be tuning or test"));
        }
        let training_rows = number(o, "training-rows", 4096_usize)?;
        let queries = number(o, "queries", 200_usize)?;
        if !(344..=32768).contains(&training_rows)
            || training_rows > descriptor["calibration"]["row_count"].as_u64().unwrap() as usize
            || queries == 0
            || queries > descriptor[split]["row_count"].as_u64().unwrap() as usize
        {
            return Err(bad("invalid real training/query size"));
        }
        // The descriptor identifies separate files; verify text records too so
        // an accidental vector-only substitution cannot lose row provenance.
        for group in ["indexed", "calibration", "tuning", "test"] {
            let record = &descriptor[group];
            if hash_file(Path::new(record["records_path"].as_str().unwrap()))?
                != record["records_blake3"]
            {
                return Err(bad("real text record BLAKE3 mismatch"));
            }
        }
        if hash_file(Path::new(descriptor["qrels"]["path"].as_str().unwrap()))?
            != descriptor["qrels"]["blake3"]
        {
            return Err(bad("real relevance record BLAKE3 mismatch"));
        }
        let rows = read_vectors(&descriptor["indexed"], 1_000_000, usize::MAX)?;
        if rows.len() < 10 {
            return Err(bad("real index needs at least10 rows"));
        }
        let training = read_vectors(&descriptor["calibration"], 32768, training_rows)?;
        let query_vectors = read_vectors(&descriptor[split], 1000, queries)?;
        let hash = hash_file(path)?;
        Ok(Self {
            corpus_hash: hash_rows(&rows),
            query_hash: hash_rows(&query_vectors),
            calibration_hash: hash_rows(&training),
            rows,
            training,
            queries: query_vectors,
            source: json!({"kind":"real-query","descriptor":descriptor,"descriptor_path":path,"descriptor_blake3":hash,"query_split":split}),
            hash,
        })
    }
    pub fn build(&self, o: &Options, dir: &Path) -> Result<Value, BenchError> {
        let sidecar = dir.join("accuracy-build.json");
        let seed: u64 = o.require_parsed("seed")?;
        if number(o, "reuse", false)? {
            if fs::metadata(&sidecar).map_err(BenchError::harness)?.len() > 1024 * 1024 {
                return Err(bad("oversized accuracy build provenance"));
            }
            let b: Value =
                serde_json::from_slice(&fs::read(&sidecar).map_err(BenchError::harness)?)
                    .map_err(BenchError::serialize)?;
            if b["corpus_hash"] != self.corpus_hash
                || b["calibration_hash"] != self.calibration_hash
                || b["training_rows"] != self.training.len()
                || b["seed"] != seed
                || b["index_current_blake3"] != hash_file(&dir.join("CURRENT"))?
            {
                return Err(bad("real index build provenance mismatch"));
            }
            return Ok(b);
        }
        let revision = SourceRevision::capture();
        let start = Instant::now();
        let mut builder = IndexBuilder::create(
            dir,
            &self.training,
            CreateOptions {
                seed,
                validation_rows: None,
            },
        )
        .map_err(BenchError::harness)?;
        for (i, row) in self.rows.iter().enumerate() {
            if builder.push(row).map_err(BenchError::harness)?.get() != i as u64 {
                return Err(bad("real row identity mismatch"));
            }
        }
        let report = builder.commit().map_err(BenchError::harness)?;
        if report.rows_added() != self.rows.len() as u64 || !report.cleanup_complete() {
            return Err(bad("incomplete real index commit"));
        }
        let mut b = json!({"git_commit":revision.commit,"dirty_worktree":revision.dirty,"dataset_hash":self.hash,"corpus_hash":self.corpus_hash,"calibration_hash":self.calibration_hash,"training_rows":self.training.len(),"seed":seed,"index_current_blake3":hash_file(&dir.join("CURRENT"))?,"build_seconds":start.elapsed().as_secs_f64()});
        finish_revision(&mut b);
        write_output(&sidecar, &b)?;
        Ok(b)
    }
    pub fn oracle(&self, path: &Path) -> Result<(Vec<Vec<Neighbor>>, Value), BenchError> {
        let r = checked_json(
            path,
            include_str!("../../../../docs/benchmarks/real-query-oracle.schema.json"),
        )?;
        if r["dataset_hash"] != self.hash
            || r["corpus_hash"] != self.corpus_hash
            || r["query_hash"] != self.query_hash
            || r["query_count"] != self.queries.len()
            || r["rows_scored"] != self.rows.len()
        {
            return Err(bad("real oracle does not match dataset and query bytes"));
        }
        let artifact = Path::new(r["artifact_path"].as_str().unwrap());
        let length = 24 + self.queries.len() * (4 + self.rows.len().min(100) * 16);
        if fs::metadata(artifact).map_err(BenchError::harness)?.len() != length as u64
            || r["artifact_bytes"] != length
        {
            return Err(bad("real oracle length mismatch"));
        }
        let bytes = fs::read(artifact).map_err(BenchError::harness)?;
        let hash = blake3::hash(&bytes).to_hex().to_string();
        if r["artifact_blake3"] != hash {
            return Err(bad("real oracle BLAKE3 mismatch"));
        }
        let exact = decode_reference(&bytes, self.rows.len() as u64, self.queries.len())?;
        Ok((
            exact,
            json!({"kind":"pinned","report_path":path,"report_blake3":hash_file(path)?,"rankings_blake3":hash,"git_commit":r["git_commit"],"dirty_worktree":r["dirty_worktree"]}),
        ))
    }
}
pub(crate) fn oracle_reference(o: &Options) -> Result<(), BenchError> {
    allowed(
        o,
        &[
            "dataset",
            "queries",
            "query-split",
            "training-rows",
            "output",
        ],
    )?;
    let revision = SourceRevision::capture();
    let start = Instant::now();
    let data = Dataset::load(o)?;
    let mut oracle = StreamingOracle::new(&data.queries, 100).map_err(BenchError::harness)?;
    oracle.extend(&data.rows).map_err(BenchError::harness)?;
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
            return Err(bad("existing oracle artifact differs"));
        }
    } else {
        fs::write(&artifact, &bytes).map_err(BenchError::harness)?;
        let mut p = fs::metadata(&artifact)
            .map_err(BenchError::harness)?
            .permissions();
        p.set_readonly(true);
        fs::set_permissions(&artifact, p).map_err(BenchError::harness)?;
    }
    let mut r = json!({"schema_version":1,"kind":"real-query-oracle","dataset_hash":data.hash,"source":data.source,"corpus_hash":data.corpus_hash,"query_hash":data.query_hash,"query_count":data.queries.len(),"rows_scored":data.rows.len(),"k":100,"artifact_path":artifact,"artifact_bytes":bytes.len(),"artifact_blake3":hash,"git_commit":revision.commit,"dirty_worktree":revision.dirty,"machine":MachineProfile::capture(),"timestamp":timestamp_rfc3339_utc(),"command":format!("spherra-bench dataset-oracle {}",o.0.iter().map(|(k,v)|format!("--{k} {v}")).collect::<Vec<_>>().join(" ")),"elapsed_seconds":start.elapsed().as_secs_f64()});
    finish_revision(&mut r);
    let schema: Value = serde_json::from_str(include_str!(
        "../../../../docs/benchmarks/real-query-oracle.schema.json"
    ))
    .map_err(BenchError::serialize)?;
    validate_against_schema(&schema, &r).map_err(BenchError::harness)?;
    write_output(output, &r)
}
