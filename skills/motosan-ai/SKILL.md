---
name: motosan-ai
description: Help developers use the motosan-ai SDK (Python and Rust) — LLM chat, streaming, tool use, ThinkStripper, and multi-provider setup. Use when a user asks how to use motosan-ai, integrate an LLM provider (Anthropic, OpenAI, Ollama, MiniMax), implement streaming responses, handle tool calls, or filter <think> tags in LLM output.
---

# motosan-ai SDK

Multi-provider LLM SDK — Python 0.4.2 / Rust 0.3.3

## Install

```bash
# Python
pip install "motosan-ai[anthropic]"          # Anthropic only
pip install "motosan-ai[anthropic,openai]"   # Multiple providers

# Rust (Cargo.toml)
motosan-ai = { version = "0.3.3", features = ["anthropic"] }
# features: anthropic | openai | minimax | ollama | ollama_native | full
```

## Environment Variables

| Provider   | Env var              |
|------------|----------------------|
| Anthropic  | `ANTHROPIC_API_KEY`  |
| OpenAI     | `OPENAI_API_KEY`     |
| MiniMax    | `MINIMAX_API_KEY`    |
| Ollama     | (none — local)       |

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
    .build();
let resp = client.chat(vec![Message::user("Hi")]).await?;
println!("{}", resp.content);
```

## When to Read References

| Task | File |
|------|------|
| Full Python API (`chat_with`, `stream`, `ChatRequest`, `Message` helpers, `RetryPolicy`) | `references/python-api.md` |
| Full Rust API (`ClientBuilder`, `stream_with`, `BoxStream`, feature flags) | `references/rust-api.md` |
| Tool calling, multi-turn tool loop | `references/tool-use.md` |
| Streaming events, ThinkStripper | `references/streaming.md` |
