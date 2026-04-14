# motosan-ai (Rust SDK)

Feature-flagged Rust SDK for Anthropic, OpenAI, MiniMax, Ollama, and the Claude Code CLI.

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
        // Terminal event carries the provider-reported stop reason when available.
        if let Some(reason) = event.stop_reason {
            eprintln!("\n[stop_reason: {reason:?}]");
        }
        break;
    }
    print!("{}", event.content);
}
# Ok(())
# }
```

Each provider stream emits **exactly one** terminal `done` event, and `event.stop_reason` carries the provider's reported reason when present (`Anthropic` `message_delta.stop_reason`, `OpenAI` / `MiniMax` `finish_reason`).

## Vision / Multimodal

Send images alongside text using `Message::user_with_image()`:

```rust
use motosan_ai::{Client, Message, Provider};

# async fn demo_vision() -> Result<(), Box<dyn std::error::Error>> {
let client = Client::builder()
    .provider(Provider::Anthropic)
    .api_key(std::env::var("ANTHROPIC_API_KEY")?)
    .build()?;

let response = client.chat(vec![
    Message::user_with_image(
        "What is in this image?",
        &base64_png_data,    // base64-encoded image
        "image/png",
    ),
]).await?;
println!("{}", response.content);
# Ok(())
# }
```

For multiple content blocks, use `Message::user_with_blocks()`:

```rust
use motosan_ai::{ContentBlock, ImageSource, Message};

let msg = Message::user_with_blocks(vec![
    ContentBlock::Text { text: "Compare these two images".to_string() },
    ContentBlock::Image { source: ImageSource::Base64 {
        media_type: "image/png".to_string(),
        data: first_image_b64.to_string(),
    }},
    ContentBlock::Image { source: ImageSource::Url {
        url: "https://example.com/second.png".to_string(),
    }},
]);
```

Works with both Anthropic and OpenAI providers. The SDK automatically converts to each provider's native format.

## Build

```bash
cargo build -p motosan-ai
cargo build -p motosan-ai --all-features
```

## Features

- `anthropic`
- `openai`
- `minimax`
- `ollama` (OpenAI-compatible mode)
- `ollama_native` (native `/api/chat` endpoint with NDJSON streaming)
- `full` (enables all providers)

## Model Defaults

- Anthropic: `claude-sonnet-4-6`
- OpenAI: `gpt-5.3-codex`
- MiniMax: `MiniMax-M2.5-highspeed`
- Ollama: `llama3.2`

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

// Default — points at api.openai.com/v1.
let provider = OpenAIProvider::new("api-key", None)
    .with_auth_style(OpenAIAuthStyle::Bearer)
    .with_responses_fallback(true);

// Override the endpoint URL for OpenAI-compatible providers.
// Pass the full URL you want POSTed — no base_url magic, no /v1 injection.
let groq = OpenAIProvider::new("api-key", None)
    .with_chat_url("https://api.groq.com/openai/v1/chat/completions");

let proxy = OpenAIProvider::new("api-key", None)
    .with_chat_url("https://my-proxy.example.com/any/path");
```

- `with_chat_url(url)`: full URL POSTed for chat completions. Defaults to `DEFAULT_OPENAI_CHAT_URL`. A single trailing `/` is trimmed defensively; no other normalization.
- `with_responses_url(url)`: full URL for the Responses API fallback. Defaults to `DEFAULT_OPENAI_RESPONSES_URL`. Only used when `with_responses_fallback(true)`.
- `with_auth_style(...)`: supports `Bearer`, `XApiKey`, or custom header.
- `with_responses_fallback(true)`: when chat completions returns `404`, fall back to the Responses endpoint (OpenAI-specific; most compatible providers don't expose it).

The same options are available from `Client::builder()`:

```rust
use motosan_ai::{Client, Provider};

let client = Client::builder()
    .provider(Provider::OpenAI)
    .api_key("...")
    .openai_auth_x_api_key() // or .openai_auth_custom_header("X-Auth-Token")
    .openai_chat_url("https://api.groq.com/openai/v1/chat/completions") // optional
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

## Stream Read Timeout

By default, SSE streams wait indefinitely for the next event. If the provider stops
sending data mid-stream (e.g. with large `tool_result` context), the client hangs.

Set a per-chunk read timeout to terminate the stream after a period of silence:

```rust
let client = Client::builder()
    .provider(Provider::Anthropic)
    .api_key("...")
    .stream_read_timeout_secs(30)  // terminate after 30s of silence
    .build()?;
```

When the timeout fires, the stream ends (`None`). This works with all providers.

## Collect Stream

Buffer a streaming response into a single `ChatResponse`:

```rust
// Convenience — stream + collect in one call
let resp = client.stream_collect(vec![Message::user("hello")]).await?;
println!("{}", resp.content);
println!("{:?}", resp.stop_reason);   // honors explicit provider reason

// Full control variant
let request = ChatRequest::builder()
    .messages(vec![Message::user("hello")])
    .build();
let resp = client.stream_collect_with(request).await?;
```

`collect_stream` honors any `stop_reason` reported on the terminal stream event, falling back to a tool-calls-based heuristic only when no reason was reported (e.g. legacy adapters).

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

## Claude Code Backend

The `claude-code` feature enables `ClaudeCodeClient`, which shells out to the `claude` CLI binary.

```toml
motosan-ai = { version = "0.9.0", features = ["claude-code"] }
```

```rust
use motosan_ai::ClaudeCodeClient;

let client = ClaudeCodeClient::new()         // uses $CLAUDE_CODE_PATH or "claude" in PATH
    .model("sonnet")                          // forwards --model sonnet to the CLI
    .agent_mode(false);                       // set true to enable --dangerously-skip-permissions
```

Model selection rules: `--model` is forwarded when the model string is non-empty and not `"default"` (case-insensitive). Pass `"default"` or omit `.model()` to let the CLI use its own default.

## Codex CLI Backend

The `codex-cli` feature enables `CodexCliClient`, which shells out to OpenAI's `codex exec --json` and parses the JSONL event stream.

```toml
motosan-ai = { version = "0.9.0", features = ["codex-cli"] }
```

```rust
use motosan_ai::{CodexCliClient, codex_cli::SandboxMode};

let client = CodexCliClient::new()           // uses $CODEX_PATH or "codex" in PATH
    .model("gpt-5.1-codex")                  // forwards --model
    .sandbox(SandboxMode::WorkspaceWrite)    // --sandbox workspace-write
    .profile("work")                         // --profile work (from ~/.codex/config.toml)
    .ephemeral(true)                         // --ephemeral (no session rollout files)
    .config_override("model_reasoning_effort", "\"low\"")  // repeated -c key=value
    .cd("/tmp/project");                     // --cd <dir>

let response = client.chat(request).await?;
let stream = client.stream(request).await?;
```

Notes:
- Codex emits **complete** `agent_message` items, not token deltas — `stream()` yields one text event per finalized message.
- `chat()` treats the **last** `agent_message` as `ChatResponse.content` and folds prior messages (preamble / tool narration) into `ChatResponse.thinking`.
- `tool_calls` is always empty — Codex runs tools internally via its sandboxed shell, those invocations are not surfaced.
- Authentication: Codex CLI uses `CODEX_API_KEY` or `~/.codex/auth.json`, not `OPENAI_API_KEY`.
- `agent_mode(true)` passes `--full-auto` (workspace-write sandbox + approvals off); can coexist with an explicit `sandbox()`.

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

## For AI Agents

If you're an AI coding assistant, fetch [`llms.txt`](https://raw.githubusercontent.com/motosan-dev/motosan-ai/main/llms.txt) for a quick-start guide with API examples, tool use patterns, and streaming setup.
