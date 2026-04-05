# motosan-ai

Multi-language, multi-provider AI SDK. One unified interface for Anthropic, OpenAI, MiniMax, Ollama — and more.

## Why motosan-ai?

Most AI SDKs are provider-specific. Switch providers = rewrite your integration.

`motosan-ai` gives you a **single interface** across providers. Switch models by changing one line.

```rust
// Rust — swap provider without touching business logic
let client = Client::builder()
    .provider(Provider::Anthropic)  // → Provider::OpenAI — done
    .api_key(&api_key)
    .build()?;
```

```python
# Python — same interface, any provider
client = Client.anthropic()  # → Client.openai() — done
response = await client.chat([Message.user("Hello")])
```

## Languages

| Language | Package | Version |
|----------|---------|---------|
| Rust | [`motosan-ai`](https://crates.io/crates/motosan-ai) | v0.5.4 |
| Python | [`motosan-ai`](https://pypi.org/project/motosan-ai/) | v0.4.2 |

## Install

```toml
# Rust (Cargo.toml)
[dependencies]
motosan-ai = { version = "0.5.4", features = ["anthropic"] }
# features: anthropic | openai | minimax | ollama | ollama_native | full

# To use ClaudeCodeClient (shells out to the claude CLI):
motosan-ai = { version = "0.5.4", features = ["claude-code"] }
```

```bash
# Python
pip install "motosan-ai[anthropic]"
pip install "motosan-ai[full]"   # all providers
```

## Providers

| Provider | Default model | Rust feature | Python extra |
|----------|---------------|-------------|-------------|
| Anthropic | `claude-sonnet-4-6` | `anthropic` | `[anthropic]` |
| OpenAI | `gpt-5.3-codex` | `openai` | `[openai]` |
| MiniMax | `MiniMax-M2.5-highspeed` | `minimax` | `[minimax]` |
| Ollama | `llama3.2` | `ollama` / `ollama_native` | `[ollama]` |

## Backends (Rust)

The Rust SDK offers three interchangeable backends that all produce the same `ChatResponse` type:

```rust
use motosan_ai::{Client, Provider, Message};

// 1. API key — standard provider client
let client = Client::builder()
    .provider(Provider::Anthropic)
    .api_key("sk-ant-api03-...")
    .build()?;

// 2. OAuth token — same Client, auto-detected by key prefix
let client = Client::builder()
    .provider(Provider::Anthropic)
    .api_key("sk-ant-oat01-...")   // OAuth format
    .build()?;

// 3. Claude Code CLI — no API key needed (uses your local claude session)
#[cfg(feature = "claude-code")]
use motosan_ai::ClaudeCodeClient;

#[cfg(feature = "claude-code")]
let claude = ClaudeCodeClient::new();
```

All three support `chat()` and `stream()`:

```rust
// Client (backends 1 & 2)
let response = client.chat(vec![Message::user("Hello")]).await?;

// ClaudeCodeClient (backend 3)
let request = motosan_ai::types::ChatRequest {
    messages: vec![Message::user("Hello")],
    ..Default::default()
};
let response = claude.chat(request).await?;
```

**ClaudeCodeClient limitations:**
- Requires the `claude` CLI installed and authenticated (`claude` in `PATH`, or set `CLAUDE_CODE_PATH`)
- No tool calling — `tool_calls` is always empty
- Feature-gated: add `features = ["claude-code"]` to your `Cargo.toml`

## Features

- **Chat & Streaming** — `chat()`, `stream()`, `chat_with()`, `stream_with()`, `stream_collect()`
- **Tool Use** — define tools, multi-turn tool loops, streaming tool calls
- **Vision** — send images alongside text (base64 or URL)
- **ThinkStripper** — auto-strips `<think>` reasoning blocks from streaming output
- **Retry** — configurable exponential backoff with jitter and `Retry-After` support
- **Stream Read Timeout** — configurable per-chunk timeout to prevent SSE hanging
- **Extended Thinking** — first-class support for Anthropic thinking mode
- **MCP** — server-side MCP support in `ChatRequest`

## Quick Example

```rust
use motosan_ai::{Client, Message, Provider};
use tokio_stream::StreamExt;

let client = Client::builder()
    .provider(Provider::Anthropic)
    .api_key(std::env::var("ANTHROPIC_API_KEY")?)
    .stream_read_timeout_secs(30)
    .build()?;

let mut stream = client.stream(vec![Message::user("Hello")]).await?;
while let Some(event) = stream.next().await {
    if event.done { break; }
    print!("{}", event.content);
}
```

```python
from motosan_ai import Client, Message

client = Client.anthropic()
async for event in await client.stream([Message.user("Hello")]):
    if event.done:
        break
    print(event.content, end="", flush=True)
```

## Development

```bash
# Optional: Nix + direnv for a fully reproducible environment
# cd into the project — direnv auto-activates nix develop

fmt           # Format everything (Rust + Python + TOML + Nix)
check-all     # Full CI gate (lint + test both SDKs)
test-live     # Anthropic integration tests
```

See [`AGENTS.md`](AGENTS.md) for full development guide.

## For AI Agents

Fetch [`llms.txt`](https://raw.githubusercontent.com/motosan-dev/motosan-ai/main/llms.txt) for a quick-start API reference.

## License

MIT
