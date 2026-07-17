# Shared Type Definitions

Canonical definitions shared across all language SDKs.

## Message

| Field | Type | Notes |
|-------|------|-------|
| `role` | `"user" \| "assistant" \| "system" \| "tool"` | |
| `content` | `string` | Plain-text fallback; always populated |
| `content_blocks` | `ContentBlock[]` | Multimodal content; empty for text-only messages |
| `tool_call_id` | `string?` | Required on `role: "tool"` messages |
| `tool_calls` | `ToolCall[]` | Set on assistant messages that request tool execution |

### Message Constructors (Rust)

```rust
Message::user("text")
Message::assistant("text")
Message::system("text")
Message::tool_result("call_id", "result JSON")
Message::assistant_with_tool_calls("text", tool_calls)
Message::user_with_image("text", "base64data", "image/png")   // image/jpeg/gif/webp
Message::user_with_blocks(vec![ContentBlock::Image { .. }, ContentBlock::Text { .. }])
Message::user_with_pdf_base64("text", "base64data")           // Anthropic only
```

## ContentBlock

```
ContentBlock::Text     { text: string }
ContentBlock::Image    { source: ImageSource }
ContentBlock::Document { source: DocumentSource }   // Anthropic only
```

## ImageSource

```
ImageSource::Base64 { media_type: string, data: string }   // "image/png" | "image/jpeg" | "image/gif" | "image/webp"
ImageSource::Url    { url: string }
```

Serialized as: Anthropic → `{type: "base64"/"url", ...}`, OpenAI → data URI / `{url}`, Gemini → `inlineData` / `fileData`.

## DocumentSource

```
DocumentSource::Base64 { media_type: string, data: string }   // "application/pdf"
DocumentSource::Url    { url: string }
```

## ProviderCapabilities (Rust, v0.13.1+)

| Field | Type | Notes |
|-------|------|-------|
| `supports_image` | `bool` | Provider accepts `ContentBlock::Image` |
| `supports_document` | `bool` | Provider accepts `ContentBlock::Document` |

Named constructors: `text_only()` / `with_image()` / `full()`.

Default per provider: Anthropic → `full()`, OpenAI/Gemini/GeminiCodeAssist → `with_image()`, all others → `text_only()`.
Passing unsupported content returns `Err(UnsupportedFeature)` before any network call.

## ChatRequest

| Field | Type | Required |
|-------|------|----------|
| `messages` | `Message[]` | ✅ |
| `model` | `string` | ❌ defaults per provider |
| `system` | `string` | ❌ SDK normalizes to provider format |
| `system_blocks` | `SystemBlock[]` | ❌ Anthropic prompt caching |
| `temperature` | `float` | ❌ |
| `max_tokens` | `int` | ❌ |
| `tools` | `Tool[]` | ❌ |
| `tool_choice` | `ToolChoice` | ❌ `auto \| required \| none \| {tool: name}` |
| `stop_sequences` | `string[]` | ❌ |
| `provider_options` | `object` | ❌ passthrough escape hatch |

## ChatResponse

| Field | Type |
|-------|------|
| `content` | `string` |
| `tool_calls` | `ToolCall[]` — always a list, never null |
| `model` | `string` |
| `usage` | `Usage` |
| `stop_reason` | `StopReason` |

## StopReason

`end_turn` | `max_tokens` | `tool_use` | `stop` | `other`

## ToolCall

| Field | Type |
|-------|------|
| `id` | `string` |
| `name` | `string` |
| `input` | `object` (parsed JSON) |

## Usage

| Field | Type |
|-------|------|
| `input_tokens` | `int` |
| `output_tokens` | `int` |
| `cache_creation_input_tokens` | `int?` |
| `cache_read_input_tokens` | `int?` |

## StreamEvent

| Field | Type | Notes |
|-------|------|-------|
| `content` | `string` | Text delta |
| `done` | `bool` | Exactly one terminal event per *successfully completed* stream — see [Stream termination contract](#stream-termination-contract) |
| `stop_reason` | `StopReason?` | Set on terminal event when provider reports one |
| `event_type` | `StreamEventType` | |
| `tool_call_id` | `string?` | |
| `tool_call_name` | `string?` | Set on `tool_call_start` |
| `tool_call_args_delta` | `string?` | Accumulate until `tool_call_end` |
| `usage` | `Usage?` | Set on `usage` events |

## StreamEventType

`text` | `tool_call_start` | `tool_call_args` | `tool_call_end` | `usage` | `thinking_delta` | `thinking_done`

Seven values, identical across the SDKs (Rust enum `StreamEventType`,
Python `StreamEventType` StrEnum, TypeScript `StreamEventType` string
union). There is **no** `done` event type: stream termination is
signalled by the `done: bool` **field** on `StreamEvent`, never by
`event_type` (terminal events carry the default `event_type`, `text`).
The set may grow additively as providers gain richer thinking wire
formats; consumers matching on `event_type` should keep a fallback arm.

### Thinking events

`thinking_delta` carries a partial extended-thinking delta in
`content`, emitted while the model reasons before its final answer.
`thinking_done` marks the end of a thinking block and carries the
**full concatenated thinking text** in `content`; it is preceded by
zero or more `thinking_delta` events for the same block and always
precedes the `text` events of the final answer. Collectors
(`collect_stream` / `_stream_collect` / `collectStream`) assemble
`ChatResponse.thinking` with the `thinking_done` payload taking
priority; concatenated `thinking_delta` content is the fallback for
providers that never emit `thinking_done`.

### Emitters

| SDK | Provider | `thinking_delta` | `thinking_done` |
|-----|----------|------------------|-----------------|
| Rust | Anthropic | ✅ | ✅ — emitted even for an empty thinking block |
| Rust | ChatGPT Codex | ✅ (reasoning + reasoning-summary deltas) | ❌ |
| Python | Anthropic | ✅ (0.18.0+) | ✅ (0.18.0+) — mirrors Rust, incl. empty blocks |
| Python | ChatGPT Codex | ✅ (0.18.0+) | ❌ |
| TypeScript | Anthropic | ✅ | ✅ — suppressed for an empty thinking block |
| TypeScript | ChatGPT Codex | ✅ (reasoning + reasoning-summary deltas) | ❌ |

No other provider emits thinking events. The empty-block divergence
(Rust/Python emit `thinking_done` with empty `content`; TypeScript
emits nothing) is documented reality, not a bug to fix.

**Python migration note (0.18.0, BREAKING).** Pre-0.18.0 the Python
Anthropic and ChatGPT Codex adapters emitted the **untyped string**
`event_type="thinking"` — not a `StreamEventType` member — and never
emitted `thinking_done`. 0.18.0 replaces `"thinking"` with
`thinking_delta` (both providers) and adds `thinking_done`
(Anthropic). Consumers matching `"thinking"` break and must migrate.
`StreamEvent.event_type` stays annotated `str` (StrEnum members are
`str`).

## Stream termination contract

Every provider defines a **terminal event** that marks the successful
end of a stream:

| Provider family | Terminal event |
|-----------------|----------------|
| OpenAI | `data: [DONE]` SSE sentinel, or a `finish_reason`-bearing chunk (either suffices; `finish_reason` is the semantic terminal, `[DONE]` the transport epilogue) |
| MiniMax | Python: `data: [DONE]` SSE sentinel, or a `finish_reason`-bearing chunk (either suffices, as for OpenAI — own OpenAI-compatible-wire adapter). Rust / TypeScript: `message_stop` — both delegate to the Anthropic adapter (Rust `build_minimax_provider` constructs an `AnthropicProvider`; TS `MinimaxProvider` wraps one), so the Anthropic rule applies |
| Anthropic | `message_stop` SSE event (the Python adapter additionally treats a stray `data: [DONE]` as terminal) |
| Gemini, GeminiCodeAssist | final SSE chunk carrying `finishReason` (a trailing `[DONE]` is tolerated but not required) |
| ChatGPT Codex | `response.completed` SSE event |
| Ollama | final NDJSON object with `"done": true` |

**Terminal-event rule.** Enforcement lives in the **stream adapters**,
not the collectors. When the upstream byte/event stream ends (EOF)
**without** the provider's terminal event, the adapter yields/throws
the `IncompleteStream` error below. Adapters MUST NOT fabricate a
synthetic `done` event and MUST NOT end the stream silently:
truncation is always distinguishable from completion. (On the OpenAI
wire the `done` event may be *emitted at EOF* when a `finish_reason`
chunk — a real terminal event per the table above — already arrived;
that is delivery of a received terminal, not fabrication.)

Collectors are unchanged: they keep propagating adapter errors (the
M1 fallible-stream contract) and keep the `stop_reason` heuristic
**only** for a real terminal event that lacks a reason — never as a
substitute for a missing terminal event.

### IncompleteStream error

| SDK | Spelling |
|-----|----------|
| Rust | `MotosanError::IncompleteStream(String)` — `#[error("incomplete stream: {0}")]`; new enum variant ⇒ breaking, ships 0.24.0 |
| Python | `class IncompleteStreamError(StreamError)` |
| TypeScript | `export class IncompleteStreamError extends StreamError` |

Message convention (all SDKs):
`incomplete stream: <provider> ended without a terminal event` — e.g.
`incomplete stream: openai ended without a terminal event`.

Python and TypeScript subclass `StreamError` deliberately, as a
migration softener: existing `except StreamError` /
`instanceof StreamError` handlers still catch truncation. Handlers
that must distinguish truncation match the subclass. Rust has no such
softener — the new enum variant is the breaking change.

### Retired invariant (v0.10.1)

The former guarantee that streams "emit exactly one terminal `done`
event **even when the upstream provider closes the connection
without** `[DONE]` **and without any** `finish_reason` **chunk**"
(introduced in the v0.10.1 era; implemented via the `done_emitted` /
`doneEmitted` EOF fabrication in the Rust and TypeScript OpenAI
adapters — which also served MiniMax before its v0.14 move to the
Anthropic wire — and equivalent defensive-EOF fabrication or
silent-end paths elsewhere, e.g. the TypeScript Anthropic adapter's
fallback `done` at EOF) is **deliberately retired**. Fabricating
`done` on a truncated EOF made truncation indistinguishable from
completion. What is retired is precisely the NEITHER-signal
fabrication: on the OpenAI wire an EOF after a `finish_reason`-bearing
chunk is a *semantically complete* stream (per the terminal-event
table above) and still emits `done` carrying the stashed stop reason —
that path is NOT retired. The narrower invariant that survives: a
stream that terminates *without error* emits exactly one terminal
`done` event.

### Cancellation

- **Rust** — drop-cancellation: dropping the stream (or the `chat()`
  future) drops the underlying `reqwest` response/future, which
  cancels the in-flight HTTP request and releases the connection.
  There is no explicit cancel API; this is documented behavior, not a
  code change.
- **TypeScript** — per-request `AbortSignal`: aborting a
  caller-supplied signal cancels the underlying `fetch` and surfaces
  `CancelledError extends MotosanError`, which is never retried — see
  [`retry.md`](./retry.md#classification).
- **Python** — standard `asyncio` task cancellation: cancelling the
  task awaiting `chat()` / iterating `stream()` raises
  `asyncio.CancelledError` through the SDK, and `httpx` closes the
  underlying connection. The SDK neither swallows nor converts
  `CancelledError`.

## CLI backend chat/stream contract

Applies to the six CLI-spawning backends: Rust
`sdks/rust/src/providers/claude_code/`, `codex_cli/`, `gemini_cli/`
and Python `sdks/python/motosan_ai/providers/claude_code.py`,
`codex_cli.py`, `gemini_cli.py`. TypeScript has no CLI backends.
Normative from Rust 0.25.0 / Python 0.18.0 (BREAKING — see CHANGELOG).

- **`stop_reason` is always `end_turn`.** A successfully completed CLI
  turn reports `stop_reason = end_turn` on **both** the `chat()` and
  the `stream()` path. CLI backends never report `tool_use`: their
  tools are executed internally by the CLI process, and `tool_use`
  means "the caller must execute tools" — something a CLI backend
  never requests. (The pre-0.25.0 / pre-0.18.0 behavior — `tool_use`
  whenever the transcript contained a tool call — made agent loops
  re-execute already-executed tools.)
- **`ChatResponse.tool_calls` is a record, not a request.** For a CLI
  backend it lists the tools the CLI already executed during the turn;
  callers MUST NOT execute them.
- **`chat()` ≡ collect(`stream()`).** Every CLI backend implements
  `chat()` by collecting its own `stream()` (Rust `collect_stream`,
  Python `_stream_collect`), so `content` / `thinking` / `tool_calls`
  / `stop_reason` / `usage` / `session_id` parity holds by
  construction. The single documented parity exception: `chat()` may
  backfill `ChatResponse.model` from provider config when the
  collected value is empty.
- CLI backends perform no transport-level retry — see
  [`retry.md` § CLI backends](./retry.md#cli-backends).

## Token sources (ChatGPT Codex)

The ChatGPT Codex provider authenticates with a short-lived OAuth
bearer token. Each SDK exposes a **token source** seam so long-running
processes can supply a fresh token without rebuilding the client; the
token is resolved **once per retry attempt** — a retried request never
reuses a token fetched for an earlier attempt. Introduced in Rust
0.25.0 / Python 0.18.0 / TypeScript 0.15.0.

| SDK | Seam |
|-----|------|
| Rust | `pub trait TokenSource` in ungated `src/auth.rs` — `async fn access_token(&self) -> Result<String, MotosanError>` — plus `StaticTokenSource(String)` for fixed tokens. `ChatGptCodexProvider` stores `Arc<dyn TokenSource>`; `new()` keeps its `access_token: String` signature (wraps `StaticTokenSource`); `with_token_source` and `ClientBuilder::chatgpt_codex_token_source(Arc<dyn TokenSource>)` inject a dynamic source. The per-attempt fetch runs inside the shared retry engine via its async-build variant (see [`retry.md` § One retry engine per SDK](./retry.md#one-retry-engine-per-sdk)) |
| Python | `token_source: Callable[[], Awaitable[str]] \| None = None` on `ChatGptCodexProvider` and `Client.chatgpt_codex()`; at least one of `access_token` / `token_source` is required (`ConfigError` when neither is given; when both are set, `token_source` wins — matching Rust); when a source is set, the bearer token is resolved at the top of every retry attempt |
| TypeScript | constructor `accessToken: string \| (() => Promise<string>)`; a function value is awaited once per attempt |

- The SDKs never depend on the OAuth crates
  (`sdks/rust/crates/anthropic-oauth`, `codex-oauth`,
  `motosan-ai-oauth`): a refreshing token source is caller-supplied
  glue built on top of them.
- Token material MUST NOT appear in `Debug` / `repr` / log output;
  the Rust provider implements a custom `Debug` that redacts it.

## MotosanError (Rust)

`Auth` | `RateLimit` | `InvalidRequest` | `Config` | `ProviderError` | `Network` | `Stream` | `StreamReadTimeout(u64)` | `IncompleteStream(String)` | `UnsupportedFeature(String)`

Retry classification, backoff, and `Retry-After` handling are specified in [`retry.md`](./retry.md).

## Default Models

| Provider | Default |
|----------|---------|
| Anthropic | `claude-sonnet-4-6` |
| OpenAI | `gpt-5.3-codex` |
| MiniMax | `MiniMax-M2.7` |
| Ollama | `llama3.2` |
| Gemini | `gemini-2.0-flash` |
| GeminiCodeAssist | `gemini-2.5-flash` |
