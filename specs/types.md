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
| `model` | `string` |
| `usage` | `Usage` |
| `stop_reason` | `StopReason` |

## StopReason
`end_turn` | `max_tokens` | `tool_use` | `stop` | `other`

## StreamEvent
| Field | Type |
|-------|------|
| `content` | `string` (delta) |
| `done` | `bool` |

## Default Models
| Provider | Default |
|----------|---------|
| anthropic | `claude-sonnet-4-5` |
| openai | `gpt-4o` |
| minimax | `MiniMax-Text-01` |
