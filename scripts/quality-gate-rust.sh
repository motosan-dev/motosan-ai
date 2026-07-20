#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT/sdks/rust"

echo "[1/8] Generate lockfile"
cargo generate-lockfile

echo "[2/8] Format"
cargo fmt --all -- --check

echo "[3/8] Clippy"
cargo clippy --locked --all-features --all-targets -- -D warnings

echo "[4/8] Tests (all features)"
cargo test --locked --all-features

echo "[5/8] Tests (no features)"
cargo test --locked

echo "[6/8] Doc tests"
cargo test --locked --doc --all-features

echo "[7/8] MSRV no-feature build/test"
cargo +1.82.0 update -p indexmap --precise 2.13.1
cargo +1.82.0 update -p uuid --precise 1.18.1
cargo +1.82.0 update -p reqwest --precise 0.12.4
cargo +1.82.0 update -p url --precise 2.4.1
cargo +1.82.0 update -p native-tls --precise 0.2.15
cargo +1.82.0 update -p tempfile --precise 3.24.0
cargo +1.82.0 update -p zeroize --precise 1.8.2
cargo +1.82.0 update -p wasip2 --precise 1.0.1+wasi-0.2.4
cargo +1.82.0 build --locked
cargo +1.82.0 test --locked

echo "[8/8] Package dry-run"
cargo publish --locked --dry-run --allow-dirty
