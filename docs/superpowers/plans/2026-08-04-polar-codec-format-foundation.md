# Spherra polar codec and format foundation implementation plan

Status: **P4 technical plan approved by Codex and Claude Opus; Spherra naming amendment applied on 2026-08-04**
Revision: P4 plus naming amendment
Date: 2026-08-04

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build and measure the scalar Rust reference for the approved 768D transform, direct-int4 primary code, PQ96x8 residual, fixed comparison path, certified bounds, and versioned corruption-safe file foundation.

**Architecture:** This plan implements only M0 and M1 of the approved implementation specification. `spherra-domain` defines invariants, `spherra-simd` supplies the scalar mathematical oracle, `spherra-codec` owns encoding/scoring/certificates, `spherra-format` owns explicit durable bytes and checked readers, and `spherra-testkit`/`spherra-bench` own evidence. Production database, Raft, networking, HNSW, and sharding do not begin in this plan.

**Tech Stack:** Rust 1.88.0 edition 2024, an exact dated nightly used only for fuzzing, Cargo workspace, half, rand_chacha, zerocopy, positional file reads, crc32c, blake3, serde/serde_json for benchmark output only, thiserror, proptest, cargo-fuzz, cargo-nextest, and criterion or divan only after the correctness gate. The plan is executable as written even when the recommended orchestration sub-skills are unavailable.

---

## Scope and exit gate

This plan passes only when fresh evidence proves all of the following:

- The transform uses exactly two rounds of sign flips, one global permutation, and six normalized 128-point Hadamard blocks.
- Forward/inverse, norm, and dot-product properties hold within declared scalar FP32 tolerances.
- A primary code is exactly 384 logical bytes and a residual code is exactly 96 logical bytes.
- A primary scan has no API or file handle through which it can read residual data.
- Refined scoring implements `dot(T(normalize(q)), p + e)` with no reconstructed-vector normalization and no affine correction.
- Fixed-point tables and int64 accumulation conform to the scalar FP reference within the recorded rounding certificate.
- Certified lower/upper bounds enclose the stored-original score for every property/fuzz case.
- Durable readers reject wrong versions, truncation, bad offsets, overlap, invalid alignment, bad CRCs, and mismatched BLAKE3 without panic.
- A reproducible JSON measurement reports byte accounting, seeds and versions, corpus identity, recall, bound tightness, primary-scan time, and candidate-rerank time.
- Existing C++/Python experiments still pass, while documentation states exactly what they do and do not prove.

Every step named **Refresh project state** means the complete maintenance gate from `AGENTS.md`: update the guide, run Graphify incremental update, run `graphify check-update .`, complete semantic extraction when pending, and query the concepts changed by that task. A task cannot be committed or marked complete before that gate passes.

This is a combined M0-M1 plan. Task 1 establishes repository/tooling bootstrap; Task 8 completes the M0 corpus, exact-oracle, machine-profile, and result-schema deliverables after the codec/format APIs they measure exist. Therefore the complete M0 exit gate is assessed at the end of Task 8, not immediately after Task 1.

## Planned file map

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
.gitignore
.github/workflows/ci.yml
deny.toml
scripts/check_dependency_policy.py
scripts/ci.sh

crates/spherra-domain/
  Cargo.toml
  src/lib.rs
  src/error.rs
  src/ids.rs
  src/record.rs
  src/sequence.rs
  tests/contracts.rs

crates/spherra-simd/
  Cargo.toml
  src/lib.rs
  src/scalar.rs
  tests/scalar_transform.rs

crates/spherra-codec/
  Cargo.toml
  src/lib.rs
  src/spec.rs
  src/transform.rs
  src/int4.rs
  src/tiled_soa.rs
  src/pq96.rs
  src/scorer.rs
  src/certificate.rs
  tests/transform_contract.rs
  tests/codec_contract.rs
  tests/scorer_contract.rs
  tests/residual_access.rs

crates/spherra-format/
  Cargo.toml
  src/lib.rs
  src/error.rs
  src/header.rs
  src/section.rs
  src/writer.rs
  src/reader.rs
  tests/format_contract.rs
  tests/golden_compatibility.rs
  tests/fixtures/primary-v1-minimal.bin
  tests/fixtures/residual-v1-minimal.bin

crates/spherra-testkit/
  Cargo.toml
  src/lib.rs
  src/corpus.rs
  src/exact.rs
  src/machine.rs
  src/results.rs

crates/spherra-bench/
  Cargo.toml
  src/main.rs
  tests/measurement_contract.rs

fuzz/
  Cargo.toml
  fuzz_targets/format_open.rs
  fuzz_targets/bound_soundness.rs

docs/benchmarks/README.md
docs/benchmarks/codec-format-baseline.schema.json
tools/build_scifact_corpus.py
```

## Task 1: Establish version control and the pinned Rust workspace

**Files:**

- Create: `.gitignore`
- Create: `Cargo.toml`
- Create: `Cargo.lock`
- Create: `rust-toolchain.toml`
- Create: `.github/workflows/ci.yml`
- Create: `deny.toml`
- Create: `scripts/check_dependency_policy.py`
- Create: `scripts/ci.sh`
- Create: six root-workspace crate manifests plus the isolated fuzz manifest and minimal sources listed above
- Modify: `AGENTS.md`

- [x] **Step 1: Verify the initialized Git repository and absence of a Rust workspace**

Run:

```bash
test -d .git
test ! -f Cargo.toml
test "$(git branch --show-current)" = "main"
test "$(git remote get-url origin)" = "https://github.com/bhrugusetlur-art/spherra.git"
command -v brew
command -v rustup || true
```

Expected: Git is on `main`, `origin` matches the private Spherra repository, no Rust workspace exists, and Homebrew prints its path. Rustup may be absent in the starting environment. If the observed workspace differs, update this plan and `AGENTS.md` before continuing.

- [x] **Step 2: Verify the connected remote**

Run:

```bash
git remote -v
GIT_TERMINAL_PROMPT=0 git ls-remote origin
```

Expected: `origin` uses `https://github.com/bhrugusetlur-art/spherra.git` for fetch and push, and `ls-remote` succeeds. Do not alter or replace the configured remote during this step.

- [x] **Step 3: Install and pin the stable Rust toolchain components**

Run:

```bash
if ! command -v rustup >/dev/null 2>&1; then
  brew install rustup
  export PATH="$(brew --prefix rustup)/bin:$PATH"
fi
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
rustup toolchain install 1.88.0 --profile minimal --component rustfmt --component clippy
rustup toolchain install nightly-2025-06-26 --profile minimal
rustc +1.88.0 --version
rustc +nightly-2025-06-26 --version
cargo +1.88.0 install cargo-nextest --version 0.9.114 --locked
cargo +nightly-2025-06-26 install cargo-fuzz --version 0.13.2 --locked --ignore-rust-version
cargo +1.88.0 install cargo-deny --version 0.20.2 --locked
```

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.88.0"
profile = "minimal"
components = ["rustfmt", "clippy"]
```

Expected: both compilers and all three Cargo subcommands report versions. Versions cargo-nextest 0.9.114, cargo-fuzz 0.13.2, and cargo-deny 0.20.2 were verified as published on crates.io on 2026-08-04. Record all tool versions in the first benchmark result; compiler versions are evidence metadata, not durable-format identities.

Tooling-only reproducibility amendment (2026-08-04): Homebrew's current keg-only `rustup` formula no longer provides `rustup-init`, and does not add `~/.cargo/bin` to a non-login shell. The revised bootstrap commands above use the installed formula directly and explicitly expose Cargo's bin directory. The locked dependency graph for `cargo-fuzz 0.13.2` resolves `cargo-platform 0.3.3`, which currently declares Rust 1.91 while the required nightly reports Rust 1.90.0-nightly. Retain the exact approved cargo-fuzz and nightly versions and use Cargo's `--ignore-rust-version`; record both the unamended command's exit 101 and the successful fallback. This does not alter architecture, dependencies in the root workspace, fuzz target behavior, or any durable-format decision.

- [x] **Step 4: Create the root workspace manifest**

Create `Cargo.toml`:

```toml
[workspace]
resolver = "3"
exclude = ["fuzz"]
members = [
  "crates/spherra-domain",
  "crates/spherra-simd",
  "crates/spherra-codec",
  "crates/spherra-format",
  "crates/spherra-testkit",
  "crates/spherra-bench",
]

[workspace.package]
edition = "2024"
license = "Apache-2.0"
rust-version = "1.88"

[workspace.dependencies]
blake3 = "1"
crc32c = "0.6"
half = "2"
proptest = "1"
rand = "0.9"
rand_chacha = "0.9"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sysinfo = "0.37"
tempfile = "3"
thiserror = "2"
zerocopy = { version = "0.8", features = ["derive"] }

[workspace.lints.rust]
unsafe_code = "deny"
warnings = "deny"

[workspace.lints.clippy]
all = "deny"
pedantic = "allow"
```

The root policy denies unsafe code. Libraries use `#![deny(unsafe_code)]`, not `forbid`, so a later milestone can permit unsafe only inside a named audited SIMD or mmap module after its differential/lifecycle tests exist. M1 uses positional reads and contains no unsafe exception.

- [x] **Step 5: Create focused crates and dependency direction**

Use package names matching their directories. Every manifest inherits workspace edition, license, rust-version, and lints. Dependency rules:

```text
spherra-domain: no project dependency
spherra-simd: spherra-domain
spherra-codec: spherra-domain + spherra-simd
spherra-format: spherra-domain
spherra-testkit: spherra-domain + spherra-codec + spherra-format
spherra-bench: spherra-domain + spherra-codec + spherra-format + spherra-testkit
```

Each library initially contains only `#![deny(unsafe_code)]`. `spherra-bench/src/main.rs` initially contains `fn main() {}`. The format crate stores codec/scorer/transform/layout identities as opaque fixed-width bytes and integers rather than importing codec types.

Create `fuzz/Cargo.toml` with its own empty `[workspace]` table, `cargo-fuzz = true` package metadata, `libfuzzer-sys` and `arbitrary` dependencies, path dependencies on `spherra-codec` and `spherra-format`, and the two named binary targets. Create both target files as compiling byte-input smoke targets:

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    std::hint::black_box(data);
});
```

Tasks 6 and 7 replace the smoke bodies with their soundness/parser properties. The root workspace excludes `fuzz`, so fuzz dependencies do not enter normal workspace resolution.

- [x] **Step 6: Create `.gitignore`**

```gitignore
/target/
/.idea/
/.vscode/
*.profraw
*.profdata
__pycache__/
*.pyc
/graphify-out/.graphify_python
/graphify-out/.graphify_root
/graphify-out/.vocab.txt
/graphify-out/cache/
/graphify-out/cost.json
/fuzz/target/
/fuzz/artifacts/
```

Do not ignore the root or fuzz `Cargo.lock`, `fuzz/corpus/` regression seeds, benchmark schemas, golden fixtures, raw result JSON selected as project evidence, `AGENTS.md`, or the tracked Graphify graph/report/manifest/memory.

- [x] **Step 7: Add and run the explicit dependency-policy check**

Create `scripts/check_dependency_policy.py`. It loads `cargo metadata --format-version 1`, selects workspace packages whose names begin with `spherra-`, inspects direct normal/build dependencies, and constructs its allowed set exactly like this:

```python
workspace_ids = set(metadata["workspace_members"])
spherra_packages = {
    package["name"]
    for package in metadata["packages"]
    if package["id"] in workspace_ids and package["name"].startswith("spherra-")
}
ALLOWED = {(package, "spherra-domain") for package in spherra_packages if package != "spherra-domain"}
ALLOWED |= {
    ("spherra-codec", "spherra-simd"),
    ("spherra-testkit", "spherra-codec"),
    ("spherra-testkit", "spherra-format"),
    ("spherra-bench", "spherra-codec"),
    ("spherra-bench", "spherra-format"),
    ("spherra-bench", "spherra-testkit"),
}
```

The script enforces normal/build edges; dev-dependencies are outside this M0 architectural check. It prints each invalid edge and exits 1; otherwise it prints `dependency policy passed`.

Run:

```bash
python3 scripts/check_dependency_policy.py
```

Expected: `dependency policy passed`.

- [x] **Step 8: Resolve and lock dependencies**

Run:

```bash
cargo generate-lockfile
cargo metadata --locked --no-deps --format-version 1 > /tmp/spherra-metadata.json
cargo tree --workspace --locked
```

Expected: all workspace members resolve, `Cargo.lock` exists, and no lower-level crate depends on `spherra-bench` or `spherra-testkit` outside dev-dependencies.

- [x] **Step 9: Add local CI and dependency policy**

Create `deny.toml` permitting Apache-2.0, MIT, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, and Zlib licenses, denying unknown registries/sources. Create `scripts/ci.sh` as the single local CI entry point:

```bash
#!/usr/bin/env bash
set -euo pipefail
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked
cargo nextest run --workspace --all-features --locked --no-tests=pass
cargo test --workspace --doc --locked
python3 scripts/check_dependency_policy.py
cargo deny check licenses bans sources
```

The dependency-policy script resolves `cargo` from `PATH` first, then from `${CARGO_HOME:-$HOME/.cargo}/bin` with a clear error if neither exists. This and the local-CI path export preserve the literal gate commands on Homebrew's keg-only Rust installation.

Create `.github/workflows/ci.yml` that checks out the repository, installs Rust 1.88.0 with rustfmt/clippy, restores Cargo caches, installs cargo-nextest 0.9.114 and cargo-deny 0.20.2, and runs `bash scripts/ci.sh`. Run `cargo deny check advisories` as a separately labeled security job whose advisory-database timestamp is recorded; it is not part of mathematical/storage reproducibility. Long fuzzing and performance measurements remain local recorded gates because hosted runners do not represent the M1 Pro target.

- [x] **Step 10: Run the empty-workspace quality gate**

Run:

```bash
bash scripts/ci.sh
cargo test --workspace --all-features --locked
```

Expected: both commands exit 0.

- [x] **Step 11: Update living project state and Graphify**

Update `AGENTS.md` to mark Git/workspace bootstrap complete and Task 2 as next. Run the mandatory Graphify refresh and query `Rust workspace codec format foundation`.

- [x] **Step 12: Commit the bootstrap**

Run:

```bash
git add AGENTS.md Cargo.toml Cargo.lock rust-toolchain.toml .gitignore .github \
  deny.toml scripts crates docs experiments fuzz \
  graphify-out/graph.json graphify-out/GRAPH_REPORT.md graphify-out/manifest.json \
  graphify-out/.graphify_labels.json graphify-out/memory
git commit -m "chore: bootstrap Spherra Rust workspace"
```

Expected: one baseline commit containing the approved docs, project guide, graph, and empty workspace; no remote interaction.

## Task 2: Encode domain invariants before codec code

**Files:**

- Create: `crates/spherra-domain/src/error.rs`
- Create: `crates/spherra-domain/src/ids.rs`
- Create: `crates/spherra-domain/src/record.rs`
- Create: `crates/spherra-domain/src/sequence.rs`
- Modify: `crates/spherra-domain/src/lib.rs`
- Test: `crates/spherra-domain/tests/contracts.rs`

- [x] **Step 1: Write failing contract tests**

The test must assert:

```rust
use spherra_domain::{ChunkId, DocumentId, PutSeq, ValidatedVector, DIMENSION};

#[test]
fn dimension_and_sequence_contracts_are_stable() {
    assert_eq!(DIMENSION, 768);
    let seq = PutSeq::new(7, (1_u64 << 48) - 1).unwrap();
    assert_eq!(seq.epoch(), 7);
    assert_eq!(seq.index(), (1_u64 << 48) - 1);
    assert!(PutSeq::new(7, 1_u64 << 48).is_err());
    assert_ne!(ChunkId::from_u128(1), ChunkId::from_u128(2));
    assert_ne!(DocumentId::from_u128(1), DocumentId::from_u128(2));
}

#[test]
fn vector_validation_rejects_non_finite_and_marks_small_norms() {
    assert!(ValidatedVector::new(vec![0.0; 767]).is_err());
    let mut invalid = vec![0.0; DIMENSION];
    invalid[3] = f32::NAN;
    assert!(ValidatedVector::new(invalid).is_err());
    let zero = ValidatedVector::new(vec![0.0; DIMENSION]).unwrap();
    assert!(zero.direction_unreliable());
    assert!(zero.normalized_direction().is_none());
}
```

- [x] **Step 2: Run the tests and observe failure**

Run:

```bash
cargo test -p spherra-domain --test contracts --locked
```

Expected: compilation fails because the public contracts do not exist.

- [x] **Step 3: Implement the domain types**

Use `DIMENSION: usize = 768`, distinct 128-bit newtypes for chunk/document IDs, and this sequence representation:

```rust
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PutSeq(u64);

impl PutSeq {
    pub const MAX_INDEX: u64 = (1_u64 << 48) - 1;

    pub fn new(epoch: u16, index: u64) -> Result<Self, DomainError> {
        if index > Self::MAX_INDEX {
            return Err(DomainError::SequenceOverflow);
        }
        Ok(Self((u64::from(epoch) << 48) | index))
    }

    pub const fn epoch(self) -> u16 { (self.0 >> 48) as u16 }
    pub const fn index(self) -> u64 { self.0 & Self::MAX_INDEX }
    pub const fn raw(self) -> u64 { self.0 }
}
```

`ValidatedVector::new` requires exactly 768 finite components, computes the L2 norm in FP64, rejects norms above `half::f16::MAX`, stores the original FP16 radius, and marks norms below the collection `min_norm_epsilon` as direction-unreliable. Provide a constructor with explicit epsilon for tests and a default of `1e-12`. `normalized_direction()` returns `None` for an unreliable direction, and every transform/codec entry point requires the `ReliableDirection` newtype so zero or near-zero vectors cannot be normalized accidentally. Such rows remain eligible only for the future non-routable radius/metadata partition.

- [x] **Step 4: Run domain tests and quality checks**

Run:

```bash
cargo test -p spherra-domain --test contracts --locked
cargo clippy -p spherra-domain --all-targets --locked
```

Expected: tests pass and clippy exits 0.

- [x] **Step 5: Update project state, refresh Graphify, and commit**

Commit message:

```text
feat(domain): define vector and sequence invariants
```

## Task 3: Implement the exact scalar transform oracle

**Files:**

- Create: `crates/spherra-simd/src/scalar.rs`
- Modify: `crates/spherra-simd/src/lib.rs`
- Test: `crates/spherra-simd/tests/scalar_transform.rs`
- Create: `crates/spherra-codec/src/spec.rs`
- Create: `crates/spherra-codec/src/transform.rs`
- Modify: `crates/spherra-codec/src/lib.rs`
- Test: `crates/spherra-codec/tests/transform_contract.rs`

- [ ] **Step 1: Write failing mathematical property tests**

Tests use deterministic random vectors and assert:

```rust
const TOLERANCE: f32 = 2.0e-4;

proptest! {
    #[test]
    fn transform_preserves_norm_and_inverts(seed in any::<u64>(), raw in prop::collection::vec(-4.0f32..4.0, 768)) {
        let plan = TransformPlan::from_seed(seed);
        let x: [f32; 768] = raw.try_into().unwrap();
        let tx = transform(&plan, &x);
        let recovered = inverse_transform(&plan, &tx);
        prop_assert!((l2(&x) - l2(&tx)).abs() <= TOLERANCE * l2(&x).max(1.0));
        prop_assert!(max_abs_diff(&x, &recovered) <= TOLERANCE);
    }
}
```

Add a dot-preservation test and a structural test that records two rounds, six blocks/round, block length 128, and dimension 768 without padding.

- [ ] **Step 2: Observe the tests fail**

Run:

```bash
cargo test -p spherra-simd -p spherra-codec --test scalar_transform --test transform_contract --locked
```

Expected: missing types/functions cause compilation failure.

- [ ] **Step 3: Implement normalized 128-point Hadamard**

`spherra-simd::scalar::hadamard_128` performs seven butterfly stages and multiplies every output by `1.0 / sqrt(128.0)`. Reject any general-length API; the fixed `[f32; 128]` type prevents padding or accidental size changes.

- [ ] **Step 4: Implement deterministic two-round transform identity**

`TransformPlan::from_seed` expands the collection seed into two 32-byte ChaCha20 seeds plus a BLAKE3 identity and caches the derived tables. For each round, use one deterministic RNG stream to create 768 sign bits and a Fisher-Yates permutation of `0..768`.

Forward round order is:

```text
sign flip -> global permutation -> six normalized H128 blocks
```

Inverse round order, processing rounds in reverse, is:

```text
six normalized H128 blocks -> inverse global permutation -> same sign flip
```

Cache derived signs/permutations in an immutable `TransformPlan`; do not regenerate them per vector.

- [ ] **Step 5: Run transform tests**

Run:

```bash
cargo test -p spherra-simd -p spherra-codec --test scalar_transform --test transform_contract --locked
```

Expected: structural, determinism, inverse, norm, and dot tests pass.

- [ ] **Step 6: Refresh project state and commit**

Commit message:

```text
feat(codec): add scalar two-round transform oracle
```

## Task 4: Implement direct-int4 and TILED_SOA_32

**Files:**

- Create: `crates/spherra-codec/src/int4.rs`
- Create: `crates/spherra-codec/src/tiled_soa.rs`
- Modify: `crates/spherra-codec/src/lib.rs`
- Test: `crates/spherra-codec/tests/codec_contract.rs`

- [ ] **Step 1: Write failing byte and layout tests**

Tests must prove:

```rust
assert_eq!(DirectCode::BYTE_LEN, 384);
assert_eq!(RadiusFlags::BYTE_LEN, 4);
assert_eq!(TiledSoa32::direction_bytes_for_full_tile(), 12_288);
```

They also encode/decode endpoints and random codes, compare a full-tile scan with per-row scalar scoring, and test a 1-row and 31-row tail while reporting physical padding separately from logical payload.

- [ ] **Step 2: Observe failure**

Run:

```bash
cargo test -p spherra-codec --test codec_contract --locked
```

Expected: missing direct-code and layout types.

- [ ] **Step 3: Implement quantizer training and nibble packing**

The scalar reference trainer sorts calibration values independently for each transformed coordinate and selects 16 deterministic quantile centers. Ties preserve lower code order. The table is `[f32; 768 * 16]` and is identified by BLAKE3 over canonical little-endian FP32 bytes.

Encoding selects the nearest center with ties going to the smaller code. Pack even coordinate `i` into the low nibble and `i + 1` into the high nibble. Decode and score use the table from the codec header, not a hard-coded `[-1, 1]` mapping.

- [ ] **Step 4: Implement TILED_SOA_32**

Within each 32-row tile, store coordinate-major nibbles. For a full tile each coordinate consumes 16 bytes and 768 coordinates consume 12,288 bytes. Tail tiles are zero padded physically, while row count controls visibility and byte accounting reports both logical and physical bytes.

- [ ] **Step 5: Run layout tests**

Run:

```bash
cargo test -p spherra-codec --test codec_contract --locked
```

Expected: exact byte, round-trip, tie-breaking, full-tile, and tail tests pass.

- [ ] **Step 6: Refresh project state and commit**

Commit message:

```text
feat(codec): add direct int4 tiled scalar layout
```

## Task 5: Implement PQ96x8 residual refinement

**Files:**

- Create: `crates/spherra-codec/src/pq96.rs`
- Modify: `crates/spherra-codec/src/lib.rs`
- Test: `crates/spherra-codec/tests/codec_contract.rs`
- Test: `crates/spherra-codec/tests/residual_access.rs`

- [ ] **Step 1: Write failing residual contract tests**

Assert:

```rust
assert_eq!(Pq96Code::SUBQUANTIZERS, 96);
assert_eq!(Pq96Code::SUBVECTOR_DIMENSION, 8);
assert_eq!(Pq96Code::CENTROIDS, 256);
assert_eq!(Pq96Code::BYTE_LEN, 96);
```

Train twice with the same seed and calibration corpus and require byte-identical canonical codebooks. Encode a known residual, decode it, and verify each byte selects one centroid from the corresponding 8D subspace.

- [ ] **Step 2: Observe failure**

Run:

```bash
cargo test -p spherra-codec --test codec_contract --test residual_access --locked
```

Expected: PQ types and residual boundary are absent.

- [ ] **Step 3: Implement deterministic PQ training**

For each of 96 contiguous 8D subvectors:

1. Select 256 initial centroids with deterministic seeded k-means++.
2. Run exactly 25 Lloyd iterations or stop early when assignments no longer change.
3. Accumulate centroid sums in FP64 in calibration-row order.
4. Resolve empty clusters by selecting the row with the largest current squared error; ties use the lower row index.
5. Serialize centroids as canonical little-endian FP32 and identify the codebook with BLAKE3.

The training input is the residual `T(normalize(x)) - p`, not the original vector and not a normalized reconstruction.

- [ ] **Step 4: Implement candidate-only access separation**

Define separate traits:

```rust
pub trait PrimaryCodes {
    fn scan_primary(
        &self,
        rows: core::ops::Range<u32>,
        query: &PreparedQuery,
        out: &mut [PrimaryScore],
    ) -> Result<usize, CodecError>;
}

pub trait ResidualCodes {
    fn load_residual(&self, row: u32) -> Result<Pq96Code, CodecError>;
}
```

The primary scan function accepts only `&dyn PrimaryCodes`, a bounded row range, and an output capacity; it returns the number written. The reranker accepts both sources and a bounded candidate slice. In `residual_access.rs`, use a spy residual source and assert zero loads during primary scan and exactly one load per reranked candidate.

- [ ] **Step 5: Run PQ and access tests**

Run:

```bash
cargo test -p spherra-codec --test codec_contract --test residual_access --locked
```

Expected: deterministic training, 96-byte shape, decode semantics, and candidate-only access tests pass.

- [ ] **Step 6: Refresh project state and commit**

Commit message:

```text
feat(codec): add deterministic PQ96x8 residual
```

## Task 6: Implement fixed-point scoring and certified bounds

**Files:**

- Create: `crates/spherra-codec/src/scorer.rs`
- Create: `crates/spherra-codec/src/certificate.rs`
- Modify: `crates/spherra-codec/src/lib.rs`
- Test: `crates/spherra-codec/tests/scorer_contract.rs`
- Test: `fuzz/fuzz_targets/bound_soundness.rs`

- [ ] **Step 1: Write failing scorer-conformance tests**

Normalize the raw query and stored original in FP64 using the same specified reduction/division order. For normalized original-space query `q`, normalized original-space vector `x`, primary reconstruction `p`, and PQ reconstruction `e`, define the truth value before any transform arithmetic:

```rust
let true_score = dot_f64(&q, &x);
let tq = transform(&plan, &q);
let oracle_primary = dot_f64(&tq, &p);
let oracle_refined = dot_f64(&tq, &add(&p, &e));
```

Require the serving scorer to match its transformed-space oracle within its constructive scoring term, and require the full certificate to enclose the original-space `true_score`. Add a regression case proving that normalizing `p + e` changes the result and that the serving scorer returns the unnormalized value.

- [ ] **Step 2: Write the failing certificate property**

For every generated vector/query pair:

```rust
assert!(primary.lower <= true_score && true_score <= primary.upper);
assert!(refined.lower <= true_score && true_score <= refined.upper);
```

Let `delta_t` be a constructive, outward-rounded upper bound on the complete path from the FP64 normalized unit vector through its kernel-input conversion and implemented transform, relative to the exact mathematical transform of that same FP64 unit vector. Use:

```text
eta_transform_dot = 2*delta_t + delta_t^2
query_norm_upper  = 1 + delta_t
epsilon           = eta_transform_dot
                  + query_norm_upper * max_reconstruction_l2_error
                  + eta_serving_score
```

Compute `max_reconstruction_l2_error` against the exact mathematical transformed original, not merely the implemented transformed buffer. `eta_serving_score` covers query-table quantization, table rounding, int64 accumulation, and comparison-scale conversion. Outward-round every operation. The exhaustive reconstruction maximum is computed while original FP32 vectors are present; no sampled maximum may satisfy this API.

- [ ] **Step 3: Observe failure**

Run:

```bash
cargo test -p spherra-codec --test scorer_contract --locked
```

Expected: missing scorer/certificate implementations.

- [ ] **Step 4: Implement fixed-point tables and int64 accumulation**

Choose one `scorer_version` scale by measuring the maximum table entry and proving the worst-case sum of 768 primary terms plus 96 residual terms cannot overflow int64. Generate query-by-code lookup tables once per prepared query. Accumulate in deterministic coordinate/subquantizer order. Store the selected fractional-bit count and proof inputs in the scorer metadata and benchmark JSON.

- [ ] **Step 5: Implement constructive rounding terms and outward bounds**

Specify one FP64 normalization algorithm for truth, serving query preparation, and certificate construction. Derive `delta_t` from the FP64-to-kernel conversion, fixed seven-stage normalized H128 arithmetic, and two-round sign/permutation composition, using an operation-count FP error model with FP64 evaluation and outward conversion. Validate the derivation against adversarial basis, alternating-sign, dense-equal, and randomized vectors, but do not replace the constructive bound with their sampled maximum. For each original build vector, compute `||T_implemented(normalize_fp64(x)) - p||2` (or `p + e`) in FP64, add `delta_t` with outward rounding, and use that value as the conservative exact-transform reconstruction error; then take the exhaustive block maximum. `eta_primary_score` and `eta_rerank_score` include query-table quantization, table-entry rounding, deterministic int64 accumulation, and int64-to-comparison conversion. Use next-representable-float outward rounding for every endpoint. A certificate constructor requires an iterator over every original vector in the block and returns the exhaustive maximum reconstruction error plus the transform and scorer terms.

- [ ] **Step 6: Run property tests and a bounded fuzz smoke**

Run:

```bash
cargo test -p spherra-codec --test scorer_contract --locked
cargo +nightly-2025-06-26 fuzz run bound_soundness -- -max_total_time=60
```

Expected: no conformance or enclosure failure. Preserve every discovered fuzz seed as a regression fixture before fixing a failure.

- [ ] **Step 7: Refresh project state and commit**

Commit message:

```text
feat(codec): add deterministic scoring certificates
```

## Task 7: Define the versioned segment foundation and checked reader

**Files:**

- Create: `crates/spherra-format/src/error.rs`
- Create: `crates/spherra-format/src/header.rs`
- Create: `crates/spherra-format/src/section.rs`
- Create: `crates/spherra-format/src/writer.rs`
- Create: `crates/spherra-format/src/reader.rs`
- Modify: `crates/spherra-format/src/lib.rs`
- Test: `crates/spherra-format/tests/format_contract.rs`
- Test: `crates/spherra-format/tests/golden_compatibility.rs`
- Test: `crates/spherra-format/tests/fixtures/primary-v1-minimal.bin`
- Test: `crates/spherra-format/tests/fixtures/residual-v1-minimal.bin`
- Test: `fuzz/fuzz_targets/format_open.rs`

- [ ] **Step 1: Write failing format rejection tests**

Construct byte arrays for a minimal file and assert rejection of:

- Wrong magic, kind, major version, dimension, codec, scorer, or layout.
- Header/section length overflow.
- Section outside file, overlapping sections, non-monotonic directory, invalid alignment, or row-count mismatch.
- Truncation at every byte boundary.
- Bad block CRC32C or whole-file BLAKE3.

- [ ] **Step 2: Define the explicit canonical header**

The v1 header contains fixed-width little-endian fields for:

```text
magic[8], major:u16, minor:u16, file_kind:u16, header_len:u32,
collection_id[16], segment_id[16], dimension:u16, row_count:u32,
codec_id[32], scorer_version:u32,
transform_id[32], quantizer_id[32], pq_codebook_id[32], layout_id:u16,
section_count:u16, section_directory_offset:u64, payload_len:u64,
whole_file_blake3[32]
```

The section directory contains kind, flags, alignment, offset, length, logical row count, block size, CRC-table section index, and section-level identity. Every independently read data section has a paired CRC-table section containing one canonical little-endian u32 CRC32C per logical block in block order; the final partial block is checksummed over only its logical bytes. Reserve distinct section kinds for IDs/sequences, radius/flags, direct-int4 primary, int4 quantizer table, primary certificate, refined certificate, PQ96x8 residual, PQ codebook, and CRC tables. The primary file carries the quantizer table; the residual file carries its PQ codebook. Do not serialize Rust structs by memory layout; encode/decode every field explicitly.

- [ ] **Step 3: Observe test failure**

Run:

```bash
cargo test -p spherra-format --test format_contract --locked
```

Expected: format APIs do not exist.

- [ ] **Step 4: Implement writer and reader boundary**

The writer produces a temporary file, writes explicit headers/sections, records CRC32C per independently read block, computes whole-file BLAKE3 with the identity field zeroed, and returns a staged-file descriptor. Durable temp/sync/rename/directory-sync belongs to the later LSM milestone, not this pure format writer.

M1 uses positional reads only. A private checked-file helper validates the complete header and directory before exposing data. It constructs one of two public capability types based on `file_kind`:

```rust
pub struct PrimaryFileReader(SegmentReaderCore);
pub struct ResidualFileReader(SegmentReaderCore);
```

`PrimaryFileReader` exposes IDs/sequences, radius/flags, primary codes, quantizer tables, and certificate blocks. `ResidualFileReader` keeps its PQ96x8 row accessor crate-private. A primary file containing a residual section—or a residual file containing a primary section—is rejected. `PairedSegmentReaders::open` validates equality of collection, `segment_id`, dimension, row count, transform, codec, quantizer, PQ codebook, scorer, and layout identities before returning the only public candidate-rerank accessor; no residual read API is publicly reachable before pairing succeeds. Mmap is deferred to M2, where it may exist only inside the private `SegmentReaderCore` audited unsafe module with reader-lifetime, unlink, truncation, and SIGBUS-risk tests. Never mmap-write.

- [ ] **Step 5: Create and pin the minimal golden fixture**

Generate two two-row files with the same fixed collection and segment IDs and matching identities. `primary-v1-minimal.bin` contains fixed IDs/sequences, radius/flags, certificates, quantizer table, and one TILED_SOA_32 tail tile. `residual-v1-minimal.bin` contains its PQ codebook and exactly two PQ96 codes. Check in both byte sequences and test their independent capability readers. Tests must prove that swapping file kinds, embedding a residual section in the primary file, or embedding a primary section in the residual file is rejected. Generate valid residual files in the test with a different collection ID and with a different segment ID, each with valid BLAKE3, and assert pairing rejects both before any residual read. Regenerate both canonical fixtures in memory and compare every byte to the checked-in files to enforce writer determinism. A format change must continue reading both fixtures or deliberately change the major version and add a migration/compatibility decision.

- [ ] **Step 6: Run format tests and fuzz smoke**

Run:

```bash
cargo test -p spherra-format --test format_contract --test golden_compatibility --locked
cargo +nightly-2025-06-26 fuzz run format_open -- -max_total_time=60
```

Expected: all contract/golden tests pass and fuzzing reports no panic or memory violation.

- [ ] **Step 7: Refresh project state and commit**

Commit message:

```text
feat(format): add versioned checked segment foundation
```

## Task 8: Build reproducible corpus, exact-oracle, and measurement tooling

**Files:**

- Create: `crates/spherra-testkit/src/corpus.rs`
- Create: `crates/spherra-testkit/src/exact.rs`
- Create: `crates/spherra-testkit/src/machine.rs`
- Create: `crates/spherra-testkit/src/results.rs`
- Modify: `crates/spherra-testkit/src/lib.rs`
- Create: `crates/spherra-bench/src/main.rs`
- Test: `crates/spherra-bench/tests/measurement_contract.rs`
- Create: `docs/benchmarks/README.md`
- Create: `docs/benchmarks/codec-format-baseline.schema.json`
- Create: `tools/build_scifact_corpus.py`

- [ ] **Step 1: Write failing measurement-schema tests**

The output JSON must require:

```text
schema_version, timestamp, git_commit, dirty_worktree,
os, architecture, cpu, physical_memory_bytes, rustc, cargo_profile,
cache_state, durability_mode,
command, seed, corpus_name, corpus_hash, dimension, vector_count, query_count,
transform_id, codec_id, scorer_version, quantizer_id, pq_codebook_id, layout_id,
logical_primary_bytes_per_vector, physical_primary_bytes, tail_padding_bytes,
logical_residual_bytes_per_vector, header_bytes,
recall_at_10, recall_at_100, candidate_budget,
primary_scan_vectors_per_second, residual_reranks_per_second,
primary_bound_violation_count, refined_bound_violation_count,
primary_bound_width_percentiles, refined_bound_width_percentiles
```

The test deserializes a generated result, validates the JSON Schema, and rejects omitted identity, corpus, byte, or soundness fields.

- [ ] **Step 2: Implement deterministic corpus descriptors and exact top-k**

Support generated Gaussian/correlated smoke corpora and file-backed real 768D FP32 corpora. A file-backed descriptor records path, byte length, BLAKE3, row count, dimension, normalization policy, source dataset revision, embedding model revision, and license. Exact top-k uses FP64 dot accumulation and deterministic ordering by score descending then row ID.

Create `tools/build_scifact_corpus.py` as the default reproducible real-corpus builder. It downloads the public BEIR SciFact corpus through the `mteb` dataset loader, embeds corpus text with `sentence-transformers/all-mpnet-base-v2` at 768 dimensions, writes normalized row-major FP32 bytes, and emits a descriptor containing the exact dataset/model revisions and BLAKE3. If the runtime cannot resolve immutable upstream revisions, the builder exits nonzero rather than creating unpinned evidence.

SciFact is a smoke-scale real corpus, not sufficient evidence for a 10M format freeze. `docs/benchmarks/README.md` must label its results accordingly. M1 can prove harness correctness with it, but the R7 quality gate remains open until multiple real corpora, including at least one with 100,000 or more vectors, are recorded.

Do not commit proprietary or private corpus contents. Commit descriptors and hashes only when licensing permits reproducible retrieval.

- [ ] **Step 3: Implement machine profile capture**

Use the `sysinfo` crate for CPU and physical-memory metadata and Rust standard APIs for OS/architecture plus the active toolchain. Do not shell out to commands that may include user paths or secrets. M1 emits `cache_state` as `cold`, `warm`, or `hot` and `durability_mode` as `not-applicable`; later storage milestones replace the latter with the actual fsync mode.

- [ ] **Step 4: Implement `spherra-bench codec-format`**

Required arguments:

```text
--corpus <descriptor-or-generated-name>
--queries <count>
--seed <u64>
--candidate-budget <comma-separated-list>
--layout tiled-soa-32
--output <json-path>
```

The command trains on a disjoint calibration split, computes exact neighbors, encodes the corpus, runs primary scan plus bounded residual rerank, verifies every certificate during the measurement, and writes one result per candidate budget. It exits nonzero on any bound violation or identity mismatch.

Also implement `spherra-bench certify --trials <u64> --seed <u64> --transform-seeds <u32> --output <json-path>`. It deterministically generates normalized original-space vector/query pairs across the requested number of transform seeds, exercises primary and refined certificates, writes progress every 100,000 trials, and exits at the first enclosure failure with a reproducible seed tuple. Its JSON schema requires schema/toolchain versions, command, root seed, transform-seed count and list, requested/completed trial counts, primary/refined violation counts, maximum observed normalized certificate slack, elapsed time, and machine profile.

- [ ] **Step 5: Run schema and smoke measurements**

Run:

```bash
cargo test -p spherra-bench --test measurement_contract --locked
cargo run -p spherra-bench --release --locked -- codec-format \
  --corpus generated-correlated-768x20000 \
  --queries 200 \
  --seed 20260804 \
  --candidate-budget 10,20,50,100,200 \
  --layout tiled-soa-32 \
  --output target/measure/codec-format-smoke.json
```

Expected: schema validation passes, result file contains five candidate-budget entries, and both bound-violation counts are zero.

- [ ] **Step 6: Refresh project state and commit**

Commit message:

```text
feat(bench): add reproducible codec format harness
```

## Task 9: Re-run historical evidence and execute the complete M1 gate

**Files:**

- Modify: `experiments/codec_bench/README.md`
- Modify: `docs/benchmarks/README.md`
- Create: one dated result JSON under `docs/benchmarks/results/` after verifying it contains no private path or corpus data
- Modify: `AGENTS.md`

- [ ] **Step 1: Run the existing native and Python correctness tests**

Run:

```bash
clang++ -std=c++20 -O3 -mcpu=native -DNDEBUG -Wall -Wextra -Werror \
  experiments/codec_bench/codec_test.cpp \
  experiments/codec_bench/codec.cpp \
  -o /tmp/spherra_codec_test
/tmp/spherra_codec_test

uv run --python 3.12 --with numpy --with scipy \
  python experiments/codec_bench/accuracy_test.py
```

Expected: native output is `codec tests passed`; Python reports all tests passing.

- [ ] **Step 2: Run the complete Rust correctness gate**

Run:

```bash
bash scripts/ci.sh
cargo test --workspace --all-features --locked
cargo deny check advisories
```

Expected: zero failed tests, formatting differences, clippy diagnostics, doc-test failures, dependency-policy violations, license/source violations, or current advisories. Record the advisory-database timestamp separately from the reproducible soundness result.

- [ ] **Step 3: Run bounded fuzz gates**

Run:

```bash
cargo +nightly-2025-06-26 fuzz run format_open -- -max_total_time=300
cargo +nightly-2025-06-26 fuzz run bound_soundness -- -max_total_time=300
```

Expected: no crash, panic, memory error, or bound violation.

- [ ] **Step 4: Run the deterministic two-million-trial certificate soak**

Run:

```bash
cargo run -p spherra-bench --release --locked -- certify \
  --trials 2000000 \
  --seed 20260804 \
  --transform-seeds 64 \
  --output target/measure/certificate-soak.json
```

Expected: exactly 2,000,000 trials complete, both violation counters are zero, and all 64 transform seeds appear in the result.

- [ ] **Step 5: Run the recorded scalar baseline**

Run the smoke command from Task 8. Then build the pinned SciFact/all-mpnet-base-v2 descriptor and run the same candidate-budget sweep against it. Copy sanitized JSON into `docs/benchmarks/results/` and record the exact commands plus corpus descriptor in `docs/benchmarks/README.md`.

The result is evidence, not an automatic format freeze. Compare it with R7 recall gates and explicitly record pass, fail, or insufficient-evidence for each gate.

- [ ] **Step 6: Self-review milestone coverage**

Verify line by line that:

- Every M1 deliverable in the implementation specification has a tested artifact.
- No Raft, service, HNSW, router, or sharding implementation entered the diff.
- No code path renormalizes `p + e`.
- Primary scan cannot read residual bytes.
- All durable identities and versions are present in the golden fixture and measurement JSON.
- No benchmark claim exceeds what the measured corpus/hardware supports.

- [ ] **Step 7: Update living state and refresh Graphify**

Update `AGENTS.md` with the actual gate outcomes. If M1 passes, set the next step to writing the detailed M2 one-member-Raft vertical-slice plan; if any gate fails, name the exact failed gate and next investigation. Run the full Graphify update, check-update, and query workflow.

- [ ] **Step 8: Obtain independent review**

Assign a reviewer who did not implement the milestone. Resolve every Critical and Important finding, then rerun the affected verification commands.

- [ ] **Step 9: Commit the verified evidence**

Commit message:

```text
test: record scalar codec format foundation gate
```

## Plan self-review record

- Spec coverage: M0 and M1 requirements are mapped to Tasks 1-9.
- Scope: the plan stops before Raft, memtables, services, routing, SIMD, and HNSW.
- Type consistency: crate and durable-identity names match the implementation specification and `AGENTS.md`.
- Evidence separation: existing experiments are preserved but are not treated as production scorer or residual validation.
- Immediate next plan: M2 will start with a real one-member OpenRaft state machine and searchable FP32 publication; it will not introduce a weaker local commit path.

## Approval record

- **Codex:** `APPROVE` for the P4 technical plan.
- **Claude Opus:** explicitly returned `APPROVE` for the complete P4 technical plan.
- **Branding amendment:** planned package, command, temporary-output, and prototype identifiers were changed to Spherra at the user's direction. Task order, gates, and implementation behavior are unchanged; no renewed technical review was required.

No implementation may begin from this plan until both conditions are satisfied for the same unchanged revision.
