# AGENTS.md — motosan-ai Development Brief

Read this before writing any code.

---

## Project Overview

**motosan-ai** is a multi-language, multi-provider AI SDK. Each language is an independent idiomatic implementation — no FFI, no shared runtime.

Current versions:
- Python SDK v0.4.2 (`sdks/python/`) — published to PyPI
- Rust SDK v0.5.2 (`sdks/rust/`) — published to crates.io

---

## Repository Structure

```
motosan-ai/
├── sdks/
│   ├── python/                 # Python SDK (PyPI: motosan-ai)
│   │   ├── pyproject.toml
│   │   ├── ruff.toml           # Lint + format config
│   │   ├── motosan_ai/
│   │   │   ├── client.py       # Client, Provider
│   │   │   ├── types.py        # Message, ChatRequest, ChatResponse, StreamEvent, Tool
│   │   │   ├── retry.py        # RetryPolicy
│   │   │   ├── think_stripper.py
│   │   │   └── providers/      # anthropic.py, openai.py, minimax.py, ollama.py
│   │   └── tests/
│   └── rust/                   # Rust SDK (crates.io: motosan-ai)
│       ├── Cargo.toml
│       ├── rustfmt.toml        # Format config
│       ├── .clippy.toml        # Lint config (msrv)
│       ├── src/
│       │   ├── lib.rs
│       │   ├── client.rs       # Client, ClientBuilder
│       │   ├── types.rs        # Message, ChatRequest, ChatResponse, StreamEvent, Tool
│       │   ├── stream.rs       # BoxStream, collect_stream
│       │   ├── retry.rs        # RetryPolicy
│       │   └── think_stripper.rs
│       └── tests/
├── devshell/                   # Nix devShell + scripts
│   ├── default.nix             # Shell definition
│   └── scripts.nix             # fmt, lint, check-*, test-live
├── flake.nix                   # Nix flake (fenix Rust + Python + tools)
├── treefmt.toml                # Unified formatter config
├── taplo.toml                  # TOML format config
├── .editorconfig               # Editor defaults
├── specs/types.md              # Canonical type definitions (source of truth)
└── docs/
```

---

## Dev Environment

Uses **Nix flake + direnv**. `cd` into the project to auto-activate.

```bash
nix develop          # Manual entry if direnv not hooked
```

Toolchain: fenix stable Rust (rustc/cargo/clippy/rustfmt/rust-src) + Python 3.12 + uv + ruff + cargo-nextest + treefmt + taplo + nixpkgs-fmt

---

## Quick Commands

All commands are available inside `nix develop`:

| Command | What it does | Mirrors |
|---------|-------------|---------|
| `fmt` | Format everything (Rust + Python + TOML + Nix) | — |
| `lint` | Clippy + ruff + treefmt --fail-on-change | — |
| `check-rust` | fmt check → clippy → test --all-features | `ci-rust.yml` |
| `check-python` | ruff check → ruff format --check → pytest | `ci-python.yml` |
| `check-all` | Python + Rust full gate | pre-push gate |
| `test-live` | Anthropic integration tests (auto-resolves API key) | pre-push gate (live) |

---

## Formatting & Lint Standards

| Language | Formatter | Linter | Config |
|----------|-----------|--------|--------|
| Rust | `rustfmt` (edition 2021, max_width 100) | `clippy` (msrv 1.82) | `sdks/rust/rustfmt.toml`, `.clippy.toml` |
| Python | `ruff format` (py311, line-length 100) | `ruff check` (E/W/F/I/UP/B/SIM/RUF) | `sdks/python/ruff.toml` |
| TOML | `taplo` | — | `taplo.toml` |
| Nix | `nixpkgs-fmt` | — | — |

`treefmt.toml` runs all formatters via a single `fmt` command. Pre-commit hook runs `fmt` automatically.

---

## Key Design Decisions

- **Provider parity** — all providers must implement `chat()`, `stream()`, `chat_with()`, `stream_with()`
- **ThinkStripper** — stateful, applied at `Client.stream()` level; cross-chunk safe
- **Anthropic tool_call_id** — must track `current_tool_id` in state; `content_block_start` carries id, deltas don't
- **No premature abstraction** — keep per-language idiomatic; no shared core

---

## Coding Standards

- Python: type hints required, `dataclass`, `async/await`, `AsyncIterator`
- Rust: `async-trait`, `thiserror`, feature flags per provider
- Tests: unit tests for all public API; integration tests gated behind feature flags or env vars

---

## Cross-language Consistency Rules

1. Field name `input` (not `args`, not `params`) for tool call payloads — everywhere
2. `tool_call_id` (snake_case in Rust/Python), `toolCallId` (camelCase in TypeScript)
3. `ChatResponse.tool_calls` is always a `Vec`/`list`/`array` — never optional
4. `Message::tool_result(id, content)` constructor must exist in all languages

---

## Provider Serialization (CRITICAL — do not mix up)

**Anthropic:**
- Assistant + tool calls → `content: [{"type":"tool_use","id":...,"name":...,"input":...}]`
- Tool result → `role:"user", content:[{"type":"tool_result","tool_use_id":...,"content":...}]`
- System prompt → top-level `"system"` field, NOT in messages array

**OpenAI / MiniMax:**
- Assistant + tool calls → top-level `"tool_calls":[{"id":...,"type":"function","function":{"name":...,"arguments":"<JSON string>"}}]`
- Note: `arguments` is a **JSON string**, not an object
- Tool result → `role:"tool", tool_call_id:..., content:...`

---

## What NOT to Do

- Do not add sync wrappers to Python (use `asyncio.run()` at the call site)
- Do not share code between Python and Rust via FFI or subprocess
- Do not add provider-specific logic outside `providers/` (Python) or per-provider modules (Rust)
- Do not break the `LlmClient` Protocol in motosan-chat compatibility
- Do not commit without running `fmt` (pre-commit hook enforces this)

---

## Git Hooks

| Hook | Script | Skip |
|------|--------|------|
| pre-commit | `fmt` (treefmt) | `git commit --no-verify` |
| pre-push | `scripts/pre-push-gate.sh` (tests + optional live tests) | `git push --no-verify` |

Live tests require `ANTHROPIC_API_KEY`. Auto-resolved from macOS Keychain if available.

---

## Releasing

| SDK | Tag format | Triggers |
|-----|-----------|----------|
| Rust | `rust-v0.5.2` | `publish-rust.yml` → crates.io |
| Python | `python-v0.4.2` | `publish-python.yml` → PyPI |

Checklist:
1. Update CHANGELOGs (`sdks/rust/CHANGELOG.md`, `sdks/python/CHANGELOG.md`)
2. Bump version in `Cargo.toml` / `pyproject.toml`
3. Update version numbers in: `README.md` (root), `AGENTS.md`, `llms.txt`, `skills/motosan-ai/SKILL.md`
4. Commit, tag (`rust-vX.Y.Z` / `python-vX.Y.Z`), push with tags
