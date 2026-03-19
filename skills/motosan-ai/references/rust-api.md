# motosan-ai Rust API Reference

## Cargo.toml

```toml
[dependencies]
motosan-ai = { version = "0.3.3", features = ["anthropic"] }
# features: anthropic | openai | minimax | ollama | ollama_native | full
```

## ClientBuilder

```rust
use motosan_ai::{Client, Provider, RetryPolicy};

let client = Client::builder()
    .provider(Provider::Anthropic)
    .api_key(std::env::var("ANTHROPIC_API_KEY")?)
    .model("claude-3-5-sonnet-20241022")           // optional
    .retry_policy(RetryPolicy::new())              // optional
    .build();
```

Provider variants: `Provider::Anthropic` | `Provider::OpenAI` | `Provider::MiniMax` | `Provider::Ollama`

## Core Methods

### `chat()` — single-turn

```rust
use motosan_ai::Message;

let resp = client.chat(vec![Message::user("Hello")]).await?;
println!("{}", resp.content);
println!("tokens: {}+{}", resp.usage.input_tokens, resp.usage.output_tokens);
```

### `chat_with()` — full control

```rust
use motosan_ai::{ChatRequest, Message};

let resp = client.chat_with(ChatRequest {
    messages: vec![Message::user("Hello")],
    model: Some("claude-3-5-sonnet-20241022".into()),
    system: Some("You are a helpful assistant.".into()),
    temperature: Some(0.7),
    max_tokens: Some(1024),
    tools: Some(vec![...]),           // see tool-use.md
    provider_options: None,
}).await?;
```

### `stream()` — streaming

```rust
use futures_util::StreamExt;

let mut stream = client.stream(vec![Message::user("Tell me a story")]).await?;
while let Some(event) = stream.next().await {
    let event = event?;
    if event.is_text() {
        print!("{}", event.content);
    }
    if event.done { break; }
}
```

### `stream_with()` — streaming + tools

```rust
let mut stream = client.stream_with(ChatRequest {
    messages,
    tools: Some(tools),
    ..Default::default()
}).await?;
```

## StreamEvent

```rust
event.event_type           // StreamEventType enum
event.content              // String
event.done                 // bool
event.tool_call_id         // Option<String>
event.tool_call_name       // Option<String>
event.tool_call_args_delta // Option<String>

// Convenience
event.is_text()            // event_type == Text
event.is_tool_call_start()
event.is_tool_call_end()
```

## Message Helpers

```rust
Message::user("Hello")
Message::assistant("Hi")
Message::system("You are helpful")
Message::tool_result("call_id", json_string)
```

## RetryPolicy

```rust
use motosan_ai::RetryPolicy;

RetryPolicy {
    max_retries: 3,
    base_delay_ms: 500,
    max_delay_ms: 10_000,
    jitter: true,
    respect_retry_after: true,
}
```

## BoxStream Type

`stream()` and `stream_with()` return `Result<BoxStream, MotosanError>` where `BoxStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, MotosanError>> + Send>>`.

Consume with `futures_util::StreamExt::next()` in a loop.
