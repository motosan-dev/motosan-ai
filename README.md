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
| Rust | [`motosan-ai`](https://crates.io/crates/motosan-ai) | v0.10.0 |
| Python | [`motosan-ai`](https://pypi.org/project/motosan-ai/) | v0.5.0 |

## Install

```toml
# Rust (Cargo.toml)
[dependencies]
motosan-ai = { version = "0.10.0", features = ["anthropic"] }
# features: anthropic | openai | minimax | ollama | ollama_native | full | claude-code | codex-cli
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

> **OpenAI-compatible providers** (Groq, DeepSeek, Together, self-hosted proxies, etc.) work via the `openai` feature with a custom chat URL — pass the full endpoint you want POSTed:
>
> ```rust
> Client::builder()
>     .provider(Provider::OpenAI)
>     .api_key("...")
>     .openai_chat_url("https://api.groq.com/openai/v1/chat/completions")
>     .build()?;
> ```

## Backends (Rust)

`motosan-ai` supports four ways to run LLM turns, all returning the same `ChatResponse` / `StreamEvent` types:

```rust
use motosan_ai::{Client, Message, Provider};

// 1. API key — direct HTTP to Anthropic
let client = Client::builder()
    .provider(Provider::Anthropic)
    .api_key("sk-ant-api03-...")
    .build()?;
let response = client.chat(vec![Message::user("Hello")]).await?;

// 2. OAuth token — same Client, auto-detected from token prefix
let client = Client::builder()
    .provider(Provider::Anthropic)
    .api_key("sk-ant-oat01-...")   // OAuth format
    .build()?;
let response = client.chat(vec![Message::user("Hello")]).await?;
```

```rust
// 3. Claude Code CLI — no API key needed (uses your local claude session)
// Requires: cargo add motosan-ai --features claude-code
use motosan_ai::ClaudeCodeProvider;

let client = ClaudeCodeProvider::new();
let response = client.chat(request).await?;
let stream = client.stream(request).await?;   // NDJSON streaming
```

```rust
// 4. Codex CLI — shells out to OpenAI's `codex exec --json`
// Requires: cargo add motosan-ai --features codex-cli
use motosan_ai::{CodexCliProvider, codex_cli::SandboxMode};

let client = CodexCliProvider::new()
    .sandbox(SandboxMode::WorkspaceWrite)
    .ephemeral(true);
let response = client.chat(request).await?;
let stream = client.stream(request).await?;
```

> **CLI backend limitations (Claude Code / Codex CLI):** Tool calls run internally by the CLI and are **not** surfaced on `ChatResponse.tool_calls` (always empty). Both require the corresponding binary installed and authenticated. Enable with `--features claude-code` or `--features codex-cli`.

## Features

- **Chat & Streaming** — `chat()`, `stream()`, `chat_with()`, `stream_with()`, `stream_collect()`
- **Tool Use** — define tools, multi-turn tool loops, streaming tool calls
- **Vision** — send images alongside text (base64 or URL)
- **ThinkStripper** — auto-strips `<think>` reasoning blocks from streaming output
- **Retry** — configurable exponential backoff with jitter and `Retry-After` support
- **Stream Read Timeout** — configurable per-chunk timeout to prevent SSE hanging
- **Extended Thinking** — first-class support for Anthropic thinking mode
- **MCP** — server-side MCP support in `ChatRequest`
- **Claude Code Backend** — shell out to `claude` CLI via `ClaudeCodeProvider` (`--features claude-code`)
- **Codex CLI Backend** — shell out to `codex exec --json` via `CodexCliProvider` with sandbox / profile / config-override support (`--features codex-cli`)

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
