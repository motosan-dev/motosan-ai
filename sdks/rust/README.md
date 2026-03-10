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
- MiniMax: `MiniMax-Text-01`

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

Error handling policy reference: `docs/error-handling-policy.md`.

## Model Maintenance (survey process)

When updating model defaults, verify against official provider documentation:

- Anthropic models: https://docs.anthropic.com/
- OpenAI models: https://platform.openai.com/docs/models
- MiniMax API docs: https://www.minimax.io/platform/document

Prefer stable aliases for defaults and keep dated snapshots listed in `src/models.rs`.
