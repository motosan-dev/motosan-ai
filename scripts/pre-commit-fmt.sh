#!/usr/bin/env bash
set -euo pipefail

# Pre-commit hook: auto-format staged files via treefmt.
# If treefmt changes files, the commit is aborted so you can review and re-stage.
# Skip with: git commit --no-verify

if ! command -v treefmt &>/dev/null; then
    echo "ℹ️  treefmt not found — skipping format check. Install via nix develop or https://treefmt.com"
    exit 0
fi

REPO_ROOT="$(git rev-parse --show-toplevel)"

# Run treefmt on the whole repo (it's fast — only touches changed files)
treefmt -C "$REPO_ROOT" --quiet

# Check if treefmt modified any staged files
CHANGED=$(git diff --name-only --cached)
if [ -n "$CHANGED" ]; then
    # Re-check: did any staged file actually change on disk?
    REFORMATTED=$(git diff --name-only)
    if [ -n "$REFORMATTED" ]; then
        echo "🔧 treefmt reformatted files:"
        echo "$REFORMATTED"
        echo ""
        echo "Review the changes, then: git add -u && git commit"
        exit 1
    fi
fi
