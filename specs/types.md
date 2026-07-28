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
| `supports_freeform_tools` | `bool` | Provider accepts native `ModelToolSpec::Freeform` / `ModelToolCall::Freeform` transport (Rust 0.26.0+, Python 0.20.0+, TypeScript 0.16.0+) |

Named constructors: `text_only()` / `with_image()` / `with_freeform_tools()` /
`with_image_and_freeform_tools()` / `full()`.

Default per provider: Anthropic → `full()` (image + document),
OpenAI Chat Completions/Gemini/GeminiCodeAssist → `with_image()`, OpenAI
Responses → `with_image_and_freeform_tools()`, ChatGPT Codex →
`with_freeform_tools()`, all others → `text_only()`. Passing unsupported
content or native Freeform tools returns `Err(UnsupportedFeature)` before any
network call.

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

## Native Model API

> Implemented in Rust 0.26.0+, Python 0.20.0+, and TypeScript 0.16.0+.

The legacy cross-SDK `ChatRequest`, `Tool`, `ToolCall`, `ChatResponse`, and
`StreamEvent` APIs remain function-tool-only. Every SDK MUST expose a parallel
native model API for providers that speak OpenAI Responses-style ordered input
items and custom tool calls. The type shapes below are normative; each SDK
models them in its own idiom (Rust enums, Python variant dataclasses behind a
union alias, TypeScript discriminated unions), and each SDK's wire encoding
MUST live outside its type module — Rust `providers/responses.rs`, Python
`providers/responses.py`, TypeScript `serialize/responses.ts`.

Wire keys deliberately differ from field names and MUST be honoured exactly:
`ModelToolCall.id` ↔ wire `call_id` (deserialization accepts `call_id` **or**
`id`); `ModelChatRequest.max_tokens` ↔ body `max_output_tokens`;
`Tool.input_schema` ↔ tool-spec `parameters`. When building a request body,
system messages carried inside `context` MUST be hoisted into `instructions`
**and removed from `input`**, and `provider_options` MUST be shallow-merged
**last**, so a caller can override anything the encoder produced.

### Tool definitions

```
ModelToolSpec::Function(Tool)              // wire type: "function"
ModelToolSpec::Freeform(FreeformTool)      // wire type: "custom"

FreeformTool {
  name: string,
  description: string,
  format: FreeformToolFormat {
    type: string,
    syntax: string,
    definition: string,
  },
}
```

Freeform `format` is mandatory. Function tools serialize as Responses
`{type:"function", name, description, parameters}`. Freeform tools serialize as
`{type:"custom", name, description, format}`.

### Ordered context and history

```
ModelContextItem::Message(Message)
ModelContextItem::ToolCall(ModelToolCall)
ModelContextItem::ToolOutput(ModelToolOutput)
```

`ModelChatRequest.context` preserves mixed message / model tool-call / caller
tool-output order for subsequent requests. This is required for byte-exact
replay of Freeform inputs in multi-turn histories.

### Calls and outputs

```
ModelToolCall::Function { id, name, arguments }   // wire type: "function_call"
ModelToolCall::Freeform { id, name, input }       // wire type: "custom_tool_call"

ModelToolOutput::Function { call_id, output }     // wire type: "function_call_output"
ModelToolOutput::Custom { call_id, name, output } // wire type: "custom_tool_call_output"
```

Function `arguments` and Freeform `input` are both strings on the native Rust
surface, but they are different transports. Freeform `input` is raw model text
such as JavaScript and MUST be preserved byte-for-byte; providers MUST NOT parse
it as JSON and MUST NOT lower it into OpenAI function-call `arguments`.

`FunctionCallOutputPayload` supports either plain text or Responses-style
content items:

```
Text(string)
Content([InputText | InputImage | EncryptedContent])
```

### Fields the native request does not carry

`ModelChatRequest` MUST NOT accept extended thinking or MCP configuration.
Rust models `thinking`, `mcp_servers`, and `mcp_tool_configs` as fields that
exist only so validation can reject them; Python and TypeScript omit the
fields entirely, so a caller who reaches for one gets an attribute or type
error instead of a runtime rejection. Both spellings satisfy this contract.
Provider-specific reasoning controls travel through `provider_options`.

### Rejection error type

Rejections that happen before any network I/O — an unsupported native
Freeform spec or history, unsupported image or document content, a provider
with no native path — surface as:

| SDK | Spelling |
|-----|----------|
| Rust | `MotosanError::UnsupportedFeature(String)` |
| Python | `class UnsupportedFeatureError(InvalidRequestError)` |
| TypeScript | `export class UnsupportedFeatureError extends MotosanError` |

Python subclasses `InvalidRequestError` deliberately, as a migration softener:
existing `except InvalidRequestError` handlers keep working while callers that
must distinguish match the subclass. It stays non-retryable by inheritance.

### Requests, responses, and streams

```
Client::model_chat_with(ModelChatRequest) -> ModelChatResponse
Client::model_stream_with(ModelChatRequest) -> BoxModelStream
Client::model_stream_collect_with(ModelChatRequest) -> ModelChatResponse

ModelChatResponse {
  content: string,
  thinking: string?,
  tool_calls: ModelToolCall[],
  model: string,
  usage: Usage,
  stop_reason: StopReason,
  session_id: string?,
}

ModelStreamDelta =
  Text { delta }
  | ThinkingDelta { delta }
  | ThinkingDone { thinking }
  | FunctionArguments { call_id, delta }
  | FreeformInput { call_id, delta }
  | ToolCallDone { call }
  | Usage { usage }
  | Done { stop_reason }
```

`ToolCallDone` is authoritative for completed custom calls. Collectors may
accumulate `FreeformInput` deltas for display, but must preserve the completed
`ModelToolCall::Freeform.input` when the provider sends it.

### Provider support

ChatGPT Codex supports native Freeform transport through its Responses
endpoint by default. OpenAI supports the native API **only** when the caller
opts in — Rust `ClientBuilder::openai_responses_api(true)` /
`OpenAIProvider::with_responses_api(true)`, Python
`Client(openai_responses_api=True)` / `Client.openai(openai_responses_api=True)`
/ `OpenAIProvider(responses_api=True)`, TypeScript
`ClientBuilder.openaiResponsesApi(true)` /
`OpenAIProvider.withResponsesApi(true)`. TypeScript's pre-existing
`withResponsesFallback` is a 404 recovery path and is **not** the native
opt-in; the two MUST stay distinguishable.

The ChatGPT Codex body overrides the caller: `store=false`,
`include=["reasoning.encrypted_content"]`, `parallel_tool_calls=true`, and
`tool_choice="auto"` **regardless of what the caller passed**. When a
reasoning effort resolves — per-request `provider_options["reasoning_effort"]`
first, provider default second, omitted if neither — the body carries
`reasoning = {"effort": <value>, "summary": "auto"}` and any top-level
`reasoning_effort` key MUST be removed, because the `provider_options`
shallow merge will have injected the raw key onto the body.

Every other provider rejects native Freeform specs or history before network
I/O with the rejection error type above.

### Stream termination (native)

`ModelStreamDelta` streams follow the [Stream termination
contract](#stream-termination-contract): exactly one `Done { stop_reason }`
delta per successfully completed stream, emitted when the wire delivers a
`response.completed` or `response.incomplete` terminal event. When the byte
stream ends (EOF) without either terminal, the adapter yields or raises the
`IncompleteStream` error (Rust `MotosanError::IncompleteStream`, Python
`IncompleteStreamError`, TypeScript `IncompleteStreamError`) with the standard
payload `<provider> ended without a terminal event`. The provider names on the
native path are exactly `openai` and `chatgpt-codex` — note the hyphen, which
differs from the underscore the legacy Python `chatgpt_codex` adapter uses.
The exact native payloads are `openai ended without a terminal event` and
`chatgpt-codex ended without a terminal event`.

Six further rules are contract, not implementation detail, and every SDK's
collector (`collect_model_stream` / `collectModelStream`) MUST reproduce them:

- `ToolCallDone` is **authoritative**. Accumulated `FunctionArguments` /
  `FreeformInput` deltas are display bookkeeping and MUST NOT be lowered into
  the returned call.
- Freeform `input` survives byte-for-byte: never parsed as JSON, never lowered
  into function-call `arguments`.
- `Usage` **replaces** rather than merges.
- `ThinkingDone` wins over accumulated thinking deltas, and an explicitly
  empty `ThinkingDone` payload resolves to no thinking at all.
- Pending deltas drain before a stored stream error surfaces.
- A read-idle timeout wraps the native stream on HTTP providers.

The collector propagates an `IncompleteStream` error; its `stop_reason`
heuristic applies only to streams that did deliver a terminal.

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
| OpenAI (legacy `stream()` adapter) | `data: [DONE]` SSE sentinel, or a `finish_reason`-bearing chunk (either suffices; `finish_reason` is the semantic terminal, `[DONE]` the transport epilogue) |
| MiniMax | Python: `data: [DONE]` SSE sentinel, or a `finish_reason`-bearing chunk (either suffices, as for OpenAI — own OpenAI-compatible-wire adapter). Rust / TypeScript: `message_stop` — both delegate to the Anthropic adapter (Rust `build_minimax_provider` constructs an `AnthropicProvider`; TS `MinimaxProvider` wraps one), so the Anthropic rule applies |
| Anthropic | `message_stop` SSE event (the Python adapter additionally treats a stray `data: [DONE]` as terminal) |
| Gemini, GeminiCodeAssist | final SSE chunk carrying `finishReason` (a trailing `[DONE]` is tolerated but not required) |
| ChatGPT Codex (legacy `stream()` adapter) | `response.completed` SSE event |
| OpenAI Responses mode / ChatGPT Codex — native model API (`model_stream`) | `response.completed` or `response.incomplete` SSE event (either is a received terminal) |
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
