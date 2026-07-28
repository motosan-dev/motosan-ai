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

// ---------------------------------------------------------------------------
// Native model API (specs/types.md § Native Model API).
//
// A surface PARALLEL to ChatRequest/Tool/ToolCall/ChatResponse/StreamEvent,
// for providers that expose OpenAI Responses-style ordered input items and
// custom (freeform) tool calls. The legacy surface above stays
// function-tool-only; nothing here widens it.
//
// Tag choice (milestone D2): ModelToolSpec / ModelToolCall / ModelToolOutput /
// ModelContextItem are tagged on `kind` because the model shape and the wire
// shape disagree (freeform <-> wire "custom", id <-> wire call_id) — the same
// reason McpToolConfig above uses `kind`. ModelStreamDelta and
// FunctionCallOutputContentItem are tagged on `type` because their tag VALUES
// are exactly the wire values.
//
// Wire encoding lives in serialize/responses.ts, never here.
// ---------------------------------------------------------------------------

/** Grammar/format descriptor for a freeform tool. All three fields are mandatory. */
export interface FreeformToolFormat {
  type: string
  syntax: string
  definition: string
}

/** A freeform ("custom") tool definition. Serializes with a wire `type: "custom"`. */
export interface FreeformTool {
  name: string
  description: string
  format: FreeformToolFormat
}

/**
 * A tool exposed to the model on the native surface. `function` wraps the
 * existing `Tool` (wire `{type:"function", name, description, parameters}`);
 * `freeform` wraps a `FreeformTool` (wire `{type:"custom", name, description,
 * format}`).
 */
export type ModelToolSpec =
  | { kind: 'function'; tool: Tool }
  | { kind: 'freeform'; tool: FreeformTool }

/** Image fidelity hint on a Responses `input_image` content item. */
export type ImageDetail = 'auto' | 'low' | 'high' | 'original'

/** One Responses-style content item inside a tool output payload. */
export type FunctionCallOutputContentItem =
  | { type: 'input_text'; text: string }
  | { type: 'input_image'; imageUrl: string; detail?: ImageDetail }
  | { type: 'encrypted_content'; encryptedContent: string }

/** A tool output payload: plain text, or Responses-style content items. */
export type FunctionCallOutputPayload = string | FunctionCallOutputContentItem[]

/**
 * A tool call the model produced. `id` is the caller-facing identity; it is
 * written to (and read from) the wire key `call_id`. Freeform `input` is raw
 * model text — preserved byte-for-byte, never parsed as JSON, never lowered
 * into a function call's `arguments`.
 */
export type ModelToolCall =
  | { kind: 'function'; id: string; name: string; arguments: string }
  | { kind: 'freeform'; id: string; name: string; input: string }

/** A tool result the caller returns to the model. */
export type ModelToolOutput =
  | { kind: 'function'; callId: string; output: FunctionCallOutputPayload }
  | { kind: 'custom'; callId: string; name?: string; output: FunctionCallOutputPayload }

/**
 * One ordered history entry. Preserving message / tool-call / tool-output
 * ORDER is what makes byte-exact replay of freeform inputs possible in
 * multi-turn histories.
 */
export type ModelContextItem =
  | { kind: 'message'; message: Message }
  | { kind: 'toolCall'; call: ModelToolCall }
  | { kind: 'toolOutput'; output: ModelToolOutput }

/**
 * A native model request. Deliberately carries NO thinking and NO MCP config
 * (milestone D3): native requests reach provider-specific reasoning controls
 * through `providerOptions`.
 */
export interface ModelChatRequest {
  context: ModelContextItem[]
  toolSpecs?: ModelToolSpec[]
  model?: string
  system?: string
  systemBlocks?: SystemBlock[]
  systemCache?: boolean
  temperature?: number
  /** Serialized to the Responses body key `max_output_tokens`. */
  maxTokens?: number
  toolChoice?: ToolChoice
  stopSequences?: string[]
  /** Shallow-merged into the request body root LAST — it overrides everything. */
  providerOptions?: Record<string, unknown>
}

/** A native, non-streaming model response. */
export interface ModelChatResponse {
  content: string
  thinking?: string
  toolCalls: ModelToolCall[]
  model: string
  usage: Usage
  stopReason: StopReason
  sessionId?: string
}

/**
 * One native stream delta. `tool_call_done` is AUTHORITATIVE for a completed
 * call; accumulated `function_arguments` / `freeform_input` deltas are display
 * bookkeeping only. Exactly one `done` per successfully completed stream.
 */
export type ModelStreamDelta =
  | { type: 'text'; delta: string }
  | { type: 'thinking_delta'; delta: string }
  | { type: 'thinking_done'; thinking: string }
  | { type: 'function_arguments'; callId: string; delta: string }
  | { type: 'freeform_input'; callId: string; delta: string }
  | { type: 'tool_call_done'; call: ModelToolCall }
  | { type: 'usage'; usage: Usage }
  | { type: 'done'; stopReason: StopReason }
