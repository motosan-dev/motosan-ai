# motosan-ai

Multi-language, multi-provider AI SDK. One unified interface for Anthropic, OpenAI, MiniMax — and more.

[![Rust CI](https://github.com/motosan-dev/motosan-ai/actions/workflows/ci-rust.yml/badge.svg)](https://github.com/motosan-dev/motosan-ai/actions/workflows/ci-rust.yml)
[![Python CI](https://github.com/motosan-dev/motosan-ai/actions/workflows/ci-python.yml/badge.svg)](https://github.com/motosan-dev/motosan-ai/actions/workflows/ci-python.yml)

## Why motosan-ai?

Most AI SDKs are provider-specific. If you start with Anthropic and later want to try OpenAI or MiniMax, you rewrite your integration.

`motosan-ai` gives you a **single interface** across providers. Switch models by changing one line.

```rust
// Rust — swap provider without touching business logic
let client = Client::builder()
    .provider(Provider::Anthropic)  // change to Provider::OpenAI — done
    .api_key(&api_key)
    .build()?;
```

```python
# Python — same interface, any provider
client = Client(provider="openai")  # change to "anthropic" — done
response = await client.chat([{"role": "user", "content": "Hello"}])
```

## Languages

| Language | Package | Status |
|----------|---------|--------|
| 🦀 Rust | `motosan-ai` (crates.io) | 🚧 v0.1.0 in progress |
| 🐍 Python | `motosan-ai` (PyPI) | 🚧 v0.2.0 planned |
| 🔷 TypeScript | `@motosan-ai/core` (npm) | 📋 v0.3.0 planned |
| 🐹 Go | `motosan-dev/motosan-ai/sdks/go` | 📋 v0.4.0 planned |

## Providers

| Provider | Models | Feature flag |
|----------|--------|-------------|
| Anthropic | claude-opus-4, claude-sonnet-4 | `anthropic` |
| OpenAI | gpt-4o, o3, o4-mini | `openai` |
| MiniMax | MiniMax-Text-01 | `minimax` |

## Quick Start

### Rust

```toml
[dependencies]
motosan-ai = { version = "0.1", features = ["anthropic"] }
tokio = { version = "1", features = ["full"] }
```

```rust
use motosan_ai::{Client, Message, Provider};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .provider(Provider::Anthropic)
        .api_key(std::env::var("ANTHROPIC_API_KEY")?)
        .build()?;

    let response = client.chat(vec![
        Message::user("What is the capital of France?"),
    ]).await?;

    println!("{}", response.content);
    Ok(())
}
```

### Python

```bash
pip install "motosan-ai[anthropic]"
```

```python
import asyncio
from motosan_ai import Client

async def main():
    client = Client(provider="anthropic")

    response = await client.chat([
        {"role": "user", "content": "What is the capital of France?"}
    ])
    print(response.content)

asyncio.run(main())
```

## Streaming

### Rust

```rust
use futures_util::StreamExt;

let mut stream = client.stream(vec![Message::user("Tell me a story")]).await?;
while let Some(event) = stream.next().await {
    if let Ok(token) = event {
        print!("{}", token.content);
    }
}
```

### Python

```python
async with client.stream([{"role": "user", "content": "Tell me a story"}]) as stream:
    async for token in stream:
        print(token.content, end="", flush=True)
```

## Provider-specific Options

For features that don't map across providers, use `provider_options`:

```rust
// Anthropic extended thinking
let response = client.chat(messages)
    .provider_options(json!({
        "thinking": { "type": "enabled", "budget_tokens": 5000 }
    }))
    .await?;
```

```python
# OpenAI reasoning effort
response = await client.chat(
    messages,
    provider_options={"reasoning_effort": "high"}
)
```

## Repository Structure

```
motosan-ai/
├── sdks/
│   ├── rust/       Rust crate (feature-flagged providers)
│   ├── python/     Python package
│   ├── typescript/ TypeScript package (planned)
│   └── go/         Go module (planned)
├── specs/          Shared type definitions
├── docs/           Architecture decisions and plans
└── examples/       Cross-language usage examples
```

## License

MIT
