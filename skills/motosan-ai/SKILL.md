---
name: motosan-ai
description: Help developers use the motosan-ai SDK (Python and Rust) and the codex-oauth crate — LLM chat, streaming, tool use, ThinkStripper, multi-provider setup, Gemini HTTP/Code Assist providers, and Codex OAuth login. Use when code imports motosan_ai or codex_oauth, or user asks how to integrate Anthropic/OpenAI/Ollama/MiniMax/Gemini via motosan-ai, implement streaming, handle tool calls, filter <think> tags, or get an OpenAI Codex access token.
---

# motosan-ai SDK

Multi-provider LLM SDK — Python 0.8.2 / Rust 0.14.0

Providers: Anthropic, OpenAI (+ OpenAI-compatible: Groq, DeepSeek, Together, self-hosted proxies), MiniMax, Ollama, Gemini, Gemini Code Assist

## Install

```bash
# Python
pip install "motosan-ai[anthropic]"          # single provider
pip install "motosan-ai[gemini]"             # Gemini HTTP provider
pip install "motosan-ai[anthropic,openai,gemini]"   # multiple providers
```

```toml
# Rust (Cargo.toml)
motosan-ai = { version = "0.14.0", features = ["anthropic"] }
# features: anthropic | openai | minimax | ollama | ollama_native | full
#           gemini | gemini-code-assist
# CLI backends (shell out to a local binary): claude-code | codex-cli | gemini-cli

# Codex OAuth (standalone — get a token for chatgpt.com/backend-api)
codex-oauth = "0.1"
```

## Environment Variables

| Provider  | Env var             |
|-----------|---------------------|
| Anthropic | `ANTHROPIC_API_KEY` |
| OpenAI    | `OPENAI_API_KEY`    |
| MiniMax   | `MINIMAX_API_KEY`   |
| Gemini    | `GEMINI_API_KEY`    |
| Ollama    | (none — local)      |

## Model Defaults

| Provider  | Default model             |
|-----------|---------------------------|
| Anthropic | `claude-sonnet-4-6`       |
| OpenAI    | Python: `gpt-4o` · Rust: `gpt-5.3-codex` |
| MiniMax   | Python: `MiniMax-Text-01` · Rust: `MiniMax-M2.7` |
| Ollama    | `llama3.2`               |
| Gemini    | `gemini-2.5-flash`       |
| Gemini Code Assist | `gemini-2.5-flash` |

## Minimal Example

**Python:**
```python
from motosan_ai import Client, Message

client = Client.anthropic()                    # reads ANTHROPIC_API_KEY
resp = await client.chat([Message.user("Hi")]) # returns ChatResponse
print(resp.content)                            # str
```

**Rust (HTTP provider):**
```rust
use motosan_ai::{Client, Provider, Message};
let client = Client::builder()
    .provider(Provider::Anthropic)
    .api_key(std::env::var("ANTHROPIC_API_KEY")?)
    .build()?;
let resp = client.chat(vec![Message::user("Hi")]).await?;
println!("{}", resp.content);
```

**Rust (CLI backend — same `Client` API, since v0.11.0):**
```rust
use motosan_ai::codex_cli::SandboxMode;
use motosan_ai::{Client, CodexCliProvider, Message, Provider};

let client = Client::builder()
    .provider(Provider::CodexCli)
    .codex_cli(
        CodexCliProvider::new()
            .sandbox(SandboxMode::WorkspaceWrite)
            .ephemeral(true),
    )
    .build()?;  // no api_key needed for CLI backends
let resp = client.chat(vec![Message::user("Hi")]).await?;
```

## codex-oauth (Rust, standalone crate)

Browser-based PKCE OAuth login for OpenAI Codex. Returns an access token for `https://chatgpt.com/backend-api`.

```rust
// Login — opens browser, listens on localhost:1455, times out 120s
let token = codex_oauth::login().await?;

// Refresh
let token = codex_oauth::refresh(&token.refresh_token).await?;

// Expiry
if token.is_expired() { /* refresh */ }
```

`Token` implements `Serialize`/`Deserialize`. Use `token.access_token` as the Bearer token.

## When to Read References

| Task | File |
|------|------|
| Full Python API (`Client` factories, `chat_with`, `stream`, `ChatRequest`, `Message` helpers, `RetryPolicy`, errors) | `references/python-api.md` |
| Full Rust API (`ClientBuilder`, `chat_with`, `stream_with`, `BoxStream`, feature flags, `MotosanError`) | `references/rust-api.md` |
| Tool calling, multi-turn tool loop, ToolCall fields | `references/tool-use.md` |
| Streaming events, ThinkStripper, provider-specific streaming notes | `references/streaming.md` |
| Release process, version bump, tag convention, CI publish, CHANGELOG format | `references/release.md` |

## Key Design Decisions

- **`BoxStream` (Rust)**: `Pin<Box<dyn Stream<Item = StreamEvent> + Send>>` — items are `StreamEvent` directly, NOT `Result<StreamEvent>`
- **Stream `done` invariant** (Rust, since v0.10.1): every provider stream emits **exactly one** terminal event with `done == true`, even when the upstream provider closes without `[DONE]` and without any `finish_reason` chunk. Callers can rely on `if event.done { break; }` to terminate cleanly. The terminal event carries `stop_reason: Option<StopReason>` when the provider reports one (Anthropic/MiniMax `message_delta.stop_reason`, OpenAI `choices[0].finish_reason`); `None` otherwise. `collect_stream` honors the explicit reason and only falls back to a tool-calls heuristic when none was reported.
- **`ChatRequest`**: Use builder pattern in Rust (`ChatRequest::builder().messages(...).build()`), dataclass in Python
- **ThinkStripper**: Applied automatically in all `stream()` / `stream_with()` calls — no manual setup needed
- **Anthropic OAuth**: Auto-detected by token prefix (`sk-ant-oat01*`), `chat()` auto-redirects to `stream()` for OAuth tokens
- **Retry**: Enabled by default (3 retries, exponential backoff, jitter) for 429/5xx/timeout
- **`ProviderCapabilities`**: Rust `ProviderImpl` and Python `BaseProvider` expose `capabilities` / `validate_request()` guardrails. Providers that support images/documents declare capabilities; clients/providers validate before HTTP. Capability table: Anthropic → `full()` (image + doc), OpenAI/Gemini/GeminiCodeAssist → `with_image()`, all others → `text_only()`.
- **Gemini HTTP providers**: `GeminiProvider` is available in Rust (feature `gemini`) and Python (`Client.gemini()`, `Provider.gemini`, `GEMINI_API_KEY`) for `generativelanguage.googleapis.com`, API key auth, pay-per-token. Python default model is `gemini-2.5-flash`. `GeminiCodeAssistProvider` remains Rust-only (feature `gemini-code-assist`) for `cloudcode-pa.googleapis.com/v1internal`, OAuth Bearer token (`ya29.*`), requires GCP project ID via `.gemini_code_assist_project_id(...)`, subscription billing. **Critical**: For `GeminiProvider`, `Message.tool_result` / `Message::tool_result` must use the function name (not opaque call ID) as `tool_call_id` — Gemini API requires `functionResponse.name` = function name.
- **CLI backends** (Rust only): `ClaudeCodeProvider` (feature `claude-code`, shells out to `claude`), `CodexCliProvider` (feature `codex-cli`, shells out to `codex exec --json`), and `GeminiCliProvider` (feature `gemini-cli`, shells out to `gemini -p "" -o stream-json`). Live in `providers/{claude_code,codex_cli,gemini_cli}/` alongside HTTP providers. All three implement `ProviderImpl`. All three report empty `tool_calls` — tools run inside the CLI. `CodexCliProvider.chat()` splits multi-message turns into `content` (last `agent_message`) + `thinking` (preamble). `GeminiCliProvider` merges the system prompt into the stdin payload because Gemini CLI has no `--system-prompt` flag. `ClaudeCodeProvider` (since v0.12.0) covers the full SDK-relevant flag surface: `.bare` (daemon-safe `--bare`; skips hooks/plugins/auto-memory/keychain/user+project settings) / `.model` / `.system_prompt` / `.permission_mode(PermissionMode::*)` / `.effort(EffortLevel::*)` / `.fallback_model` / `.add_dir` / `.allow_tool` / `.disallow_tool` / `.mcp_config` / `.strict_mcp_config` / `.settings` / `.setting_source` / `.session_id` / `.resume` / `.continue_latest` / `.fork_session` / `.no_session_persistence` / `.plugin_dir` / `.agent` / `.max_budget_usd`.
- **Unified `Client::builder()` dispatch** (Rust, since v0.11.0): `Provider::ClaudeCode`, `Provider::CodexCli`, `Provider::GeminiCli`, `Provider::Gemini`, and `Provider::GeminiCodeAssist` are all first-class `Provider` variants. CLI backends are reachable through `Client::builder().provider(Provider::GeminiCli).gemini_cli(GeminiCliProvider::new().model("gemini-2.5-pro")).build()?` — no `api_key` required for CLI paths. Downstream consumers can hold a single `Client` and dispatch to any backend through `chat()` / `stream()` without provider-specific branching. The v0.10.0 `ClaudeCodeClient` / `CodexCliClient` type aliases were removed in v0.11.0.
- **OpenAI-compatible endpoints** (Rust): `OpenAIProvider` takes **full URLs** via `.with_chat_url(url)` / `.with_responses_url(url)` (or `.openai_chat_url(url)` on `ClientBuilder`). No `/v1` auto-injection, no `base_url` heuristics — what you pass is what gets POSTed. Works for Groq (`https://api.groq.com/openai/v1/chat/completions`), DeepSeek, Together, self-hosted proxies, etc. Defaults to `https://api.openai.com/v1/chat/completions`.
