#!/usr/bin/env bash
set -euo pipefail

# Pre-push gate: path-scoped unit tests for the SDKs touched by the pushed range.
# Live tests are OPT-IN: RUN_LIVE=1 git push
# By project decision live provider tests never run in CI, so this — together
# with the `test-live` dev-shell script and the #[ignore]d live suites — is the
# only way they run at all.
#
# git invokes pre-push with one line per ref on stdin:
#   <local_ref> <local_sha> <remote_ref> <remote_sha>

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

ZERO=0000000000000000000000000000000000000000

block() {
    echo "PUSH BLOCKED: $1" >&2
    exit 1
}

# ---- Determine which SDKs the pushed ranges touch -------------------------
CHANGED=""
if [ -t 0 ]; then
    # Manual invocation without stdin: behave conservatively, test everything.
    CHANGED="ALL"
else
    while read -r _local_ref local_sha _remote_ref remote_sha; do
        [ "$local_sha" = "$ZERO" ] && continue # branch deletion: nothing to test
        if [ "$remote_sha" = "$ZERO" ]; then
            # New remote branch: diff against merge-base with origin/main.
            base=$(git merge-base "$local_sha" origin/main 2>/dev/null) || base=""
        else
            base="$remote_sha"
        fi
        if [ -n "$base" ]; then
            CHANGED+="$(git diff --name-only "$base" "$local_sha" 2>/dev/null || echo ALL)"$'\n'
        else
            CHANGED+="ALL"$'\n'
        fi
    done
fi

if [ -z "$CHANGED" ]; then
    echo "=== Pre-push gate PASSED (deletion-only push — nothing to test) ==="
    exit 0
fi

NEED_PYTHON=0
NEED_RUST=0
NEED_TS=0
if grep -qx "ALL" <<<"$CHANGED"; then
    NEED_PYTHON=1 NEED_RUST=1 NEED_TS=1
else
    if grep -qE '^sdks/python/' <<<"$CHANGED"; then NEED_PYTHON=1; fi
    if grep -qE '^sdks/rust/' <<<"$CHANGED"; then NEED_RUST=1; fi
    if grep -qE '^sdks/typescript/' <<<"$CHANGED"; then NEED_TS=1; fi
    # Root workspace manifests affect SDK builds without living under sdks/:
    # Cargo.toml is the Rust workspace root; pyproject.toml + uv.lock are the
    # uv workspace (members = ["sdks/python"]) and its tracked lockfile.
    if grep -qxE 'Cargo\.toml' <<<"$CHANGED"; then NEED_RUST=1; fi
    if grep -qxE 'pyproject\.toml|uv\.lock' <<<"$CHANGED"; then NEED_PYTHON=1; fi
fi

echo "=== Pre-push gate ==="

if [ "$NEED_PYTHON$NEED_RUST$NEED_TS" = "000" ] && [ "${RUN_LIVE:-0}" != "1" ]; then
    echo "=== Pre-push gate PASSED (no SDK paths in pushed range — suites skipped) ==="
    exit 0
fi

# ---- Unit suites ----------------------------------------------------------
# Hermetic: strip provider credentials so the env-gated live tests that live
# INSIDE the unit suites (tests/*_live.rs, TS integration.*.test.ts — none are
# #[ignore]d; they fire whenever their key env var is set) cannot run here.
# RUN_LIVE=1 below is the ONLY live path; it uses the ambient env untouched.
UNSET_CREDS=(-u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u GEMINI_API_KEY
    -u GOOGLE_API_KEY -u GEMINI_OAUTH_TOKEN -u GEMINI_PROJECT_ID
    -u MINIMAX_API_KEY -u OLLAMA_API_KEY -u OLLAMA_BASE_URL -u OLLAMA_HOST)

if [ "$NEED_PYTHON" = "1" ]; then
    if command -v uv &>/dev/null; then
        echo "[python] unit tests..."
        env "${UNSET_CREDS[@]}" uv run pytest sdks/python/tests/ -q --ignore=sdks/python/tests/integration/ \
            || block "Python unit tests failed"
        echo "✅ Python unit tests passed."
    else
        echo "ℹ️  uv not found — skipping Python tests."
    fi
fi

if [ "$NEED_RUST" = "1" ]; then
    if command -v cargo &>/dev/null; then
        echo "[rust] unit tests (--all-features)..."
        env "${UNSET_CREDS[@]}" cargo test --manifest-path sdks/rust/Cargo.toml --all-features -q \
            || block "Rust unit tests failed"
        echo "✅ Rust unit tests passed."
    else
        echo "ℹ️  cargo not found — skipping Rust tests."
    fi
fi

if [ "$NEED_TS" = "1" ]; then
    if command -v npm &>/dev/null && [ -d sdks/typescript/node_modules ]; then
        echo "[typescript] build + vitest (pack-smoke needs dist/)..."
        (cd sdks/typescript && env "${UNSET_CREDS[@]}" npm run build && env "${UNSET_CREDS[@]}" npm run test) \
            || block "TypeScript tests failed"
        echo "✅ TypeScript tests passed."
    else
        echo "ℹ️  npm or sdks/typescript/node_modules missing — skipping TS tests."
    fi
fi

# ---- Live tests (opt-in only) ---------------------------------------------
if [ "${RUN_LIVE:-0}" = "1" ]; then
    if [ -z "${ANTHROPIC_API_KEY:-}" ]; then
        ANTHROPIC_API_KEY=$(security find-generic-password -s "Claude Code-credentials" -w 2>/dev/null \
            | python3 -c "import json,sys; print(json.loads(sys.stdin.read())['claudeAiOauth']['accessToken'])" 2>/dev/null) || true
    fi
    [ -z "${ANTHROPIC_API_KEY:-}" ] && block "RUN_LIVE=1 but no ANTHROPIC_API_KEY available (env or Keychain)"
    export ANTHROPIC_API_KEY
    echo "[live] Python live Anthropic tests..."
    uv run pytest sdks/python/tests/integration/test_anthropic_live.py -v \
        || block "Python live tests failed"
    echo "[live] Rust live Anthropic tests..."
    cargo test --manifest-path sdks/rust/Cargo.toml --features full --test anthropic_live -- --test-threads=1 \
        || block "Rust live tests failed"
    echo "✅ Live tests passed."
fi

echo "=== Pre-push gate PASSED ==="
