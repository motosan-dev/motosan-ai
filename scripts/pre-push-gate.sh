#!/usr/bin/env bash
set -euo pipefail

# Pre-push gate: runs unit tests + live Anthropic integration tests.
# Requires ANTHROPIC_API_KEY env var for live tests.
# If ANTHROPIC_API_KEY is not set, attempts to read from macOS Keychain.

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

echo "=== Pre-push gate ==="

# Skip gracefully if tools are not available (no nix/direnv environment)
if ! command -v cargo &>/dev/null; then
    echo "ℹ️  cargo not found — skipping Rust tests. Install via nix develop or rustup."
    SKIP_RUST=1
else
    SKIP_RUST=0
fi

if ! command -v uv &>/dev/null; then
    echo "ℹ️  uv not found — skipping Python tests. Install via nix develop or https://astral.sh/uv"
    SKIP_PYTHON=1
else
    SKIP_PYTHON=0
fi

if [ "$SKIP_RUST" = "1" ] && [ "$SKIP_PYTHON" = "1" ]; then
    echo "ℹ️  No tools available — skipping all checks."
    echo "=== Pre-push gate SKIPPED ==="
    exit 0
fi

# 1. Python unit tests (fast, no API needed)
if [ "$SKIP_PYTHON" = "0" ]; then
    echo "[1/2] Running Python unit tests..."
    uv run pytest sdks/python/tests/ -q --ignore=sdks/python/tests/integration/ || {
        echo "❌ Python unit tests failed. Push blocked."
        exit 1
    }
    echo "✅ Python unit tests passed."
fi

# 2. Rust unit tests (mock, no API needed)
if [ "$SKIP_RUST" = "0" ]; then
    echo "[2/2] Running Rust unit tests..."
    cargo test --manifest-path sdks/rust/Cargo.toml --all-features -q || {
        echo "❌ Rust unit tests failed. Push blocked."
        exit 1
    }
    echo "✅ Rust unit tests passed."
fi

# 3. Resolve API key for live tests
if [ -z "${ANTHROPIC_API_KEY:-}" ]; then
    ANTHROPIC_API_KEY=$(security find-generic-password -s "Claude Code-credentials" -w 2>/dev/null \
        | python3 -c "import json,sys; print(json.loads(sys.stdin.read())['claudeAiOauth']['accessToken'])" 2>/dev/null) || true
fi

if [ -z "${ANTHROPIC_API_KEY:-}" ]; then
    echo "⚠️  ANTHROPIC_API_KEY not set and not in Keychain. Skipping live tests."
    echo "=== Pre-push gate PASSED (live tests skipped) ==="
    exit 0
fi

export ANTHROPIC_API_KEY

# 4. Python live Anthropic integration tests
echo "[3/4] Running Python live Anthropic tests..."
uv run pytest sdks/python/tests/integration/test_anthropic_live.py -v || {
    echo "❌ Python live tests failed. Push blocked."
    exit 1
}
echo "✅ Python live tests passed."

# 5. Rust live Anthropic integration tests
echo "[4/4] Running Rust live Anthropic tests..."
cargo test --manifest-path sdks/rust/Cargo.toml --features full --test anthropic_live -- --test-threads=1 || {
    echo "❌ Rust live tests failed. Push blocked."
    exit 1
}
echo "✅ Rust live tests passed."

echo "=== Pre-push gate PASSED ==="
