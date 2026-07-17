# motosan-ai Rust API Reference

## Cargo.toml

```toml
[dependencies]
motosan-ai = { version = "0.24.0", features = ["anthropic"] }
# features: anthropic | openai | minimax | ollama | ollama_native | full
#           gemini | gemini-code-assist | claude-code | codex-cli | gemini-cli
```

## Public Exports

```rust
// Types
use motosan_ai::{
    Client, ClientBuilder, Provider,
    Message, Role, Tool, ToolCall,
    ChatRequest, ChatRequestBuilder, ChatResponse,
    Usage, StopReason,
    StreamEvent, StreamEventType, BoxStream,
    RetryPolicy, MotosanError,
};
```

## ClientBuilder

```rust
use motosan_ai::{Client, Provider, RetryPolicy};

let client = Client::builder()
    .provider(Provider::Anthropic)                   // required
    .api_key(std::env::var("ANTHROPIC_API_KEY")?)    // required
    .model("claude-opus-4-8")                        // optional override (default: claude-sonnet-4-6)
    .retry_policy(RetryPolicy::new().max_retries(5)) // optional
    .build()?;  // -> Result<Client, MotosanError>
```

Provider variants: `Provider::Anthropic` | `Provider::OpenAI` | `Provider::Minimax` | `Provider::Ollama`

Anthropic Opus 4.8/4.7/4.6 use adaptive thinking when `.thinking(...)` is set (`thinking.type = "adaptive"`, summarized display, `output_config.effort = "high"`), matching pi. Other thinking-capable Claude models use the budget-token shape.

### Provider-specific builder methods

```rust
// OpenAI auth styles
.openai_auth_bearer()                     // Authorization: Bearer (default)
.openai_auth_x_api_key()                  // X-Api-Key header
.openai_auth_custom_header("X-Auth-Token") // custom header name
.openai_responses_fallback(true)           // fallback to /v1/responses on 404

// MiniMax
.minimax_base_url("https://api.minimaxi.com/anthropic")  // optional CN endpoint override

// Ollama
.ollama_base_url("http://localhost:11434")  // default
.ollama_native(true)       // use native /api/chat instead of OpenAI-compat
.ollama_think("qwq")       // reasoning model name
.ollama_keep_alive("5m")   // keep model loaded
.ollama_num_ctx(4096)       // context window size

// Stream read timeout (all providers)
.stream_read_timeout_secs(30)  // terminate stream after 30s of silence
```

## Core Methods

### `chat()` — single turn

```rust
let resp = client.chat(vec![Message::user("Hello")]).await?;
println!("{}", resp.content);       // String
println!("{:?}", resp.stop_reason); // StopReason: EndTurn | MaxTokens | ToolUse | Stop | Other
println!("{}+{}", resp.usage.input_tokens, resp.usage.output_tokens);
println!("{:?}", resp.tool_calls);  // Vec<ToolCall> — empty if no tool use
```

### `chat_with()` — full control

```rust
use motosan_ai::ChatRequest;

let request = ChatRequest::builder()
    .messages(vec![Message::user("Hello")])
    .system("You are a helpful assistant.")
    .model("claude-opus-4-8")
    .temperature(0.7)
    .max_tokens(1024)
    .tools(vec![...])  // Vec<Tool>; Tool composes ToolSchema (0.18.0+) — see tool-use.md
    .provider_options(json!({"key": "val"}))  // provider escape hatch
    .build();
// Bridging agent-side schemas: .tool_schemas(&[ToolSchema]) replaces the
// removed .tool_defs(&[ToolDef]) (the agent-tool feature was dropped in 0.18.0).

let resp = client.chat_with(request).await?;
```

Builder also supports `.message(msg)` to push a single message.

### `stream()` — streaming text

```rust
use futures_util::StreamExt;

let mut stream = client.stream(vec![Message::user("Tell me a story")]).await?;
while let Some(event) = stream.next().await {
    if !event.content.is_empty() {
        print!("{}", event.content);
    }
    if event.done { break; }
}
```

### `stream_with()` — streaming + tools + full control

```rust
use motosan_ai::StreamEventType;

let request = ChatRequest::builder()
    .messages(messages)
    .tools(tools)
    .system("You are helpful")
    .build();

let mut stream = client.stream_with(request).await?;
while let Some(event) = stream.next().await {
    match event.event_type {
        StreamEventType::Text => print!("{}", event.content),
        StreamEventType::ToolCallStart => {
            // event.tool_call_id: Option<String>
            // event.tool_call_name: Option<String>
        },
        StreamEventType::ToolCallArgs => {
            // event.tool_call_args_delta: Option<String>
            // event.tool_call_id: Option<String> (may be None for some providers)
        },
        StreamEventType::ToolCallEnd => {
            // event.tool_call_id: Option<String>
        },
    }
    if event.done { break; }
}
```

### `stream_collect()` — stream + collect into ChatResponse

```rust
let resp = client.stream_collect(vec![Message::user("Hello")]).await?;
println!("{}", resp.content);
```

### `stream_collect_with()` — full control variant

```rust
let request = ChatRequest::builder()
    .messages(vec![Message::user("Hello")])
    .tools(tools)
    .build();
let resp = client.stream_collect_with(request).await?;
```

## BoxStream Type

```rust
pub type BoxStream = Pin<Box<dyn Stream<Item = StreamEvent> + Send>>;
```

**Important**: Items are `StreamEvent` directly — NOT wrapped in `Result`. Consume with `futures_util::StreamExt::next()`.

## StreamEvent

```rust
pub struct StreamEvent {
    pub content: String,
    pub done: bool,
    pub tool_call_id: Option<String>,
    pub tool_call_name: Option<String>,
    pub tool_call_args_delta: Option<String>,
    pub usage: Option<Usage>,
    pub event_type: StreamEventType,  // Text | ToolCallStart | ToolCallArgs | ToolCallEnd | Usage
}
```

Constructors (useful for testing):
```rust
StreamEvent::text("hello")
StreamEvent::done()
StreamEvent::tool_call_start("tc-1", "get_weather")
StreamEvent::tool_call_args("partial json")
StreamEvent::tool_call_args_with_id("tc-1", "partial json")
StreamEvent::tool_call_end()
StreamEvent::tool_call_end_with_id("tc-1")
```

## Message Helpers

```rust
Message::user("Hello")
Message::assistant("Hi")
Message::system("You are helpful")
Message::tool_result("call_id", "result JSON string")
Message::tool("result", "call_id")                     // alias for tool_result
Message::assistant_with_tool_calls("text", tool_calls)  // Vec<ToolCall>
```

## RetryPolicy

```rust
use motosan_ai::RetryPolicy;

let policy = RetryPolicy::new()  // defaults below
    .max_retries(3)              // default 3
    .base_delay_ms(100)          // default 100
    .max_delay_ms(2_000)         // default 2000
    .jitter(true)                // default true
    .respect_retry_after(true);  // default true
```

Retries on: 429, 5xx, timeout/connect errors. Backoff: `base * 2^(attempt-1)`, capped, with jitter.

## Error Handling

```rust
use motosan_ai::MotosanError;

match result {
    Err(MotosanError::Auth(msg)) => ...,
    Err(MotosanError::RateLimit(msg)) => ...,
    Err(MotosanError::InvalidRequest(msg)) => ...,
    Err(MotosanError::Config(msg)) => ...,
    Err(MotosanError::ProviderError(msg)) => ...,
    Err(MotosanError::Network(msg)) => ...,
    Err(MotosanError::Stream(msg)) => ...,
    Err(MotosanError::StreamReadTimeout(secs)) => ...,  // stream silence exceeded timeout
    Err(MotosanError::UnsupportedFeature(msg)) => ...,
}
```

## Anthropic Auth Matrix

- Regular key (`sk-ant-api*`) → `x-api-key` header
- OAuth token (`sk-ant-oat01*`) → auto-switches to OAuth mode:
  - `Authorization: Bearer` + anthropic-beta headers
  - Streaming required (chat() auto-redirects to stream())
  - System prompt sent as array with cache_control prefix
