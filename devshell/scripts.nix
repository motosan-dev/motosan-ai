{ writeShellScriptBin, ... }:

{
  # Format everything: Rust + Python + TOML + Nix
  fmt = writeShellScriptBin "fmt" ''
    set -euo pipefail
    REPO_ROOT="$(git rev-parse --show-toplevel)"
    echo "=== treefmt ==="
    treefmt -C "$REPO_ROOT" "$@"
  '';

  # Lint without fixing (for CI parity)
  lint = writeShellScriptBin "lint" ''
    set -euo pipefail
    REPO_ROOT="$(git rev-parse --show-toplevel)"
    echo "=== Lint ==="

    echo "[1/3] cargo clippy"
    cargo clippy --manifest-path "$REPO_ROOT/sdks/rust/Cargo.toml" \
      --all-features --all-targets -- -D warnings

    echo "[2/3] ruff check"
    ruff check "$REPO_ROOT/sdks/python/motosan_ai/"

    echo "[3/3] treefmt --fail-on-change"
    treefmt -C "$REPO_ROOT" --fail-on-change

    echo "✅ Lint passed."
  '';

  # Mirrors ci-rust.yml: fmt → clippy → test (all features)
  check-rust = writeShellScriptBin "check-rust" ''
    set -euo pipefail
    RUST_DIR="''${REPO_ROOT:-$(git rev-parse --show-toplevel)}/sdks/rust"
    echo "=== Rust checks ==="

    echo "[1/3] cargo fmt --check"
    cargo fmt --manifest-path "$RUST_DIR/Cargo.toml" --all -- --check

    echo "[2/3] cargo clippy --all-features"
    cargo clippy --manifest-path "$RUST_DIR/Cargo.toml" \
      --all-features --all-targets -- -D warnings

    echo "[3/3] cargo test --all-features"
    cargo test --manifest-path "$RUST_DIR/Cargo.toml" --all-features

    echo "✅ Rust checks passed."
  '';

  # Mirrors ci-python.yml: ruff → pytest
  check-python = writeShellScriptBin "check-python" ''
    set -euo pipefail
    PYTHON_DIR="''${REPO_ROOT:-$(git rev-parse --show-toplevel)}/sdks/python"
    echo "=== Python checks ==="

    echo "[1/3] ruff check"
    ruff check "$PYTHON_DIR/motosan_ai/"

    echo "[2/3] ruff format --check"
    ruff format --check "$PYTHON_DIR/motosan_ai/" "$PYTHON_DIR/tests/"

    echo "[3/3] pytest (unit)"
    uv run pytest "$PYTHON_DIR/tests/" -q --ignore="$PYTHON_DIR/tests/integration/"

    echo "✅ Python checks passed."
  '';

  # Full pre-push gate: Python + Rust (no live tests)
  check-all = writeShellScriptBin "check-all" ''
    set -euo pipefail
    echo "=== motosan-ai full check ==="
    check-python
    check-rust
    echo "=== All checks passed ==="
  '';

  # Live integration tests (requires ANTHROPIC_API_KEY)
  test-live = writeShellScriptBin "test-live" ''
    set -euo pipefail
    REPO_ROOT="$(git rev-parse --show-toplevel)"

    if [ -z "''${ANTHROPIC_API_KEY:-}" ]; then
      ANTHROPIC_API_KEY=$(security find-generic-password -s "Claude Code-credentials" -w 2>/dev/null \
        | python3 -c "import json,sys; print(json.loads(sys.stdin.read())['claudeAiOauth']['accessToken'])" 2>/dev/null) || true
    fi

    if [ -z "''${ANTHROPIC_API_KEY:-}" ]; then
      echo "❌ ANTHROPIC_API_KEY not set. Cannot run live tests."
      exit 1
    fi
    export ANTHROPIC_API_KEY

    echo "=== Live integration tests ==="

    echo "[1/2] Python live tests"
    uv run pytest "$REPO_ROOT/sdks/python/tests/integration/test_anthropic_live.py" -v

    echo "[2/2] Rust live tests"
    cargo test --manifest-path "$REPO_ROOT/sdks/rust/Cargo.toml" \
      --features full --test anthropic_live -- --test-threads=1

    echo "✅ Live tests passed."
  '';
}
