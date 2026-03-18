# motosan-ai (Rust SDK)

Feature-flagged Rust SDK for Anthropic, OpenAI, and MiniMax.

## Quickstart

```rust
use motosan_ai::{Client, Message, Provider};

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let client = Client::builder()
    .provider(Provider::Anthropic)
    .api_key(std::env::var("ANTHROPIC_API_KEY")?)
    .build()?;

let response = client.chat(vec![Message::user("hello")]).await?;
println!("{}", response.content);
# Ok(())
# }
```

## Streaming Example

```rust
use motosan_ai::{Client, Message, Provider};
use tokio_stream::StreamExt;

# async fn demo_stream() -> Result<(), Box<dyn std::error::Error>> {
let client = Client::builder()
    .provider(Provider::OpenAI)
    .api_key(std::env::var("OPENAI_API_KEY")?)
    .build()?;

let mut stream = client.stream(vec![Message::user("stream hello")]).await?;
while let Some(event) = stream.next().await {
    if event.done {
        break;
    }
    print!("{}", event.content);
}
# Ok(())
# }
```

## Build

```bash
cargo build -p motosan-ai
cargo build -p motosan-ai --all-features
```

## Features

- `anthropic`
- `openai`
- `minimax`
- `full` (enables all providers)

## Model Defaults

- Anthropic: `claude-sonnet-4-6`
- OpenAI: `gpt-5.3-codex`
- MiniMax: `MiniMax-M2.5-highspeed`

Override per client:

```rust
let client = Client::builder()
    .provider(Provider::OpenAI)
    .api_key("...")
    .model("gpt-4o")
    .build()?;
```

Override per request:

```rust
use motosan_ai::{ChatRequest, Message};

let request = ChatRequest::builder()
    .message(Message::user("hello"))
    .model("gpt-4o")
    .build();
```

## Error Handling Example

```rust
match client.chat(vec![Message::user("hello")]).await {
    Ok(response) => println!("{}", response.content),
    Err(error) => eprintln!("request failed: {error}"),
}
```

## OpenAI Provider Options

Advanced OpenAI-compatible usage:

```rust
use motosan_ai::providers::openai::{OpenAIAuthStyle, OpenAIProvider};

let provider = OpenAIProvider::new("api-key", None, Some("https://api.openai.com".to_string()))
    .with_auth_style(OpenAIAuthStyle::Bearer)
    .with_responses_fallback(true);
```

- `with_auth_style(...)`: supports `Bearer`, `XApiKey`, or custom header.
- `with_responses_fallback(true)`: when `/v1/chat/completions` returns `404`, fallback to `/v1/responses`.

The same behavior is available from `Client::builder()`:

```rust
use motosan_ai::{Client, Provider};

let client = Client::builder()
    .provider(Provider::OpenAI)
    .api_key("...")
    .openai_auth_x_api_key() // or .openai_auth_custom_header("X-Auth-Token")
    .openai_responses_fallback(true)
    .build()?;
```

## Retry Policy

Retry is enabled by default for transient failures (`429`, `5xx`, timeout/connect errors).

```rust
use motosan_ai::{Client, Provider, RetryPolicy};

let retry_policy = RetryPolicy::new()
    .max_retries(3)
    .base_delay_ms(100)
    .max_delay_ms(2_000)
    .jitter(true)
    .respect_retry_after(true);

let client = Client::builder()
    .provider(Provider::OpenAI)
    .api_key("...")
    .retry_policy(retry_policy)
    .build()?;
```

## Anthropic Auth Matrix

- `sk-ant-api*` or regular Anthropic API key → `x-api-key` header
- `sk-ant-oat01*` OAuth token → OAuth mode:
  - `Authorization: Bearer <token>` header
  - `anthropic-beta: claude-code-20250219,oauth-2025-04-20,...` headers
  - `user-agent: claude-code/<version>` + `x-app: cli` identity headers
  - Streaming required (non-streaming returns 400)
  - System prompt sent as array of blocks (prefix block with `cache_control` + user system block)
  - Array format for user message content
  - Claude Code system prompt prefix auto-injected
  - `chat()` auto-redirects to `stream()` and collects result (including `tool_calls`)

When using `Provider::Anthropic`, pass either token string into `Client::builder().api_key(...)`.
The SDK auto-selects the correct auth mode and request format based on token prefix.

## Testing

```bash
# Unit tests (mock, no API needed)
cargo test --all-features

# Live integration tests (requires ANTHROPIC_API_KEY, supports OAuth tokens)
ANTHROPIC_API_KEY=... cargo test --features full --test anthropic_live -- --test-threads=1
```

## MiniMax Compatibility

MiniMax provider uses OpenAI-compatible chat completions path (`/chat/completions`) with `Authorization: Bearer` authentication.
The SDK also maps MiniMax payload-level `base_resp` errors (e.g. invalid API key) into SDK error variants.
For compatibility, MiniMax system prompts are merged into the first user message (instead of sending `role: system`).

For `MiniMax-M2.5-highspeed`, responses can include `<think>...</think>` reasoning blocks.
By default, the SDK strips these blocks and returns only the final answer text.
If `message.content` is empty (or only contains `<think>` blocks), the SDK falls back to
`message.reasoning_content` for chat and stream parsing.

To expose raw reasoning content:

```rust
use motosan_ai::{Client, Provider};

let client = Client::builder()
    .provider(Provider::Minimax)
    .api_key("...")
    .minimax_expose_reasoning(true)
    .build()?;
```

Or per request:

```rust
use motosan_ai::{ChatRequest, Message};
use serde_json::json;

let request = ChatRequest::builder()
    .message(Message::user("hello"))
    .provider_options(json!({"minimax_expose_reasoning": true}))
    .build();
```

Error handling policy reference: `docs/error-handling-policy.md`.

## Publishing

Automated via `publish-rust.yml` on `rust-v*` tag push → crates.io.

```bash
# Tag and push to trigger publish
git tag -a rust-vX.Y.Z -m "rust-vX.Y.Z — summary"
git push origin rust-vX.Y.Z

# Manual (emergency)
cargo publish
```

Rust and Python SDKs are versioned independently.

## Model Maintenance (survey process)

When updating model defaults, verify against official provider documentation:

- Anthropic models: https://docs.anthropic.com/
- OpenAI models: https://platform.openai.com/docs/models
- MiniMax API docs: https://www.minimax.io/platform/document

Prefer stable aliases for defaults and keep dated snapshots listed in `src/models.rs`.
