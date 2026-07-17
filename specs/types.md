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

`text` | `tool_call_start` | `tool_call_args` | `tool_call_end` | `usage` | `done`

## Stream termination contract

Every provider defines a **terminal event** that marks the successful
end of a stream:

| Provider family | Terminal event |
|-----------------|----------------|
| OpenAI | `data: [DONE]` SSE sentinel |
| MiniMax | Python: `data: [DONE]` SSE sentinel (own OpenAI-compatible-wire adapter). Rust / TypeScript: `message_stop` — both delegate to the Anthropic adapter (Rust `build_minimax_provider` constructs an `AnthropicProvider`; TS `MinimaxProvider` wraps one), so the Anthropic rule applies |
| Anthropic | `message_stop` SSE event (the Python adapter additionally treats a stray `data: [DONE]` as terminal) |
| Gemini, GeminiCodeAssist | final SSE chunk carrying `finishReason` (a trailing `[DONE]` is tolerated but not required) |
| ChatGPT Codex | `response.completed` SSE event |
| Ollama | final NDJSON object with `"done": true` |

**Terminal-event rule.** Enforcement lives in the **stream adapters**,
not the collectors. When the upstream byte/event stream ends (EOF)
**without** the provider's terminal event, the adapter yields/throws
the `IncompleteStream` error below. Adapters MUST NOT fabricate a
synthetic `done` event and MUST NOT end the stream silently:
truncation is always distinguishable from completion.

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
completion. The narrower invariant that survives: a stream that
terminates *without error* emits exactly one terminal `done` event.

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
