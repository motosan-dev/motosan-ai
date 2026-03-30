#!/usr/bin/env bash
set -euo pipefail

# Install git hooks for motosan-ai.
# Run once after cloning: ./scripts/setup-hooks.sh

REPO_ROOT="$(git rev-parse --show-toplevel)"
HOOKS_DIR="$REPO_ROOT/.git/hooks"

echo "Installing git hooks..."

# pre-commit: auto-format via treefmt
cat > "$HOOKS_DIR/pre-commit" << 'HOOK'
#!/usr/bin/env bash
exec "$(git rev-parse --show-toplevel)/scripts/pre-commit-fmt.sh"
HOOK
chmod +x "$HOOKS_DIR/pre-commit"
echo "  ✓ pre-commit (treefmt auto-format)"

# pre-push: full test gate
cat > "$HOOKS_DIR/pre-push" << 'HOOK'
#!/usr/bin/env bash
exec "$(git rev-parse --show-toplevel)/scripts/pre-push-gate.sh"
HOOK
chmod +x "$HOOKS_DIR/pre-push"
echo "  ✓ pre-push (test gate)"

echo "Done. Skip with --no-verify if needed."
