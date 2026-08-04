#!/usr/bin/env bash
set -euo pipefail

export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked
cargo nextest run --workspace --all-features --locked --no-tests=pass
cargo test --workspace --doc --locked
python3 scripts/check_dependency_policy.py
cargo deny check licenses bans sources
