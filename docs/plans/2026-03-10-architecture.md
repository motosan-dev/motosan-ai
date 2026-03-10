# Architecture Plan — motosan-ai

*Date: 2026-03-10 | Status: Approved*

## Goal

A friendly, multi-language AI SDK that abstracts provider differences behind a single interface. Engineers should be able to switch from Anthropic to OpenAI to MiniMax by changing one configuration line — nothing else.

## Design Principles

1. **Idiomatic per language** — Rust gets Builder + Result + async streams. Python gets async/await + sync wrapper. Go gets Context + channels. No FFI shortcuts that hurt DX.
2. **Core + escape hatch** — Unify the 80% (chat, streaming, tools, system messages). Expose `provider_options` for the 20% that can't be abstracted (extended thinking, reasoning effort, etc.).
3. **Feature flags in Rust** — Single crate, opt-in providers. Users only compile what they use.
4. **Optional deps in Python** — `pip install motosan-ai[anthropic]` installs only what's needed.

## What Can Be Unified

| Feature | Abstraction approach |
|---------|---------------------|
| Basic chat | Normalize response fields |
| Streaming | Unified `StreamEvent { content, done }` |
| System message | SDK handles Anthropic's separate `system` param vs OpenAI's message role |
| Tool calling | Unified tool schema → convert to provider format |
| Stop reasons | Normalize to `StopReason` enum |
| Usage/tokens | Unified `Usage { input_tokens, output_tokens }` |

## What Cannot Be Unified

| Feature | Solution |
|---------|----------|
| Anthropic Extended Thinking | `provider_options.thinking` |
| OpenAI reasoning effort | `provider_options.reasoning_effort` |
| MiniMax-specific params | `provider_options` passthrough |
| Vision (image formats differ) | Phase 2 — provider-specific until stable |

## Rust Architecture

### Feature Flags

```toml
[features]
default = []
anthropic = ["dep:reqwest", "dep:serde_json", "dep:eventsource-stream"]
openai    = ["dep:reqwest", "dep:serde_json", "dep:eventsource-stream"]
minimax   = ["dep:reqwest", "dep:serde_json", "dep:eventsource-stream"]
full      = ["anthropic", "openai", "minimax"]
```

### Core Traits

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError>;
    async fn stream(&self, req: ChatRequest) -> Result<BoxStream<Result<StreamEvent, MotosanError>>, MotosanError>;
}
```

### Types

```rust
pub struct Message {
    pub role: Role,           // User | Assistant | System
    pub content: String,
}

pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub system: Option<String>,
    pub tools: Option<Vec<Tool>>,
    pub provider_options: Option<serde_json::Value>,
}

pub struct ChatResponse {
    pub content: String,
    pub model: String,
    pub usage: Usage,
    pub stop_reason: StopReason,
}

pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

pub enum StopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    Stop,
}

pub struct StreamEvent {
    pub content: String,
    pub done: bool,
}
```

### Error Handling

```rust
pub enum MotosanError {
    Auth(String),
    RateLimit { retry_after: Option<u64> },
    InvalidRequest(String),
    ProviderError { status: u16, message: String },
    Network(reqwest::Error),
    Stream(String),
}
```

### File Structure

```
sdks/rust/
├── Cargo.toml
├── src/
│   ├── lib.rs             pub use re-exports
│   ├── client.rs          Client struct + builder
│   ├── types.rs           Message, ChatRequest, ChatResponse, Usage, StopReason
│   ├── error.rs           MotosanError
│   ├── stream.rs          StreamEvent, stream helpers
│   └── providers/
│       ├── mod.rs         Provider trait
│       ├── anthropic.rs   #[cfg(feature = "anthropic")]
│       ├── openai.rs      #[cfg(feature = "openai")]
│       └── minimax.rs     #[cfg(feature = "minimax")]
├── examples/
│   ├── basic_chat.rs
│   ├── streaming.rs
│   └── tool_calling.rs
└── tests/
    ├── test_anthropic.rs
    ├── test_openai.rs
    └── test_minimax.rs
```

## Python Architecture

### Optional Dependencies

```toml
[project.optional-dependencies]
anthropic = ["anthropic>=0.40"]
openai    = ["openai>=1.50"]
minimax   = ["httpx>=0.27"]
all       = ["motosan-ai[anthropic,openai,minimax]"]
```

### Provider Protocol

```python
from typing import Protocol, AsyncIterator

class ProviderProtocol(Protocol):
    async def chat(self, req: ChatRequest) -> ChatResponse: ...
    async def stream(self, req: ChatRequest) -> AsyncIterator[StreamEvent]: ...
```

### File Structure

```
sdks/python/
├── pyproject.toml
├── motosan_ai/
│   ├── __init__.py        Client, Message, etc. re-exports
│   ├── client.py          Client class (async + sync wrapper)
│   ├── types.py           Message, ChatRequest, ChatResponse, Usage, StreamEvent
│   ├── error.py           MotosanError and subclasses
│   └── providers/
│       ├── __init__.py
│       ├── base.py        ProviderProtocol
│       ├── anthropic.py   AnthropicProvider
│       ├── openai.py      OpenAIProvider
│       └── minimax.py     MinimaxProvider
├── tests/
│   ├── test_client.py
│   ├── test_anthropic.py
│   ├── test_openai.py
│   └── test_minimax.py
└── examples/
    ├── basic_chat.py
    ├── streaming.py
    └── tool_calling.py
```

## Milestones

| Version | Scope | Target |
|---------|-------|--------|
| v0.1.0 | Rust SDK — all 3 providers, streaming, tools | 2026-03-24 |
| v0.2.0 | Python SDK — all 3 providers, streaming, tools | 2026-04-07 |
| v0.3.0 | TypeScript SDK | 2026-04-28 |
| v0.4.0 | Go SDK | 2026-05-12 |
| v1.0.0 | Docs site, all languages stable | TBD |
