# Architecture Plan — motosan-ai

*Date: 2026-03-10 | Status: Approved*

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
| Tool calling | Unified tool schema → convert to provider format |
| Stop reasons | Normalize to `StopReason` enum |
| Token usage | Unified `Usage { input_tokens, output_tokens }` |

## What Cannot Be Unified → use `provider_options`

| Feature | Provider |
|---------|----------|
| Extended Thinking | Anthropic |
| Reasoning effort | OpenAI |
| Provider-specific params | MiniMax |

## Rust Architecture

### Feature Flags

```toml
[features]
default  = []
anthropic = [...]
openai    = [...]
minimax   = [...]
full      = ["anthropic", "openai", "minimax"]
```

### Core Types

```rust
Message      { role: Role, content: String }
ChatRequest  { messages, model, system, temperature, max_tokens, tools, provider_options }
ChatResponse { content, model, usage: Usage, stop_reason: StopReason }
Usage        { input_tokens, output_tokens }
StreamEvent  { content, done }
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
└── src/
    ├── lib.rs
    ├── client.rs       Client + ClientBuilder
    ├── types.rs        Message, ChatRequest, ChatResponse, Usage, StopReason
    ├── error.rs        MotosanError
    ├── stream.rs       StreamEvent, BoxStream
    └── providers/
        ├── mod.rs      ProviderImpl trait
        ├── anthropic.rs  #[cfg(feature = "anthropic")]
        ├── openai.rs     #[cfg(feature = "openai")]
        └── minimax.rs    #[cfg(feature = "minimax")]
```

## Python Architecture

### Optional Dependencies

```toml
[project.optional-dependencies]
anthropic = ["anthropic>=0.40"]
openai    = ["openai>=1.50"]
minimax   = []   # uses httpx (core dep)
all       = ["motosan-ai[anthropic,openai]"]
```

### Provider Protocol

```python
class ProviderProtocol(Protocol):
    async def chat(self, req: ChatRequest) -> ChatResponse: ...
    async def stream(self, req: ChatRequest) -> AsyncIterator[StreamEvent]: ...
```

### File Structure

```
sdks/python/
├── pyproject.toml
└── motosan_ai/
    ├── __init__.py
    ├── client.py       Client (async + sync wrapper)
    ├── types.py        Message, ChatRequest, ChatResponse, Usage, StreamEvent
    ├── error.py        MotosanError and subclasses
    └── providers/
        ├── base.py       ProviderProtocol
        ├── anthropic.py
        ├── openai.py
        └── minimax.py
```

## Milestones

| Version | Scope | Target |
|---------|-------|--------|
| v0.1.0 | Rust SDK — 3 providers, streaming, tool calling, tests | 2026-03-24 |
| v0.2.0 | Python SDK — 3 providers, async + sync, tests | 2026-04-07 |
| v0.3.0 | TypeScript SDK | 2026-04-28 |
| v1.0.0 | All stable, docs site, crates.io + PyPI publish | TBD |
