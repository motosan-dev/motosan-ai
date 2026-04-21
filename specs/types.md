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
| `done` | `bool` | Exactly one terminal event per stream |
| `stop_reason` | `StopReason?` | Set on terminal event when provider reports one |
| `event_type` | `StreamEventType` | |
| `tool_call_id` | `string?` | |
| `tool_call_name` | `string?` | Set on `tool_call_start` |
| `tool_call_args_delta` | `string?` | Accumulate until `tool_call_end` |
| `usage` | `Usage?` | Set on `usage` events |

## StreamEventType

`text` | `tool_call_start` | `tool_call_args` | `tool_call_end` | `usage` | `done`

## MotosanError (Rust)

`Auth` | `RateLimit` | `InvalidRequest` | `Config` | `ProviderError` | `Network` | `Stream` | `StreamReadTimeout(u64)` | `UnsupportedFeature(String)`

## Default Models

| Provider | Default |
|----------|---------|
| Anthropic | `claude-sonnet-4-6` |
| OpenAI | `gpt-5.3-codex` |
| MiniMax | `MiniMax-M2.7` |
| Ollama | `llama3.2` |
| Gemini | `gemini-2.0-flash` |
| GeminiCodeAssist | `gemini-2.5-flash` |
