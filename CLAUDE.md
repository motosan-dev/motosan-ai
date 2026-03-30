# CLAUDE.md — motosan-ai

## Dev Environment

Uses Nix flake + direnv. `cd` into the project to auto-activate.

```bash
# Manual entry (if direnv not hooked)
nix develop
```

## Quick Commands

```bash
fmt           # Format everything (Rust + Python + TOML + Nix)
lint          # Clippy + ruff + treefmt --fail-on-change
check-rust    # fmt → clippy → test (mirrors CI)
check-python  # ruff → format check → pytest (mirrors CI)
check-all     # Python + Rust full gate
test-live     # Anthropic integration tests (needs API key)
```

## Before Committing

Pre-commit hook runs `fmt` automatically. If it fails, review and re-stage.

For manual checks: `check-all`

## Formatting Standards

- **Rust**: `rustfmt` (edition 2021, max_width 100) + `clippy` (msrv 1.82)
- **Python**: `ruff` (py311, line-length 100, E/W/F/I/UP/B/SIM/RUF rules)
- **TOML**: `taplo`
- **Nix**: `nixpkgs-fmt`
- **Unified**: `treefmt` runs all formatters

## Project Layout

```
sdks/rust/     # Rust SDK — feature-flagged providers
sdks/python/   # Python SDK — optional deps per provider
devshell/      # Nix devShell config + scripts
specs/         # Canonical type definitions
```

## Key Rules

- Provider-specific logic stays in `providers/` only
- Field name `input` (not `args`) for tool call payloads
- `ChatResponse.tool_calls` is always a list — never optional
- Do not add sync wrappers to Python
- Do not share code between SDKs via FFI
