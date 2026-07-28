---
name: motosan-ai
description: Help developers use the motosan-ai SDK (Python and Rust) and the codex-oauth crate — LLM chat, streaming, tool use, ThinkStripper, multi-provider setup, Gemini HTTP/Code Assist providers, and Codex OAuth login. Use when code imports motosan_ai or codex_oauth, or user asks how to integrate Anthropic/OpenAI/Ollama/MiniMax/Gemini via motosan-ai, implement streaming, handle tool calls, filter <think> tags, or get an OpenAI Codex access token.
---

# motosan-ai SDK

Multi-provider LLM SDK — Python 0.20.0 / Rust 0.27.1 / TypeScript 0.16.0

Providers: Anthropic, OpenAI (+ OpenAI-compatible: Groq, DeepSeek, Together, self-hosted proxies), MiniMax, Ollama, Gemini, Gemini Code Assist, Claude Code CLI, Codex CLI, Gemini CLI, ChatGPT Codex (Responses API)

Python 0.13.0 adds CLI-runtime setters (`.cwd()`, session continuity via `session_id` + `resume()`, per-run `.env()/.envs()`, CLI tool-call stream events, configurable `.timeout()/.no_timeout()`) and a **breaking** fallible stream: HTTP provider `stream()` now raises `motosan_ai.error.StreamError` mid-stream instead of swallowing transport/parse faults (`collect_stream` propagates it; `Client.stream_with` does not retry after a mid-stream raise).

## Install

```bash
# Python
pip install "motosan-ai[anthropic]"          # single provider
pip install "motosan-ai[gemini]"             # Gemini HTTP provider
pip install "motosan-ai[anthropic,openai,gemini]"   # multiple providers
```

```bash
# Rust
cargo add motosan-ai --features anthropic
# features: anthropic | openai | minimax | ollama | ollama_native (alias: ollama-native) | full
#           gemini | gemini-code-assist | chatgpt-codex
# CLI backends (shell out to a local binary): claude-code | codex-cli | gemini-cli

# Codex OAuth (standalone — get a token for chatgpt.com/backend-api)
cargo add codex-oauth
```

```bash
# TypeScript / Node (ESM, Node >= 20.3)
npm install @motosan-ai/sdk
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

Anthropic catalog includes `claude-opus-4-8` as an override. For Opus 4.8/4.7/4.6, `thinking` uses Anthropic adaptive thinking (`thinking.type = "adaptive"`, summarized display, `output_config.effort = "high"`) and OAuth adaptive-thinking requests omit the legacy `interleaved-thinking` beta header, matching pi.

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

- **Python Client API parity (v0.10.0)**: `chat_with(request)` and `stream_with(request)` are the canonical full-`ChatRequest` paths. Use them with `ChatRequest.builder()` for `thinking`, `tool_choice`, `mcp_servers`, `system_blocks`, and `stop_sequences`. `stream_collect(messages, **kwargs)` and `stream_collect_with(request)` drive a stream to completion and return a `ChatResponse`. `chat_sync()` is deprecated; recommend `asyncio.run(client.chat(...))`.
- **`BoxStream` (Rust 0.20+)**: `Pin<Box<dyn Stream<Item = Result<StreamEvent, MotosanError>> + Send>>` — unwrap each item with `let event = item?`; mid-stream provider/timeout errors surface as `Err(...)`
- **Stream termination contract** (Rust 0.24.0 / Python 0.17.0 / TypeScript 0.14.0; replaces the v0.10.1 fabricated-`done` invariant): a stream is complete only when the provider sends its terminal event (OpenAI `[DONE]` or a final `finish_reason` chunk — either suffices, Anthropic `message_stop`, Gemini/codex terminal frames). EOF without any such event errors with Rust `MotosanError::IncompleteStream` / Python `IncompleteStreamError` (subclass of `StreamError`) / TypeScript `IncompleteStreamError extends StreamError` — message `"incomplete stream: <provider> ended without a terminal event"`. Successful streams still end with one terminal `done == true` event carrying `stop_reason` when reported; `collect_stream` keeps the tool-calls heuristic only for a real terminal event that lacks a reason.
- **`ChatRequest`**: Use builder pattern in Rust (`ChatRequest::builder().messages(...).build()`), dataclass in Python
- **Rust native Freeform/custom tools (0.26.0)**: Keep legacy `ChatRequest` / `Tool` / `ToolCall` for function tools. Use `ModelChatRequest`, `ModelContextItem`, `ModelToolSpec`, `FreeformTool`, `ModelToolCall`, `ModelToolOutput`, `ModelChatResponse`, `ModelStreamDelta`, `BoxModelStream`, and `collect_model_stream` for ordered Function + Freeform/custom transport. OpenAI requires `ClientBuilder::openai_responses_api(true)` / `OpenAIProvider::with_responses_api(true)`; ChatGPT Codex supports it by default. Freeform `input` is raw text and must remain byte-for-byte intact — never parse it as JSON or lower it into function `arguments`. Unsupported providers return `UnsupportedFeature` before HTTP.
- **ThinkStripper**: Applied automatically in all `stream()` / `stream_with()` calls — no manual setup needed
- **Anthropic OAuth**: Auto-detected by token prefix (`sk-ant-oat01*`), `chat()` auto-redirects to `stream()` for OAuth tokens. Budget-based thinking sends `display: "summarized"`; Opus 4.8/4.7/4.6 thinking uses adaptive mode and skips the legacy interleaved-thinking beta.
- **Retry**: Enabled by default (3 retries, exponential backoff, jitter) for 429/5xx/timeout
- **`ProviderCapabilities`**: Rust `ProviderImpl` and Python `BaseProvider` expose `capabilities` / `validate_request()` guardrails. Providers that support images/documents/native Freeform declare capabilities; clients/providers validate before HTTP. Capability table: Anthropic → `full()` (image + doc), OpenAI Chat Completions/Gemini/GeminiCodeAssist → `with_image()`, OpenAI Responses → `with_image_and_freeform_tools()`, ChatGPT Codex → `with_freeform_tools()`, all others → `text_only()`.
- **Gemini HTTP providers**: `GeminiProvider` is available in Rust (feature `gemini`) and Python (`Client.gemini()`, `Provider.gemini`, `GEMINI_API_KEY`) for `generativelanguage.googleapis.com`, API key auth, pay-per-token. Python default model is `gemini-2.5-flash`. `GeminiCodeAssistProvider` is available in Rust (feature `gemini-code-assist`) and Python v0.10.0 (`Client.gemini_code_assist()`, `Provider.gemini_code_assist`) for `cloudcode-pa.googleapis.com/v1internal`, OAuth Bearer token (`ya29.*`), requires GCP project ID, subscription billing. Python includes `motosan_ai.oauth` PKCE helpers and a 0600 token cache. **Critical**: For `GeminiProvider`, `Message.tool_result` / `Message::tool_result` must use the function name (not opaque call ID) as `tool_call_id` — Gemini API requires `functionResponse.name` = function name.
- **ChatGPT Codex provider** (Python v0.14.0): `Client.chatgpt_codex(access_token, account_id, model)` / `Provider.openai_chatgpt` / `ChatGptCodexProvider` — native inference via the OpenAI Responses API at `chatgpt.com/backend-api/codex/responses` with a pre-obtained ChatGPT OAuth bearer token + `chatgpt-account-id` + codex CLI headers. No `api_key`. Text-only, default model `gpt-5.5`, `chat()` = `stream()` + collect. Reasoning effort via per-request `provider_options["reasoning_effort"]` or a provider-level default (`ChatGptCodexProvider.reasoning_effort(...)`). Mirrors the Rust `ChatGptCodexProvider`. **TypeScript v0.11.0** ships the same provider: `Client.builder().chatgptCodex(accessToken, accountId, model?, { reasoningEffort? })` / `Provider` variant `'chatgpt_codex'` / `ChatGptCodexProvider` — same wire/behavior; per-request `providerOptions.reasoning_effort` (string) overrides the provider default; mid-stream `error`/`response.failed` frames terminate the stream silently (TS convention, not a throw).
- **CLI backends**: Rust has `ClaudeCodeProvider` (feature `claude-code`, shells out to `claude`), `CodexCliProvider` (feature `codex-cli`, shells out to `codex exec --json`), and `GeminiCliProvider` (feature `gemini-cli`, shells out to `gemini -p "" -o stream-json`). Python v0.9.2+ has built-in `ClaudeCodeClient`, `CodexCliClient`, and `GeminiCliClient` with Rust-compatible flag coverage. As of Rust 0.25.0 / Python 0.18.0, blocking `chat()` on all three delegates to `stream()` + collect — `chat().tool_calls` records the tools the CLI already executed (never a request to execute; a completed CLI turn always reports `stop_reason = end_turn`, never `tool_use`) — and `stream()` surfaces CLI tool use as `ToolCallStart → ToolCallArgs → ToolCallEnd` events (Claude `tool_use`, Codex `command_execution`/`mcp_tool_call` named `server/tool`, Gemini `tool_use`). `CodexCliProvider.chat()` content is the concatenated stream text and no longer applies the old preamble→thinking heuristic; Python `CodexCliClient` surfaces agent messages as content and maps `turn.completed.usage.cached_input_tokens` to `Usage.cache_read_input_tokens`. `GeminiCliProvider` / Python `GeminiCliClient` merge the system prompt into stdin because Gemini CLI has no `--system-prompt` flag, use no trailing `-` marker, and map `result.stats.cached` to `Usage.cache_read_input_tokens`. Claude Code covers the full SDK-relevant flag surface: `.bare` (daemon-safe `--bare`; skips hooks/plugins/auto-memory/keychain/user+project settings) / `.model` / `.system_prompt` (`--system-prompt`, while request system text uses `--append-system-prompt`) / `.permission_mode` / `.effort` / `.fallback_model` / `.add_dir` / variadic `.allow_tool` / `.disallow_tool` / `.mcp_config` / `.strict_mcp_config` / `.settings` / `.setting_source` / `.session_id` / `.resume` / `.continue_latest` / `.fork_session` / `.no_session_persistence` / `.plugin_dir` / `.agent` / `.max_budget_usd` (`--max-budget-usd`). **All three CLI providers also share these knobs (Rust v0.20.0):** `.cwd(dir)` (spawns the child with `Command::current_dir`; Codex uses `.cd()` → `--cd`); `.env(k,v)` / `.envs(iter)` (per-run secret injection into the child, redacted from `Debug`); `.timeout(dur)` / `.no_timeout()` (per-invocation deadline, default Claude 300 s / Codex·Gemini 600 s, plus a per-line stream read-stall deadline → `Err(StreamReadTimeout)`); and `.resume(id)` session continuity (Codex `exec resume`, Gemini/Claude `--resume`; the provider-minted id is surfaced on `StreamEvent::session_id` / `ChatResponse::session_id`). Python and Rust streams emit a `usage` event before terminal `done` when Claude Code, Codex, or Gemini CLI reports usage.
- **Unified `Client::builder()` dispatch** (Rust, since v0.11.0): `Provider::ClaudeCode`, `Provider::CodexCli`, `Provider::GeminiCli`, `Provider::Gemini`, and `Provider::GeminiCodeAssist` are all first-class `Provider` variants. CLI backends are reachable through `Client::builder().provider(Provider::GeminiCli).gemini_cli(GeminiCliProvider::new().model("gemini-2.5-pro")).build()?` — no `api_key` required for CLI paths. Downstream consumers can hold a single `Client` and dispatch to any backend through `chat()` / `stream()` without provider-specific branching. The v0.10.0 `ClaudeCodeClient` / `CodexCliClient` type aliases were removed in v0.11.0.
- **OpenAI-compatible endpoints** (Rust): `OpenAIProvider` takes **full URLs** via `.with_chat_url(url)` / `.with_responses_url(url)` (or `.openai_chat_url(url)` on `ClientBuilder`). No `/v1` auto-injection, no `base_url` heuristics — what you pass is what gets POSTed. Works for Groq (`https://api.groq.com/openai/v1/chat/completions`), DeepSeek, Together, self-hosted proxies, etc. Defaults to `https://api.openai.com/v1/chat/completions`.
