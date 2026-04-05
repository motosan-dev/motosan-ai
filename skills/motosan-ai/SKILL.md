---
name: motosan-ai
description: Help developers use the motosan-ai SDK (Python and Rust) — LLM chat, streaming, tool use, ThinkStripper, and multi-provider setup. Use when code imports motosan_ai, or user asks how to integrate Anthropic/OpenAI/Ollama/MiniMax via motosan-ai, implement streaming, handle tool calls, or filter <think> tags.
---

# motosan-ai SDK

Multi-provider LLM SDK — Python 0.4.2 / Rust 0.6.0

Providers: Anthropic, OpenAI, MiniMax, Ollama

## Install

```bash
# Python
pip install "motosan-ai[anthropic]"          # single provider
pip install "motosan-ai[anthropic,openai]"   # multiple providers
```

```toml
# Rust (Cargo.toml)
motosan-ai = { version = "0.6.0", features = ["anthropic"] }
# features: anthropic | openai | minimax | ollama | ollama_native | full
```

## Environment Variables

| Provider  | Env var             |
|-----------|---------------------|
| Anthropic | `ANTHROPIC_API_KEY` |
| OpenAI    | `OPENAI_API_KEY`    |
| MiniMax   | `MINIMAX_API_KEY`   |
| Ollama    | (none — local)      |

## Model Defaults

| Provider  | Default model             |
|-----------|---------------------------|
| Anthropic | `claude-sonnet-4-6`       |
| OpenAI    | `gpt-5.3-codex`          |
| MiniMax   | `MiniMax-M2.5-highspeed` |
| Ollama    | `llama3.2`               |

## Minimal Example

**Python:**
```python
from motosan_ai import Client, Message

client = Client.anthropic()                    # reads ANTHROPIC_API_KEY
resp = await client.chat([Message.user("Hi")]) # returns ChatResponse
print(resp.content)                            # str
```

**Rust:**
```rust
use motosan_ai::{Client, Provider, Message};
let client = Client::builder()
    .provider(Provider::Anthropic)
    .api_key(std::env::var("ANTHROPIC_API_KEY")?)
    .build()?;
let resp = client.chat(vec![Message::user("Hi")]).await?;
println!("{}", resp.content);
```

## When to Read References

| Task | File |
|------|------|
| Full Python API (`Client` factories, `chat_with`, `stream`, `ChatRequest`, `Message` helpers, `RetryPolicy`, errors) | `references/python-api.md` |
| Full Rust API (`ClientBuilder`, `chat_with`, `stream_with`, `BoxStream`, feature flags, `MotosanError`) | `references/rust-api.md` |
| Tool calling, multi-turn tool loop, ToolCall fields | `references/tool-use.md` |
| Streaming events, ThinkStripper, provider-specific streaming notes | `references/streaming.md` |
| Release process, version bump, tag convention, CI publish, CHANGELOG format | `references/release.md` |

## Key Design Decisions

- **`BoxStream` (Rust)**: `Pin<Box<dyn Stream<Item = StreamEvent> + Send>>` — items are `StreamEvent` directly, NOT `Result<StreamEvent>`
- **`ChatRequest`**: Use builder pattern in Rust (`ChatRequest::builder().messages(...).build()`), dataclass in Python
- **ThinkStripper**: Applied automatically in all `stream()` / `stream_with()` calls — no manual setup needed
- **Anthropic OAuth**: Auto-detected by token prefix (`sk-ant-oat01*`), `chat()` auto-redirects to `stream()` for OAuth tokens
- **Retry**: Enabled by default (3 retries, exponential backoff, jitter) for 429/5xx/timeout
