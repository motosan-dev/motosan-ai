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
- All providers use `httpx` directly — **no official provider SDKs** (`anthropic`, `openai`)
- Tests: pytest + pytest-asyncio, mock HTTP calls with `respx`
- Live integration tests in `tests/integration/` — require real API keys

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
cargo test --all-features  # unit tests (mock, no API needed)

# Python
cd sdks/python
uv run ruff check .
uv run pytest tests/ --ignore=tests/integration/
```

## Before Pushing (pre-push gate)

A pre-push hook (`scripts/pre-push-gate.sh`) runs automatically and blocks push on failure:

1. Python unit tests (mock)
2. Rust unit tests (mock)
3. Python live Anthropic integration tests (7 tests, ~50s)
4. Rust live Anthropic integration tests (7 tests, ~46s)

Live tests require `ANTHROPIC_API_KEY` — auto-reads from macOS Keychain if not set.
Skip with `git push --no-verify` in emergencies.

```bash
# Run live tests manually
ANTHROPIC_API_KEY=... uv run pytest sdks/python/tests/integration/test_anthropic_live.py -v
ANTHROPIC_API_KEY=... cargo test --features full --test anthropic_live -- --test-threads=1
```

## Before Releasing

Every release **must** include documentation and changelog updates. This is a hard gate — do not tag or publish without completing all items.

### Release Checklist

1. **Update CHANGELOGs** — both SDKs:
   - `sdks/rust/CHANGELOG.md` — add new version section with Added/Changed/Fixed
   - `sdks/python/CHANGELOG.md` — add new version section with Added/Changed/Fixed
   - Follow [Keep a Changelog](https://keepachangelog.com/) format
   - Include PR/issue numbers where applicable

2. **Bump versions**:
   - `sdks/rust/Cargo.toml` → `version = "X.Y.Z"`
   - `sdks/python/pyproject.toml` → `version = "X.Y.Z"`

3. **Update documentation** — any file affected by this release:
   - `sdks/rust/README.md` — new features, API changes, examples
   - `sdks/python/README.md` — new features, API changes, examples
   - `AGENTS.md` — coding standards, provider notes, milestones
   - `docs/plans/2026-03-10-architecture.md` — architecture changes, provider notes, milestones

4. **Run full test suite** (pre-push gate handles this, but verify manually if needed):
   ```bash
   # Unit tests
   cargo test --manifest-path sdks/rust/Cargo.toml --all-features
   uv run pytest sdks/python/tests/ --ignore=sdks/python/tests/integration/

   # Live integration tests
   ANTHROPIC_API_KEY=... cargo test --features full --test anthropic_live -- --test-threads=1
   ANTHROPIC_API_KEY=... uv run pytest sdks/python/tests/integration/test_anthropic_live.py -v
   ```

5. **Commit, tag, and push**:
   ```bash
   git add -p  # review each change
   git commit -m "chore: release rust-vX.Y.Z / python-vX.Y.Z"
   # Tag each SDK independently
   git tag -a rust-vX.Y.Z -m "rust-vX.Y.Z — summary"
   git tag -a python-vX.Y.Z -m "python-vX.Y.Z — summary"
   git push origin main --tags
   ```

### Tag Convention

| SDK | Tag format | Triggers |
|-----|-----------|----------|
| Rust | `rust-v0.3.3` | `publish-rust.yml` → crates.io |
| Python | `python-v0.3.3` | `publish-python.yml` → PyPI |

Rust and Python are versioned independently — tag each separately.
Can release only one SDK if the other has no changes.

### What Goes in the CHANGELOG

| Category | When to use |
|----------|-------------|
| **Added** | New features, new providers, new API methods |
| **Changed** | Breaking changes, dependency swaps, behavior changes |
| **Fixed** | Bug fixes, OAuth fixes, error handling improvements |
| **Removed** | Deprecated features, removed dependencies |

### What Docs to Update

| Change type | Files to update |
|-------------|----------------|
| New provider | Both READMEs + AGENTS.md provider table + architecture doc |
| New API method | Both READMEs (examples) + AGENTS.md (types) |
| Auth change | Both READMEs (Auth Matrix) + architecture doc (Provider Notes) |
| Dependency change | AGENTS.md (coding standards) + architecture doc (optional deps) |
| Test infrastructure | AGENTS.md (Before Pushing) + architecture doc (Testing Strategy) |

---

## Publishing

Publish is automated via GitHub Actions on tag push:
- `rust-v*` → `publish-rust.yml` → crates.io (fmt + clippy + test + publish)
- `python-v*` → `publish-python.yml` → PyPI (trusted publishing)

Manual publish (emergency):
```bash
# Rust
cd sdks/rust && cargo publish

# Python
cd sdks/python && uv build --out-dir dist && uv publish dist/*
```

---

## Milestones

| SDK | Version | Status |
|-----|---------|--------|
| Rust | rust-v0.3.3 | ✅ Current (crates.io) |
| Python | python-v0.4.0 | ✅ Current (PyPI) |
| TypeScript | ts-v0.1.0 | ⏳ Planned |
