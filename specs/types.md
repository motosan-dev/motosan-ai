# Shared Type Definitions

Canonical definitions shared across all language SDKs.

## Message
| Field | Type |
|-------|------|
| `role` | `"user" \| "assistant" \| "system"` |
| `content` | `string` |

## ChatRequest
| Field | Type | Required |
|-------|------|----------|
| `messages` | `Message[]` | ✅ |
| `model` | `string` | ❌ defaults per provider |
| `system` | `string` | ❌ SDK normalizes to provider format |
| `temperature` | `float` | ❌ |
| `max_tokens` | `int` | ❌ |
| `tools` | `Tool[]` | ❌ |
| `provider_options` | `object` | ❌ passthrough, not validated |

## ChatResponse
| Field | Type |
|-------|------|
| `content` | `string` |
| `tool_calls` | `ToolCall[]` |
| `model` | `string` |
| `usage` | `Usage` |
| `stop_reason` | `StopReason` |

## StopReason
`end_turn` | `max_tokens` | `tool_use` | `stop` | `other`

## StreamEvent
| Field | Type | Default |
|-------|------|---------|
| `content` | `string` (delta) | |
| `done` | `bool` | |
| `tool_call_id` | `string?` | `null` |
| `tool_call_name` | `string?` | `null` |
| `tool_call_args_delta` | `string?` | `null` |
| `event_type` | `StreamEventType` | `"text"` |

## StreamEventType
`text` | `tool_call_start` | `tool_call_args` | `tool_call_end`

## Default Models
| Provider | Default |
|----------|---------|
| anthropic | `claude-sonnet-4-5` |
| openai | `gpt-4o` |
| minimax | `MiniMax-Text-01` |
