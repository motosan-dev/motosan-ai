# Architecture Plan — motosan-ai

*Created: 2026-03-10 | Last updated: 2026-03-18 | Status: Rust v0.3.3 / Python v0.3.3 | Both SDKs use httpx/reqwest directly (no official provider SDKs)*

## Goal

A friendly, multi-language AI SDK that abstracts provider differences behind a single interface.
Engineers should be able to switch from Anthropic to OpenAI to MiniMax by changing one config line — nothing else.

## Design Principles

1. **Idiomatic per language** — Rust gets Builder + Result + async streams. Python gets async/await + sync wrapper. No FFI shortcuts that hurt DX.
2. **Core + escape hatch** — Unify the 80% (chat, streaming, tools, system messages). Expose `provider_options` for the 20% that can't be abstracted.
3. **Feature flags in Rust** — Single crate, opt-in providers. Only compile what you use.
4. **Optional deps in Python** — `pip install motosan-ai[anthropic]` installs only what's needed.

## What Can Be Unified

| Feature | Approach |
|---------|----------|
| Basic chat | Normalize response fields |
| Streaming | Unified `StreamEvent { content, done }` |
| System message | SDK handles Anthropic's separate `system` param vs OpenAI's message role |
| Tool calling | Unified `Tool` schema → convert to provider format; `ToolCall` in response |
| Stop reasons | Normalize to `StopReason` enum |
| Token usage | Unified `Usage { input_tokens, output_tokens }` |
| Multi-turn tool use | `Role::Tool` + `Message.tool_call_id` for tool result messages |
| Retry | Configurable `RetryPolicy` with exponential backoff + `Retry-After` |

## What Cannot Be Unified → use `provider_options`

| Feature | Provider |
|---------|----------|
| Extended Thinking | Anthropic |
| Reasoning effort | OpenAI (`reasoning_content` fallback supported) |
| MiniMax reasoning exposure | MiniMax (`minimax_expose_reasoning` flag) |
| Custom auth header | OpenAI-compatible endpoints (`openai_auth_custom_header`) |
| `/v1/responses` fallback | OpenAI (`openai_responses_fallback`) |

---

## Rust Architecture (`sdks/rust/`)

### Status: v0.1.1 released ✅

### Feature Flags

```toml
[features]
default   = []
anthropic = ["dep:reqwest", "dep:eventsource-stream", "dep:tokio-stream", "dep:tokio"]
openai    = ["dep:reqwest", "dep:eventsource-stream", "dep:tokio-stream", "dep:tokio"]
minimax   = ["dep:reqwest", "dep:eventsource-stream", "dep:tokio-stream", "dep:tokio"]
full      = ["anthropic", "openai", "minimax"]
```

### Core Types (actual, as of v0.1.1)

```rust
// Role — includes Tool for multi-turn tool use
enum Role { User, Assistant, System, Tool }

// Message — tool_call_id for Role::Tool result messages
Message { role: Role, content: String, tool_call_id: Option<String> }

// Tool definition (sent in request)
Tool { name: String, description: Option<String>, input_schema: Option<Value> }

// Tool call (returned in response)
ToolCall { id: String, name: String, input: Value }

ChatRequest {
    messages, model, system, temperature, max_tokens,
    tools: Option<Vec<Tool>>,
    provider_options: Option<Value>
}

ChatResponse {
    content: String,
    tool_calls: Vec<ToolCall>,   // empty when no tool calls
    model: String,
    usage: Usage,
    stop_reason: StopReason,
}

Usage        { input_tokens: u32, output_tokens: u32 }
StreamEvent  { content: String, done: bool }

StopReason   { EndTurn, MaxTokens, ToolUse, Stop, Other }
```

### RetryPolicy

```rust
RetryPolicy {
    max_retries: u32,       // default 3
    base_delay_ms: u64,     // default 1000
    max_delay_ms: u64,      // default 60000
    jitter: bool,           // default true
    respect_retry_after: bool, // default true
}
```

### Provider Trait

```rust
#[async_trait]
trait ProviderImpl: Send + Sync {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError>;
    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError>;
}
```

### File Structure

```
sdks/rust/
├── Cargo.toml
├── CHANGELOG.md
└── src/
    ├── lib.rs
    ├── client.rs        Client + ClientBuilder
    ├── types.rs         Message, ChatRequest, ChatResponse, ToolCall, Usage, StopReason
    ├── models.rs        Model name constants + catalogs per provider
    ├── error.rs         MotosanError (thiserror)
    ├── stream.rs        StreamEvent, BoxStream
    ├── retry.rs         RetryPolicy
    └── providers/
        ├── mod.rs         ProviderImpl trait + shared helpers
        ├── anthropic.rs   #[cfg(feature = "anthropic")]
        ├── openai.rs      #[cfg(feature = "openai")]
        └── minimax.rs     #[cfg(feature = "minimax")]
```

### Provider Notes

| Provider | Notes |
|----------|-------|
| Anthropic | OAuth token support (`sk-ant-oat01-*`): Bearer auth, system as array of blocks, `chat()` auto-redirects to `stream()` with tool_call collection; standard key uses `x-api-key` |
| OpenAI | Configurable auth style (Bearer / x-api-key / custom); optional `/v1/responses` fallback |
| MiniMax | OpenAI-compatible `/chat/completions`; system prompt merged into first user message; `<think>` stripping |
| Ollama | OpenAI-compat mode (via OpenAI provider) + native NDJSON mode; no API key required |

---

## Python Architecture (`sdks/python/`)

### HTTP Client

All providers use `httpx` directly — **no official provider SDKs** (`anthropic`, `openai`) required.
This keeps the dependency tree minimal and gives full control over auth, headers, and SSE parsing.

### Optional Dependencies

```toml
[project.optional-dependencies]
anthropic = ["httpx>=0.27"]
openai    = ["httpx>=0.27"]
minimax   = ["httpx>=0.27"]
ollama    = ["httpx>=0.27"]
all       = ["motosan-ai[anthropic,openai,minimax,ollama]"]
```

### Core Types (mirrors Rust)

```python
@dataclass
class Message:
    role: Role           # user / assistant / system / tool
    content: str
    tool_call_id: str | None = None

@dataclass
class Tool:
    name: str
    description: str | None
    input_schema: dict | None

@dataclass
class ToolCall:
    id: str
    name: str
    input: dict

@dataclass
class ChatResponse:
    content: str
    tool_calls: list[ToolCall]
    model: str
    usage: Usage
    stop_reason: StopReason
```

### File Structure

```
sdks/python/
├── pyproject.toml
├── motosan_ai/
│   ├── __init__.py
│   ├── client.py            Client (async + sync wrapper)
│   ├── types.py             Message, ChatRequest, ChatResponse, ToolCall, Usage, StreamEvent
│   ├── error.py             MotosanError and subclasses
│   ├── think_stripper.py    <think> tag removal for MiniMax
│   └── providers/
│       ├── __init__.py
│       ├── anthropic.py     httpx direct (OAuth + standard key)
│       ├── openai.py        httpx direct
│       ├── minimax.py       httpx direct
│       └── ollama.py        httpx direct (native NDJSON API)
└── tests/
    ├── test_anthropic.py    mock tests (respx)
    ├── test_openai.py       mock tests (respx)
    ├── test_minimax.py      mock tests (respx)
    ├── test_ollama.py       mock tests (respx)
    ├── test_ollama_native.py
    └── integration/
        └── test_anthropic_live.py   live API tests (7 tests)
```

---

## Testing Strategy

### Unit Tests (mock)
- Rust: `cargo test --all-features` — mockito-based, no API keys
- Python: `uv run pytest tests/ --ignore=tests/integration/` — respx-based, no API keys

### Live Integration Tests
Both SDKs have 7 identical Anthropic live tests (supports OAuth token):

1. chat (basic)
2. stream (basic)
3. system prompt
4. temperature
5. tool use (single turn)
6. tool use (multi-turn)
7. stream + tool use

```bash
# Python
ANTHROPIC_API_KEY=... uv run pytest tests/integration/test_anthropic_live.py -v

# Rust
ANTHROPIC_API_KEY=... cargo test --features full --test anthropic_live -- --test-threads=1
```

### Pre-push Gate
`scripts/pre-push-gate.sh` runs all unit + live tests before push.
Auto-reads OAuth token from macOS Keychain.

---

## Publishing

Rust and Python are versioned and released independently.

| SDK | Tag format | Workflow | Registry |
|-----|-----------|----------|----------|
| Rust | `rust-v*` | `publish-rust.yml` | crates.io |
| Python | `python-v*` | `publish-python.yml` | PyPI |

Can release only one SDK if the other has no changes.

---

## Milestones

| SDK | Version | Scope | Status |
|-----|---------|-------|--------|
| Rust | rust-v0.1.0 | 3 providers, streaming, retry, tests | ✅ Shipped 2026-03-10 |
| Rust | rust-v0.1.1 | Tool calling, MiniMax improvements, OpenAI auth style | ✅ Shipped 2026-03-11 |
| Python | python-v0.2.0 | 4 providers, async + sync, tool calling, tests | ✅ Shipped |
| Both | rust-v0.3.3 / python-v0.3.3 | Remove official SDK deps, httpx-only, OAuth fixes, live tests | ✅ Current |
| TypeScript | ts-v0.1.0 | TypeScript SDK | ⏳ Planned (due 2026-04-28) |
| All | v1.0.0 | All stable, docs site | ⏳ Planned (due 2026-06-01) |
