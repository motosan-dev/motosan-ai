# AGENTS.md — motosan-ai Development Brief

Read this before writing any code.

---

## Project Overview

**motosan-ai** is a multi-language, multi-provider AI SDK. Each language is an independent idiomatic implementation — no FFI, no shared runtime.

Current versions:
- ✅ Python SDK v0.4.2 (`sdks/python/`) — published to PyPI
- ✅ Rust SDK v0.5.2 (`sdks/rust/`) — published to crates.io
- ⏳ TypeScript SDK (`sdks/typescript/`) — planned (M3)

---

## Repository Structure

```
motosan-ai/
├── sdks/
│   ├── python/                 # Python SDK (PyPI: motosan-ai)
│   │   ├── pyproject.toml
│   │   ├── motosan_ai/
│   │   │   ├── client.py       # Client, Provider
│   │   │   ├── types.py        # Message, ChatRequest, ChatResponse, StreamEvent, Tool
│   │   │   ├── retry.py        # RetryPolicy
│   │   │   ├── think_stripper.py  # ThinkStripper
│   │   │   └── providers/      # anthropic.py, openai.py, minimax.py, ollama.py
│   │   └── tests/
│   └── rust/                   # Rust SDK (crates.io: motosan-ai)
│       ├── Cargo.toml
│       ├── src/
│       │   ├── lib.rs
│       │   ├── client.rs       # Client, ClientBuilder
│       │   ├── types.rs        # Message, ChatRequest, ChatResponse, StreamEvent, Tool
│       │   ├── stream.rs       # BoxStream, StreamEvent
│       │   ├── retry.rs        # RetryPolicy
│       │   └── think_stripper.rs
│       └── tests/
├── specs/types.md              # Canonical type definitions (source of truth)
└── docs/
```

---

## Key Design Decisions

- **Provider parity** — all providers must implement `chat()`, `stream()`, `chat_with()`, `stream_with()`
- **ThinkStripper** — stateful, applied at `Client.stream()` level; cross-chunk safe
- **Anthropic tool_call_id** — must track `current_tool_id` in state; `content_block_start` carries id, deltas don't
- **No premature abstraction** — keep per-language idiomatic; no shared core

---

## Coding Standards

- Python: type hints required, `dataclass`, `async/await`, `AsyncGenerator`
- Rust: `async-trait`, `thiserror`, feature flags per provider
- Tests: unit tests for all public API; integration tests gated behind feature flags or env vars

---

## Common Commands

```bash
# Python
cd sdks/python
uv run pytest tests/ -q

# Rust
cd sdks/rust
cargo test
cargo test --features full
cargo clippy --features full -- -D warnings
```

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

---

## Before Committing

```bash
# Rust
cd sdks/rust
cargo fmt
cargo clippy --all-features -- -D warnings
cargo test --all-features

# Python
cd sdks/python
uv run ruff check .
uv run pytest tests/ --ignore=tests/integration/
```

## Pre-push Gate

A pre-push hook (`scripts/pre-push-gate.sh`) runs automatically:
1. Python unit tests (mock)
2. Rust unit tests (mock)
3. Python live Anthropic integration tests
4. Rust live Anthropic integration tests

Live tests require `ANTHROPIC_API_KEY`. Skip with `git push --no-verify` in emergencies.

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
