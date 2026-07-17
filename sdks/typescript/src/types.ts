/**
 * Core structured type system for the Motosan AI TypeScript SDK.
 *
 * Mirrors the Rust SDK's `types.rs` (source of truth) with idiomatic TS:
 * discriminated unions for content blocks / stream events, optional fields
 * OMITTED (not `undefined`) when absent, camelCase mapping Rust's snake_case.
 * Wire serialization lives in `serialize/*.ts`, NOT here.
 */

/** Conversation role. Serialized lowercase on the wire (handled by serializers). */
export type Role = 'user' | 'assistant' | 'system' | 'tool'

/** Source for an image content block. Discriminated on `type`. */
export type ImageSource =
  | { type: 'base64'; mediaType: string; data: string }
  | { type: 'url'; url: string }

/** Source for a document content block (e.g. PDF). Anthropic-only. */
export type DocumentSource =
  | { type: 'base64'; mediaType: string; data: string }
  | { type: 'url'; url: string }

/** A single piece of structured message content. Discriminated on `type`. */
export type ContentBlock =
  | { type: 'text'; text: string }
  | { type: 'image'; source: ImageSource }
  | { type: 'document'; source: DocumentSource }

/**
 * A tool/function call requested by the model. The arguments field is `input`
 * (NOT `args`/`params`) per project convention, kept as the parsed JSON value
 * returned by the provider.
 */
export interface ToolCall {
  id: string
  name: string
  input: unknown
}

/** A tool definition exposed to the model. */
export interface Tool {
  name: string
  description?: string
  inputSchema?: Record<string, unknown>
  /** When true, attaches Anthropic `cache_control` to this tool (per-tool, position-independent). */
  cache?: boolean
}

/**
 * Controls how the model selects which tool (if any) to call.
 * Placeholder for M1 — full wire serialization lands in M2.
 */
export type ToolChoice =
  | { type: 'auto' }
  | { type: 'required' }
  | { type: 'none' }
  | { type: 'tool'; name: string }

/** Configuration for extended thinking (Anthropic). */
export interface ThinkingConfig {
  budgetTokens: number
}

/** Server-side MCP transport type. Mirrors Rust `McpServerType` (types.rs:301-305, lowercase). */
export type McpServerType = 'url'

/**
 * Config for a server-side MCP server. Mirrors Rust `McpServerConfig`
 * (types.rs:312-320). The provider connects to the MCP server server-side;
 * the client never manages the connection. Anthropic wire only.
 */
export interface McpServerConfig {
  /** Rust `kind`, serde-renamed to wire key "type" (types.rs:314). */
  type: McpServerType
  url: string
  name: string
  /** Rust `authorization_token`; omitted on the wire when absent (types.rs:318-319). */
  authorizationToken?: string
}

/**
 * Per-server MCP tool filtering. Mirrors Rust `McpToolConfig` enum
 * (types.rs:326-340). Discriminated union on `kind` — a TS contract choice
 * (the Rust enum is untagged; the wire form is hand-built by the serializer).
 */
export type McpToolConfig =
  | { kind: 'all'; mcpServerName: string }
  | { kind: 'allowed'; mcpServerName: string; allowedTools: string[] }
  | { kind: 'denied'; mcpServerName: string; deniedTools: string[] }

/** A system prompt block with optional cache control (Anthropic ephemeral cache). */
export interface SystemBlock {
  text: string
  cacheControl?: boolean
}

/** Token usage accounting. Cache fields are Anthropic-only and optional. */
export interface Usage {
  inputTokens: number
  outputTokens: number
  cacheCreationInputTokens?: number
  cacheReadInputTokens?: number
}

/** Why the model stopped generating. */
export type StopReason =
  | 'end_turn'
  | 'max_tokens'
  | 'tool_use'
  | 'stop'
  | 'stop_sequence'
  | 'other'

/**
 * The kind of a streaming event. Anthropic emits `thinking_delta` and
 * `thinking_done`; ChatGPT Codex emits `thinking_delta` reasoning deltas.
 * `collectStream` concatenates them into `ChatResponse.thinking`.
 */
export type StreamEventType =
  | 'text'
  | 'tool_call_start'
  | 'tool_call_args'
  | 'tool_call_end'
  | 'usage'
  | 'thinking_delta'
  | 'thinking_done'

/** A single streaming event. */
export interface StreamEvent {
  content: string
  done: boolean
  eventType: StreamEventType
  toolCallId?: string
  toolCallName?: string
  toolCallArgsDelta?: string
  usage?: Usage
  stopReason?: StopReason
}

/**
 * A conversation message. `content` is a flat string (first text block) for
 * backward compat; `contentBlocks` holds the structured multimodal form.
 */
export interface Message {
  role: Role
  content: string
  contentBlocks?: ContentBlock[]
  toolCallId?: string
  toolCalls?: ToolCall[]
  cache?: boolean
}

/** A chat request. Provider-agnostic; serializers project it to each wire format. */
export interface ChatRequest {
  messages: Message[]
  tools?: Tool[]
  system?: string
  systemBlocks?: SystemBlock[]
  systemCache?: boolean
  toolChoice?: ToolChoice
  thinking?: ThinkingConfig
  stopSequences?: string[]
  model?: string
  maxTokens?: number
  temperature?: number
  providerOptions?: Record<string, unknown>
  /** Server-side MCP servers (Anthropic wire only). Mirrors Rust `mcp_servers` (types.rs:364-372). */
  mcpServers?: McpServerConfig[]
  /** Per-server MCP tool filtering. Mirrors Rust `mcp_tool_configs` (types.rs:364-372). */
  mcpToolConfigs?: McpToolConfig[]
}

/** A non-streaming chat response (or the reassembly of a stream via collectStream). */
export interface ChatResponse {
  content: string
  thinking?: string
  toolCalls: ToolCall[]
  model: string
  usage: Usage
  stopReason: StopReason
}
