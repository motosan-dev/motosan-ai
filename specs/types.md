# Shared Type Definitions

Canonical type definitions shared across all language SDKs.
Each language implements these types idiomatically.

## Message

| Field | Type | Description |
|-------|------|-------------|
| `role` | `"user" \| "assistant" \| "system"` | Message role |
| `content` | `string` | Message text content |

## ChatRequest

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `messages` | `Message[]` | ✅ | Conversation history |
| `model` | `string` | ❌ | Provider model name. Defaults to SDK default per provider |
| `system` | `string` | ❌ | System prompt. SDK normalizes to provider format |
| `temperature` | `float` [0,2] | ❌ | Sampling temperature |
| `max_tokens` | `int` | ❌ | Maximum tokens in response |
| `tools` | `Tool[]` | ❌ | Tool definitions for function calling |
| `provider_options` | `object` | ❌ | Provider-specific options (passthrough, not validated) |

## ChatResponse

| Field | Type | Description |
|-------|------|-------------|
| `content` | `string` | Response text |
| `model` | `string` | Model used (from provider response) |
| `usage` | `Usage` | Token usage |
| `stop_reason` | `StopReason` | Why generation stopped |

## Usage

| Field | Type | Description |
|-------|------|-------------|
| `input_tokens` | `int` | Tokens in the request |
| `output_tokens` | `int` | Tokens in the response |

## StopReason

| Value | Description |
|-------|-------------|
| `end_turn` | Model completed naturally |
| `max_tokens` | Hit token limit |
| `tool_use` | Model called a tool |
| `stop` | Stop sequence hit |

## StreamEvent

| Field | Type | Description |
|-------|------|-------------|
| `content` | `string` | Token text delta |
| `done` | `bool` | Whether this is the final event |

## Tool

| Field | Type | Description |
|-------|------|-------------|
| `name` | `string` | Tool name |
| `description` | `string` | Tool description for the model |
| `parameters` | `JSON Schema object` | Input parameter schema |

## ToolCall (in response)

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | Tool call ID |
| `name` | `string` | Tool name |
| `input` | `object` | Parsed tool input |

## Error Types

| Type | Description |
|------|-------------|
| `AuthError` | Invalid or missing API key |
| `RateLimitError` | Too many requests. May include `retry_after` seconds |
| `InvalidRequestError` | Bad request parameters |
| `ProviderError` | Provider returned an error status |
| `NetworkError` | Connection or timeout failure |
| `StreamError` | SSE stream parsing failure |

## Provider Default Models

| Provider | Default Model |
|----------|--------------|
| `anthropic` | `claude-sonnet-4-5` |
| `openai` | `gpt-4o` |
| `minimax` | `MiniMax-Text-01` |
