# motosan-ai (Rust SDK)

Feature-flagged Rust SDK for Anthropic, OpenAI, MiniMax, Ollama, Gemini (HTTP + Code Assist), and the Claude Code / Codex / Gemini CLIs.

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
while let Some(item) = stream.next().await {
    let event = item?; // stream items are Result<StreamEvent, MotosanError> since 0.20
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

A stream is complete only when the provider sends its terminal event (OpenAI `[DONE]` or a final `finish_reason` chunk — either suffices, Anthropic `message_stop`, Gemini / chatgpt-codex terminal frames). Since v0.24.0, EOF without any such event yields `Err(MotosanError::IncompleteStream(_))` (`"incomplete stream: <provider> ended without a terminal event"`) rather than reporting completion — truncation is distinguishable from completion. `event.stop_reason` carries the provider's reported reason when present (`Anthropic` `message_delta.stop_reason`, `OpenAI` / `MiniMax` `finish_reason`).

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

Works with Anthropic, OpenAI, and Gemini HTTP providers. The SDK automatically converts to each provider's native format.

## Build

```bash
cargo build -p motosan-ai
cargo build -p motosan-ai --all-features
```

## Features

All default-off (`default = []`). Public provider features:

- `anthropic`
- `openai`
- `minimax`
- `ollama` (OpenAI-compatible mode)
- `ollama_native` (native `/api/chat` endpoint with NDJSON streaming; `ollama-native` is an equivalent alias since 0.25.0)
- `gemini` (Google Generative AI HTTP API)
- `gemini-code-assist` (Google Cloud Code Assist HTTP API; depends on `gemini`)
- `chatgpt-codex` (ChatGPT-backend Responses API; OAuth bearer token, no API key)
- `claude-code` (local Claude Code CLI backend)
- `codex-cli` (local Codex CLI backend)
- `gemini-cli` (local Gemini CLI backend)
- `full` (enables HTTP providers: `anthropic`, `openai`, `minimax`, `ollama`, `ollama_native`, `ollama-native`, `gemini`, `gemini-code-assist`, `chatgpt-codex`)

### Feature architecture rules

1. Features whose names start with an underscore (`_http`, `_cli`) are internal
   aggregation layers: an implementation detail, NOT covered by semver. Never
   enable or depend on them directly.
2. New providers MUST route through `_http` (HTTP transports) or `_cli` (local
   CLI backends) in `[features]`. Shared transport code lives in `src/transport/`
   behind a single module-level gate — adding a new per-provider
   `#[cfg(any(...))]` enumeration in shared code is a review-blocking offense.
3. Docs and examples teach `ollama-native`; `ollama_native` remains a permanent
   alias with identical semantics.

## Model Defaults

- Anthropic: `claude-sonnet-4-6`
- OpenAI: `gpt-5.3-codex`
- MiniMax: `MiniMax-M2.7`
- Ollama: `llama3.2`
- Gemini: `gemini-2.5-flash`
- Gemini Code Assist: `gemini-2.5-flash`

Anthropic model catalog includes `claude-opus-4-8`; override the default with `.model("claude-opus-4-8")` when you want Opus. For Opus 4.8/4.7/4.6, `.thinking(...)` uses Anthropic adaptive thinking (`thinking.type = "adaptive"`, summarized display, `output_config.effort = "high"`) instead of budget-token thinking, matching pi.

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
- `with_responses_url(url)`: full URL for the Responses API fallback and native model API. Defaults to `DEFAULT_OPENAI_RESPONSES_URL`. Used when `with_responses_fallback(true)` or `with_responses_api(true)`.
- `with_auth_style(...)`: supports `Bearer`, `XApiKey`, or custom header.
- `with_responses_fallback(true)`: when chat completions returns `404`, fall back to the Responses endpoint (OpenAI-specific; most compatible providers don't expose it).
- `with_responses_api(true)`: route native `model_chat_with` / `model_stream_with` requests through `/v1/responses`. This is required for native Freeform/custom tools on OpenAI.

The same options are available from `Client::builder()`:

```rust
use motosan_ai::{Client, Provider};

let client = Client::builder()
    .provider(Provider::OpenAI)
    .api_key("...")
    .openai_auth_x_api_key() // or .openai_auth_custom_header("X-Auth-Token")
    .openai_chat_url("https://api.groq.com/openai/v1/chat/completions") // optional
    .openai_responses_fallback(true)
    .openai_responses_api(true)
    .build()?;
```

## Native Freeform/custom Tools

The legacy `ChatRequest`, `Tool`, `ToolCall`, `ChatResponse`, and
`StreamEvent` APIs remain function-tool-only. Rust v0.26.0 adds a parallel
native model API for ordered Function and Freeform/custom transport:

- `ModelChatRequest` / `ModelContextItem` preserve mixed message, tool-call,
  and tool-output history order for subsequent requests.
- `ModelToolSpec::Function(Tool)` sends a normal JSON-schema function tool.
- `ModelToolSpec::Freeform(FreeformTool)` sends an OpenAI Responses
  `custom` tool with mandatory `format` metadata.
- `ModelToolCall::Freeform { input, .. }` carries raw custom input exactly as
  received; the SDK never parses it as JSON or rewrites it into function
  arguments.
- `ModelStreamDelta::FreeformInput` streams raw input deltas and
  `ModelStreamDelta::ToolCallDone` carries the final authoritative call.

```rust
use motosan_ai::{
    Client, FreeformTool, FreeformToolFormat, Message, ModelChatRequest,
    ModelContextItem, ModelToolSpec, Provider,
};

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let client = Client::builder()
    .provider(Provider::OpenAI)
    .api_key(std::env::var("OPENAI_API_KEY")?)
    .openai_responses_api(true)
    .build()?;

let request = ModelChatRequest::builder()
    .context_item(ModelContextItem::Message(Message::user(
        "Write a JavaScript snippet.",
    )))
    .tool_spec(ModelToolSpec::Freeform(FreeformTool {
        name: "javascript".to_string(),
        description: "Execute raw JavaScript source.".to_string(),
        format: FreeformToolFormat {
            r#type: "grammar".to_string(),
            syntax: "javascript".to_string(),
            definition: "program ::= .*".to_string(),
        },
    }))
    .build();

let response = client.model_chat_with(request).await?;
println!("{:?}", response.tool_calls);
# Ok(())
# }
```

Supported providers fail fast when the selected transport is unavailable:
OpenAI requires `openai_responses_api(true)`; ChatGPT Codex supports the native
API through its existing Responses endpoint. Other providers return
`MotosanError::UnsupportedFeature` before network I/O for native Freeform
requests.

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

MiniMax routing uses Anthropic-compatible `/anthropic/v1/messages` via `Provider::Minimax`.
No dedicated `MinimaxProvider` type is required on the client path.

- Default model: `MiniMax-M2.7` (also supports `MiniMax-M2.7-highspeed`)
- Default base URL: `https://api.minimax.io/anthropic`
- CN base URL override: `.minimax_base_url("https://api.minimaxi.com/anthropic")`
- Serialization follows Anthropic wire format (`tool_use` / `tool_result` blocks)
- Capabilities are text-only (`ProviderCapabilities::text_only()`)

Example:

```rust
use motosan_ai::{Client, Message, Provider};

let client = Client::builder()
    .provider(Provider::Minimax)
    .api_key("...")
    .model("MiniMax-M2.7")
    .build()?;

let resp = client.chat(vec![Message::user("hello")]).await?;
```

CN endpoint:

```rust
let client = Client::builder()
    .provider(Provider::Minimax)
    .api_key("...")
    .minimax_base_url("https://api.minimaxi.com/anthropic")
    .build()?;
```

Error handling policy reference: `docs/error-handling-policy.md`.

## Claude Code Backend

The `claude-code` feature enables `ClaudeCodeProvider`, which shells out to the `claude` CLI binary. The provider exposes a builder covering every SDK-relevant flag that the `claude --print` mode accepts.

```toml
motosan-ai = { version = "0.27.0", features = ["claude-code"] }
```

**Option A — via `Client::builder()`** (since v0.11.0, unified with HTTP providers). Build the provider with all the claude-specific flags, then hand it to the `Client` setter:

```rust
use motosan_ai::claude_code::{EffortLevel, PermissionMode};
use motosan_ai::{ClaudeCodeProvider, Client, Provider};

let client = Client::builder()
    .provider(Provider::ClaudeCode)
    .claude_code(
        ClaudeCodeProvider::new()                       // uses $CLAUDE_CODE_PATH or "claude" in PATH
            .bare(true)                                 // --bare (daemon-safe: skip hooks/plugins/auto-memory)
            .model("sonnet")                            // --model
            .system_prompt("Be terse.")                 // --system-prompt (full replacement)
            .permission_mode(PermissionMode::Plan)      // --permission-mode plan
            .effort(EffortLevel::Low)                   // --effort low
            .fallback_model("opus")                     // --fallback-model
            .add_dir("/tmp/workspace")                  // --add-dir (repeatable)
            .allow_tool("Edit")                         // --allowed-tools (variadic)
            .allow_tool("Read")
            .disallow_tool("WebFetch")                  // --disallowed-tools (variadic)
            .mcp_config("./mcp.json")                   // --mcp-config (variadic)
            .strict_mcp_config(true)                    // --strict-mcp-config
            .settings("./settings.json")                // --settings
            .setting_source("user")                     // --setting-sources user,project
            .setting_source("project")
            .session_id("11111111-2222-3333-4444-555555555555") // --session-id
            .no_session_persistence(true)               // --no-session-persistence
            .max_budget_usd(2.5),                       // --max-budget-usd
    )
    .build()?;  // api_key not required for CLI backends

let response = client.chat(vec![Message::user("hi")]).await?;
let stream = client.stream(vec![Message::user("hi")]).await?;
```

**Option B — direct use of the provider**:

```rust
use motosan_ai::ClaudeCodeProvider;

let client = ClaudeCodeProvider::new()         // uses $CLAUDE_CODE_PATH or "claude" in PATH
    .model("sonnet")                           // forwards --model sonnet to the CLI
    .agent_mode(false);                        // set true to enable --dangerously-skip-permissions

let response = client.chat(request).await?;
let stream = client.stream(request).await?;
```

### Builder reference

All setters return `Self` for chaining. Omitted setters leave the corresponding flag off. Blank strings and non-finite budgets are dropped at argv-build time.

**Prompts**
- `.system_prompt(text)` — `--system-prompt` (full replacement). Coexists with the message-extracted system prompt, which flows through `--append-system-prompt`.

**Model & reasoning**
- `.model(name)` — `--model`. `""` / `"default"` (case-insensitive) are skipped so the CLI default applies.
- `.fallback_model(name)` — `--fallback-model`. Only meaningful under `--print`; triggers automatic fallback when the primary model is overloaded.
- `.effort(EffortLevel::{Low,Medium,High,Max})` — `--effort`.

**Permissions**
- `.agent_mode(bool)` — `--dangerously-skip-permissions`, also switches the blocking path to `--output-format json` so usage tokens can be parsed.
- `.permission_mode(PermissionMode::{AcceptEdits,Auto,BypassPermissions,Default,DontAsk,Plan})` — `--permission-mode`.

**Isolation**
- `.bare(bool)` — `--bare`. Spawned `claude` skips hooks, plugins, auto-memory, keychain reads, and user/project settings discovery, so the subprocess does not inherit the operator's interactive Claude Code state. Recommended `true` for daemon / server use; leave `false` for interactive workflows that should pick up `~/.claude/` configuration. Emitted before `--dangerously-skip-permissions` in argv.

**Workspace & tools**
- `.add_dir(path)` / `.add_dirs(vec)` — `--add-dir` (repeatable).
- `.allow_tool(name)` / `.allowed_tools(vec)` — `--allowed-tools` (variadic).
- `.disallow_tool(name)` / `.disallowed_tools(vec)` — `--disallowed-tools` (variadic).
- Blank entries are skipped.

**MCP**
- `.mcp_config(path_or_json)` / `.mcp_configs(vec)` — `--mcp-config` (variadic, accepts file paths or inline JSON strings).
- `.strict_mcp_config(bool)` — `--strict-mcp-config` (only use servers from `mcp_config`).

**Settings**
- `.settings(path_or_json)` — `--settings`.
- `.setting_source(source)` / `.setting_sources(vec)` — `--setting-sources`. Entries are joined with commas at argv time; blanks are filtered. Valid values: `user` / `project` / `local`.

**Session continuity**
- `.session_id(uuid)` — `--session-id`.
- `.resume(value)` — `--resume` (accepts `"latest"` or a specific session ID).
- `.continue_latest(bool)` — `--continue` (continue the most recent conversation in cwd).
- `.fork_session(bool)` — `--fork-session` (when resuming, create a new session ID).
- `.no_session_persistence(bool)` — `--no-session-persistence` (don't save to disk).

**Plugins & agents**
- `.plugin_dir(path)` / `.plugin_dirs(vec)` — `--plugin-dir` (repeatable).
- `.agent(name)` — `--agent`.

**Budget**
- `.max_budget_usd(amount)` — `--max-budget-usd`. Negative, `NaN`, and infinite values are silently dropped so the CLI never receives an invalid number.

Notes:
- Authentication: `claude` uses your existing local login state — motosan-ai does not pass any credentials through.
- Blocking `chat()` delegates to `stream()` + collect (since 0.25.0): `ChatResponse.tool_calls` records the tools the CLI already executed — never a request to execute — and a completed turn always reports `StopReason::EndTurn`.
- `stream()` surfaces CLI tool-use blocks as `ToolCallStart` → `ToolCallArgs` → `ToolCallEnd` events.
- Argv order is stable and locked by the `common_args_full_loadout_order_is_stable` unit test. Changing the order may break callers that grep spawned command lines for debugging.
- Live integration tests that actually spawn `claude` and verify each flag group are gated behind `#[ignore]` — run with `cargo test --features claude-code -- --ignored`.

## Codex CLI Backend

The `codex-cli` feature enables `CodexCliProvider`, which shells out to OpenAI's `codex exec --json` and parses the JSONL event stream.

```toml
motosan-ai = { version = "0.27.0", features = ["codex-cli"] }
```

**Option A — via `Client::builder()`** (since v0.11.0). Build the provider with all the codex-specific flags, then hand it to the `Client` setter:

```rust
use motosan_ai::codex_cli::{LocalProvider, SandboxMode};
use motosan_ai::{Client, CodexCliProvider, Provider};

let client = Client::builder()
    .provider(Provider::CodexCli)
    .codex_cli(
        CodexCliProvider::new()              // uses $CODEX_PATH or "codex" in PATH
            .model("gpt-5.1-codex")          // --model
            .sandbox(SandboxMode::WorkspaceWrite) // --sandbox workspace-write
            .profile("work")                 // --profile from ~/.codex/config.toml
            .cd("/tmp/project")              // --cd
            .add_dir("/tmp/output")          // --add-dir (repeatable)
            .ephemeral(true)                 // --ephemeral
            .enable_feature("fast_mode")     // --enable (repeatable, validated against `codex features list`)
            .disable_feature("image_generation") // --disable (repeatable)
            .config_override("model_reasoning_effort", "\"low\""),
    )
    .build()?;                                // api_key optional for CLI backends

let response = client.chat(vec![Message::user("hi")]).await?;
```

**Option B — direct use of the provider**:

```rust
use motosan_ai::{CodexCliProvider, codex_cli::{LocalProvider, SandboxMode}};

let client = CodexCliProvider::new()
    .sandbox(SandboxMode::WorkspaceWrite)
    .ephemeral(true);

// Run against a local OSS provider instead of the OpenAI cloud:
let local = CodexCliProvider::new()
    .oss(true)
    .local_provider(LocalProvider::Ollama);  // or LocalProvider::LmStudio

// Externally-sandboxed environments only — disables ALL approvals and the sandbox:
let unsafe_client = CodexCliProvider::new()
    .dangerously_bypass_approvals_and_sandbox(true);

let response = client.chat(request).await?;
let stream = client.stream(request).await?;
```

Notes:
- Codex emits **complete** `agent_message` items, not token deltas — `stream()` yields one text event per finalized message.
- Blocking `chat()` delegates to `stream()` + collect (since 0.25.0): `ChatResponse.tool_calls` records the Codex tool invocations the CLI already executed, and a completed turn always reports `StopReason::EndTurn`.
- `stream()` surfaces `command_execution` and `mcp_tool_call` items as `ToolCallStart` → `ToolCallArgs` → `ToolCallEnd` events.
- Authentication: Codex CLI uses `CODEX_API_KEY` or `~/.codex/auth.json`, not `OPENAI_API_KEY`.
- `agent_mode(true)` passes `--full-auto` (workspace-write sandbox + approvals off); can coexist with an explicit `sandbox()`.
- `dangerously_bypass_approvals_and_sandbox(true)` should ONLY be used inside an externally sandboxed environment (disposable container, ephemeral VM).

## Gemini CLI Backend

The `gemini-cli` feature enables `GeminiCliProvider`, which shells out to Google's `gemini -p "" -o stream-json` and parses the NDJSON event stream. Auth is handled by the `gemini` CLI itself (`gemini auth` once; personal Google account or API key) — motosan-ai does not pass any credentials through.

```toml
motosan-ai = { version = "0.27.0", features = ["gemini-cli"] }
```

**Option A — via `Client::builder()`**:

```rust
use motosan_ai::gemini_cli::ApprovalMode;
use motosan_ai::{Client, GeminiCliProvider, Provider};

let client = Client::builder()
    .provider(Provider::GeminiCli)
    .gemini_cli(
        GeminiCliProvider::new()                // uses $GEMINI_CLI_PATH or "gemini" in PATH
            .model("gemini-2.5-pro")            // -m
            .approval_mode(ApprovalMode::Yolo)  // --approval-mode yolo
            .sandbox(true),                     // --sandbox
    )
    .build()?;                                  // api_key optional for CLI backends

let response = client.chat(vec![Message::user("hi")]).await?;
```

**Option B — direct use of the provider**:

```rust
use motosan_ai::{GeminiCliProvider, gemini_cli::ApprovalMode};

let client = GeminiCliProvider::new()
    .model("gemini-2.5-flash")
    .yolo(true);                 // shorthand for --yolo

let response = client.chat(request).await?;
let stream = client.stream(request).await?;
```

Notes:
- **Argv layout**: `gemini -p "" -o stream-json [-m <model>] [--yolo] [--sandbox] [--approval-mode <mode>]`. The empty `-p` puts Gemini CLI in headless mode; the real prompt flows via stdin (Gemini appends stdin to the `-p` value per `--help`).
- **System prompts**: Gemini CLI has no `--system-prompt` flag, so motosan-ai merges system text into the stdin payload as a blank-line-separated prefix. This matches how the CLI treats `GEMINI.md` context.
- **Streaming**: Gemini emits delta chunks (`{"type":"message","role":"assistant","content":"...","delta":true}`) followed by a terminal `{"type":"result","stats":{...}}` that carries token usage. Both `chat()` and `stream()` use the same parser.
- **Usage**: populated from `result.stats.input_tokens` / `output_tokens` / `cached` (mapped to `cache_read_input_tokens`). Gemini CLI does not expose cache-creation tokens.
- **Tool calls**: blocking `chat()` delegates to `stream()` + collect (since 0.25.0), so `ChatResponse.tool_calls` records already-executed Gemini `tool_use` events; `stream()` surfaces them as `ToolCallStart` → `ToolCallArgs` → `ToolCallEnd`, a completed turn always reports `StopReason::EndTurn`, and Gemini `tool_result` events are ignored.
- **Model selection**: `-m` is forwarded when the model string is non-empty and not `"default"` (case-insensitive).

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
