# AGENTS.md — motosan-ai Development Brief

Read this before writing any code.

---

## Project Overview

**motosan-ai** is a multi-language, multi-provider AI SDK. Each language is an independent idiomatic implementation — no FFI, no shared runtime.

Current status:
- ✅ Rust SDK v0.1.3 (`sdks/rust/`) — production ready, published to crates.io
- 🔄 Python SDK (`sdks/python/`) — in progress (M2)
- ⏳ TypeScript SDK (`sdks/typescript/`) — planned (M3)

---

## Repository Structure

```
motosan-ai/
├── sdks/
│   ├── rust/                   # Rust SDK (published: motosan-ai on crates.io)
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── types.rs        # Message, ChatRequest, ChatResponse, ToolCall, Role
│   │   │   ├── client.rs       # Client, ClientBuilder
│   │   │   ├── models.rs       # Model catalog + defaults
│   │   │   ├── retry.rs        # RetryPolicy
│   │   │   ├── stream.rs       # BoxStream, StreamEvent
│   │   │   ├── error.rs        # MotosanError
│   │   │   └── providers/
│   │   │       ├── anthropic.rs
│   │   │       ├── openai.rs
│   │   │       └── minimax.rs
│   │   └── tests/              # Integration tests (require real API keys, use #[ignore])
│   ├── python/                 # Python SDK (in progress)
│   └── typescript/             # TypeScript SDK (planned)
├── docs/plans/                 # Architecture decisions
└── specs/types.md              # Cross-language type spec
```

---

## Rust SDK

### Feature Flags
```toml
motosan-ai = { version = "0.1.3", features = ["anthropic"] }
# Options: "anthropic" | "openai" | "minimax" | "full"
```

### Core Types
```rust
pub struct Message {
    pub role: Role,                      // System | User | Assistant | Tool
    pub content: String,
    pub tool_call_id: Option<String>,    // for Role::Tool
    pub tool_calls: Vec<ToolCall>,       // for Role::Assistant with tool use
}

pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,   // serde_json::Value — NOT args, NOT params
}

pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<Value>,
}

pub struct ChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub model: String,
    pub usage: Usage,
    pub stop_reason: StopReason,
}
```

### Provider Serialization Rules (CRITICAL)
Each provider has different wire format. DO NOT mix them up:

**Anthropic:**
- Assistant + tool calls → `content: [{"type":"tool_use","id":...,"name":...,"input":...}]`
- Tool result → `role:"user", content:[{"type":"tool_result","tool_use_id":...,"content":...}]`
- System prompt → top-level `"system"` field, NOT in messages array

**OpenAI / MiniMax:**
- Assistant + tool calls → top-level `"tool_calls":[{"id":...,"type":"function","function":{"name":...,"arguments":"<JSON string>"}}]`
- Note: `arguments` is a **JSON string**, not an object
- Tool result → `role:"tool", tool_call_id:..., content:...`

### Coding Standards
- All `pub` items must have doc comments
- Use `thiserror` for `MotosanError` — no anyhow in lib
- Feature-gate provider code: `#[cfg(feature = "anthropic")]`
- Integration tests: `#[ignore]` unless `ANTHROPIC_API_KEY` etc. are set
- `cargo fmt + cargo clippy --all-features -- -D warnings` must pass

---

## Python SDK (in progress — M2)

### Structure
```
sdks/python/
├── pyproject.toml      # name=motosan-ai, optional deps per provider
├── motosan_ai/
│   ├── __init__.py     # re-export Client, Message, ChatRequest, ChatResponse
│   ├── types.py
│   ├── client.py
│   ├── error.py
│   └── providers/
│       ├── anthropic.py   # uses anthropic>=0.49 (optional dep)
│       ├── openai.py      # uses openai>=1.70 (optional dep)
│       └── minimax.py     # uses httpx (optional dep)
└── tests/
```

### Python Coding Standards
- Python 3.11+, type hints on all public functions
- `async`-first: `Client.chat()` and `Client.stream()` are async
- Sync wrapper: `Client.chat_sync()` via `asyncio.run()`
- Optional deps: import inside function body, raise `ImportError` with install hint if missing
- Tests: pytest + pytest-asyncio, mock provider HTTP calls with `respx` or `unittest.mock`

---

## TypeScript SDK (planned — M3)

### Package name
`@motosan-ai/sdk` (npm)

### Key types (aligned with Rust)
```typescript
type Role = 'user' | 'assistant' | 'system' | 'tool'

interface ToolCall {
  id: string
  name: string
  input: Record<string, unknown>  // NOT args, NOT params — must match Rust
}
```

---

## Cross-language Consistency Rules

1. Field name `input` (not `args`, not `params`) for tool call payloads — everywhere
2. `tool_call_id` (snake_case in Rust/Python), `toolCallId` (camelCase in TypeScript)
3. `ChatResponse.tool_calls` is always a `Vec`/`list`/`array` — never optional
4. `Message::tool_result(id, content)` constructor must exist in all languages

---

## Before Committing

```bash
# Rust
cd sdks/rust
cargo fmt
cargo clippy --all-features -- -D warnings
cargo test  # skips integration tests (they need API keys + #[ignore])

# Python
cd sdks/python
uv run ruff check .
uv run pytest tests/
```

---

## Milestones

| Milestone | Status |
|-----------|--------|
| M1 `v0.1.0 — Rust SDK` | ✅ closed |
| M6 `v0.1.2 — Multi-turn Tool Use Fix` | ✅ closed (v0.1.3 shipped) |
| M2 `v0.2.0 — Python SDK` | 🔄 in progress |
| M3 `v0.3.0 — TypeScript SDK` | ⏳ |
| M4 `v1.0.0 — Stable Release` | ⏳ |
