# AGENTS.md — motosan-ai Development Brief

Read this before writing any code.

---

## Project Overview

**motosan-ai** is a multi-language, multi-provider AI SDK. Each language is an independent idiomatic implementation — no FFI, no shared runtime.

Current versions:
- ✅ Python SDK v0.4.2 (`sdks/python/`) — published to PyPI
- ✅ Rust SDK v0.3.3 (`sdks/rust/`) — published to crates.io
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

## What NOT to Do

- Do not add sync wrappers to Python (use `asyncio.run()` at the call site)
- Do not share code between Python and Rust via FFI or subprocess
- Do not add provider-specific logic outside `providers/` (Python) or per-provider modules (Rust)
- Do not break the `LlmClient` Protocol in motosan-chat compatibility
