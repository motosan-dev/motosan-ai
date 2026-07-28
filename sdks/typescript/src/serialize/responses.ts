/**
 * OpenAI Responses API codec — the ONE place the native model surface is
 * translated to and from the wire.
 *
 * Mirrors Rust `providers/responses.rs`. Both native providers
 * (providers/chatgpt_codex.ts and providers/openai.ts) call into here, so the
 * two cannot drift.
 *
 * Wire divergences that matter (all pinned by tests):
 *   - ModelToolCall.id            -> wire key `call_id` (decoding accepts `call_id` OR `id`)
 *   - ModelChatRequest.maxTokens  -> body key `max_output_tokens`
 *   - Tool.inputSchema            -> wire key `parameters`
 *   - freeform tools              -> wire `type: "custom"` (never "freeform")
 *   - freeform `input`            -> raw text, preserved byte-for-byte, NEVER JSON-parsed
 *
 * Unlike serialize/{anthropic,gemini,openai}.ts this module owns decoding and
 * the SSE adapter too, because the Responses codec is symmetric and shared.
 */

import { IncompleteStreamError, StreamError } from '../error.js'
import { parseSse } from '../http/sse.js'
import type {
  FunctionCallOutputContentItem,
  FunctionCallOutputPayload,
  Message,
  ModelChatRequest,
  ModelChatResponse,
  ModelContextItem,
  ModelStreamDelta,
  ModelToolCall,
  ModelToolOutput,
  ModelToolSpec,
  StopReason,
  ToolChoice,
  Usage,
} from '../types.js'

/** Encode one tool definition into its Responses wire object. */
export function encodeToolSpec(spec: ModelToolSpec): Record<string, unknown> {
  if (spec.kind === 'freeform') {
    return {
      type: 'custom',
      name: spec.tool.name,
      description: spec.tool.description,
      format: {
        type: spec.tool.format.type,
        syntax: spec.tool.format.syntax,
        definition: spec.tool.format.definition,
      },
    }
  }
  // Rust's ToolSchema requires description/input_schema; TypeScript's Tool
  // makes them optional, so supply the empty forms rather than emitting null.
  return {
    type: 'function',
    name: spec.tool.name,
    description: spec.tool.description ?? '',
    parameters: spec.tool.inputSchema ?? {},
  }
}

/** Encode the whole tool list, order preserved. */
export function encodeTools(specs: ModelToolSpec[]): Record<string, unknown>[] {
  return specs.map(encodeToolSpec)
}

/** Encode a model tool call. `id` goes out under the wire key `call_id`. */
export function encodeToolCall(call: ModelToolCall): Record<string, unknown> {
  if (call.kind === 'freeform') {
    return {
      type: 'custom_tool_call',
      call_id: call.id,
      name: call.name,
      input: call.input,
    }
  }
  return {
    type: 'function_call',
    call_id: call.id,
    name: call.name,
    arguments: call.arguments,
  }
}

function encodeOutputContentItem(item: FunctionCallOutputContentItem): Record<string, unknown> {
  if (item.type === 'input_text') {
    return { type: 'input_text', text: item.text }
  }
  if (item.type === 'input_image') {
    const encoded: Record<string, unknown> = { type: 'input_image', image_url: item.imageUrl }
    if (item.detail !== undefined) encoded.detail = item.detail
    return encoded
  }
  return { type: 'encrypted_content', encrypted_content: item.encryptedContent }
}

function encodeOutputPayload(payload: FunctionCallOutputPayload): unknown {
  if (typeof payload === 'string') return payload
  return payload.map(encodeOutputContentItem)
}

/**
 * Encode a tool output. NOTE: the custom arm deliberately drops `name` — the
 * Responses body carries identity in `call_id` only, matching Rust
 * `encode_tool_output` (providers/responses.rs:34-49).
 */
export function encodeToolOutput(output: ModelToolOutput): Record<string, unknown> {
  if (output.kind === 'custom') {
    return {
      type: 'custom_tool_call_output',
      call_id: output.callId,
      output: encodeOutputPayload(output.output),
    }
  }
  return {
    type: 'function_call_output',
    call_id: output.callId,
    output: encodeOutputPayload(output.output),
  }
}

function encodeUserContent(message: Message): Record<string, unknown>[] {
  const blocks = message.contentBlocks ?? []
  if (blocks.length === 0) {
    return [{ type: 'input_text', text: message.content }]
  }

  const content: Record<string, unknown>[] = []
  for (const block of blocks) {
    if (block.type === 'text') {
      content.push({ type: 'input_text', text: block.text })
    } else if (block.type === 'image') {
      const source = block.source
      const imageUrl =
        source.type === 'base64'
          ? `data:${source.mediaType};base64,${source.data}`
          : source.url
      content.push({ type: 'input_image', image_url: imageUrl })
    }
    // Document blocks produce nothing (mirrors Rust ContentBlock::Document => {}).
  }

  if (content.length === 0) {
    content.push({ type: 'input_text', text: message.content })
  }
  return content
}

function encodeMessage(message: Message): Record<string, unknown>[] {
  switch (message.role) {
    case 'system':
      // System text belongs in `instructions`, never in `input`.
      return []
    case 'user':
      return [{ type: 'message', role: 'user', content: encodeUserContent(message) }]
    case 'assistant': {
      const items: Record<string, unknown>[] = []
      if (message.content) {
        items.push({
          type: 'message',
          role: 'assistant',
          content: [{ type: 'output_text', text: message.content }],
        })
      }
      for (const call of message.toolCalls ?? []) {
        items.push(
          encodeToolCall({
            kind: 'function',
            id: call.id,
            name: call.name,
            arguments: JSON.stringify(call.input) ?? '{}',
          }),
        )
      }
      return items
    }
    case 'tool':
      if (message.toolCallId === undefined) return []
      return [
        encodeToolOutput({
          kind: 'function',
          callId: message.toolCallId,
          output: message.content,
        }),
      ]
  }
}

/** Encode one ordered history entry into zero or more Responses input items. */
export function encodeContextItem(item: ModelContextItem): Record<string, unknown>[] {
  switch (item.kind) {
    case 'message':
      return encodeMessage(item.message)
    case 'toolCall':
      return [encodeToolCall(item.call)]
    case 'toolOutput':
      return [encodeToolOutput(item.output)]
  }
}

/** Encode the whole ordered context into the Responses `input` array. */
export function encodeInput(context: ModelContextItem[]): Record<string, unknown>[] {
  return context.flatMap(encodeContextItem)
}

function encodeToolChoice(choice: ToolChoice): unknown {
  switch (choice.type) {
    case 'auto':
      return 'auto'
    case 'required':
      return 'required'
    case 'none':
      return 'none'
    case 'tool':
      return { type: 'function', name: choice.name }
  }
}

/**
 * Build a Responses request body from a native model request.
 *
 * Two non-obvious rules, both load-bearing (Rust build_model_request_body,
 * providers/responses.rs:59-140):
 *
 *   1. System text is HOISTED into `instructions` and REMOVED from `input`.
 *      Precedence: systemBlocks (joined) > system > any Role::System message
 *      inside `context`; hoisted context messages are appended after whichever
 *      of the first two applied. `defaultInstructions` fills in only when
 *      nothing at all was supplied.
 *   2. `providerOptions` is shallow-merged into the body root LAST, so a
 *      caller can override anything this function produced.
 */
export function buildModelRequestBody(
  req: ModelChatRequest,
  defaultModel: string,
  stream: boolean,
  defaultInstructions?: string,
): Record<string, unknown> {
  const model = req.model ?? defaultModel
  const inputContext: ModelContextItem[] = []
  const instructionsParts: string[] = []

  if (req.systemBlocks !== undefined) {
    for (const block of req.systemBlocks) {
      const trimmed = block.text.trim()
      if (trimmed) instructionsParts.push(trimmed)
    }
  } else if (req.system !== undefined) {
    const trimmed = req.system.trim()
    if (trimmed) instructionsParts.push(trimmed)
  }

  for (const item of req.context) {
    if (item.kind === 'message' && item.message.role === 'system') {
      const trimmed = item.message.content.trim()
      if (trimmed) instructionsParts.push(trimmed)
      continue
    }
    inputContext.push(item)
  }

  const body: Record<string, unknown> = {
    model,
    input: encodeInput(inputContext),
  }

  if (stream) body.stream = true

  const toolSpecs = req.toolSpecs ?? []
  if (toolSpecs.length > 0) body.tools = encodeTools(toolSpecs)

  const instructions =
    instructionsParts.length > 0 ? instructionsParts.join('\n\n') : defaultInstructions
  if (instructions !== undefined) body.instructions = instructions

  if (req.temperature !== undefined) body.temperature = req.temperature
  // Wire key divergence: maxTokens -> max_output_tokens.
  if (req.maxTokens !== undefined) body.max_output_tokens = req.maxTokens
  if (req.toolChoice !== undefined) body.tool_choice = encodeToolChoice(req.toolChoice)
  if (req.stopSequences !== undefined && req.stopSequences.length > 0) {
    body.stop = req.stopSequences
  }

  // LAST — providerOptions wins over everything above.
  if (req.providerOptions !== undefined) {
    for (const [key, value] of Object.entries(req.providerOptions)) {
      body[key] = value
    }
  }

  return body
}

function asRecord(value: unknown): Record<string, any> | undefined {
  return value !== null && typeof value === 'object' ? (value as Record<string, any>) : undefined
}

function numberOrZero(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : 0
}

/**
 * Decode a Responses output item into a model tool call.
 *
 * Accepts `call_id` OR `id` for the identity, mirroring Rust's
 * `ModelToolCall` Deserialize. Returns undefined for anything that is not a
 * `function_call` / `custom_tool_call`, which is how the output-item loops use
 * it as a filter.
 */
export function decodeToolCall(item: unknown): ModelToolCall | undefined {
  const obj = asRecord(item)
  if (obj === undefined) return undefined

  const id =
    typeof obj.call_id === 'string' ? obj.call_id : typeof obj.id === 'string' ? obj.id : undefined
  const name = typeof obj.name === 'string' ? obj.name : undefined
  if (id === undefined || name === undefined) return undefined

  if (obj.type === 'function_call') {
    return {
      kind: 'function',
      id,
      name,
      arguments: typeof obj.arguments === 'string' ? obj.arguments : '',
    }
  }
  if (obj.type === 'custom_tool_call') {
    // Raw model text. Never JSON.parse it, never move it into `arguments`.
    return { kind: 'freeform', id, name, input: typeof obj.input === 'string' ? obj.input : '' }
  }
  return undefined
}

function decodeOutputContentItem(value: unknown): FunctionCallOutputContentItem | undefined {
  const obj = asRecord(value)
  if (obj === undefined) return undefined
  if (obj.type === 'input_text' && typeof obj.text === 'string') {
    return { type: 'input_text', text: obj.text }
  }
  if (obj.type === 'input_image' && typeof obj.image_url === 'string') {
    const item: FunctionCallOutputContentItem = { type: 'input_image', imageUrl: obj.image_url }
    if (
      obj.detail === 'auto' ||
      obj.detail === 'low' ||
      obj.detail === 'high' ||
      obj.detail === 'original'
    ) {
      item.detail = obj.detail
    }
    return item
  }
  if (obj.type === 'encrypted_content' && typeof obj.encrypted_content === 'string') {
    return { type: 'encrypted_content', encryptedContent: obj.encrypted_content }
  }
  return undefined
}

function decodeOutputPayload(value: unknown): FunctionCallOutputPayload | undefined {
  if (typeof value === 'string') return value
  if (!Array.isArray(value)) return undefined
  const items: FunctionCallOutputContentItem[] = []
  for (const entry of value) {
    const item = decodeOutputContentItem(entry)
    if (item === undefined) return undefined
    items.push(item)
  }
  return items
}

/** Decode a Responses output item into a caller tool output. */
export function decodeToolOutput(item: unknown): ModelToolOutput | undefined {
  const obj = asRecord(item)
  if (obj === undefined) return undefined

  const payload = decodeOutputPayload(obj.output)
  if (payload === undefined) return undefined
  const callId = typeof obj.call_id === 'string' ? obj.call_id : undefined
  if (callId === undefined) return undefined

  if (obj.type === 'function_call_output') {
    return { kind: 'function', callId, output: payload }
  }
  if (obj.type === 'custom_tool_call_output') {
    const output: ModelToolOutput = { kind: 'custom', callId, output: payload }
    if (typeof obj.name === 'string') output.name = obj.name
    return output
  }
  return undefined
}

/** Concatenate the `output_text` parts of a Responses `message` output item. */
export function decodeOutputText(item: unknown): string | undefined {
  const obj = asRecord(item)
  if (obj === undefined || obj.type !== 'message') return undefined
  if (!Array.isArray(obj.content)) return undefined

  let text = ''
  for (const part of obj.content) {
    const partObj = asRecord(part)
    if (partObj !== undefined && partObj.type === 'output_text' && typeof partObj.text === 'string') {
      text += partObj.text
    }
  }
  return text === '' ? undefined : text
}

/**
 * Decode a Responses usage object. Accepts the Responses key names and the
 * chat-completions ones; `cacheReadInputTokens` is emitted only when
 * `input_tokens_details.cached_tokens` is greater than zero.
 */
export function decodeUsage(value: unknown): Usage {
  const usage = asRecord(value)
  const inputTokens = numberOrZero(usage?.input_tokens ?? usage?.prompt_tokens)
  const outputTokens = numberOrZero(usage?.output_tokens ?? usage?.completion_tokens)
  const cached = numberOrZero(asRecord(usage?.input_tokens_details)?.cached_tokens)

  const result: Usage = { inputTokens, outputTokens }
  if (cached > 0) result.cacheReadInputTokens = cached
  return result
}

/** Map a Responses `status` to a StopReason. Tool calls always win. */
export function stopReasonFromStatus(
  status: string | undefined,
  hasToolCalls: boolean,
): StopReason {
  if (hasToolCalls) return 'tool_use'
  if (status === 'incomplete') return 'max_tokens'
  if (status === undefined || status === 'completed') return 'end_turn'
  return 'other'
}

/**
 * Decode a NON-STREAMING Responses payload into a ModelChatResponse. This is
 * the genuine blocking decode path (OpenAI); ChatGPT Codex has no
 * non-streaming endpoint and reaches the same shape via collectModelStream.
 */
export function modelChatResponseFromOutput(
  payload: unknown,
  defaultModel: string,
): ModelChatResponse {
  const obj = asRecord(payload) ?? {}
  let content = ''
  let thinking: string | undefined
  const toolCalls: ModelToolCall[] = []

  if (typeof obj.output_text === 'string') content += obj.output_text

  if (Array.isArray(obj.output)) {
    for (const item of obj.output) {
      const text = decodeOutputText(item)
      if (text !== undefined) content += text

      const itemObj = asRecord(item)
      if (itemObj !== undefined && itemObj.type === 'reasoning' && Array.isArray(itemObj.summary)) {
        let summary = ''
        for (const part of itemObj.summary) {
          const partObj = asRecord(part)
          if (partObj === undefined) continue
          const value =
            typeof partObj.text === 'string'
              ? partObj.text
              : typeof partObj.content === 'string'
                ? partObj.content
                : undefined
          if (value !== undefined) summary += value
        }
        if (summary !== '') thinking = summary
      }

      const call = decodeToolCall(item)
      if (call !== undefined) toolCalls.push(call)
    }
  }

  const response: ModelChatResponse = {
    content,
    toolCalls,
    model: typeof obj.model === 'string' ? obj.model : defaultModel,
    usage: decodeUsage(obj.usage),
    stopReason: stopReasonFromStatus(
      typeof obj.status === 'string' ? obj.status : undefined,
      toolCalls.length > 0,
    ),
  }
  if (thinking !== undefined) response.thinking = thinking
  return response
}

/**
 * First non-empty wins: top-level `message` -> `response.error.message` ->
 * `error.message` -> fallback. Mirrors Rust's error arm in
 * ResponsesModelStreamAdapter::handle_event.
 */
function responsesStreamErrorMessage(data: Record<string, any>): string {
  if (typeof data.message === 'string' && data.message) return data.message
  const nested = asRecord(asRecord(data.response)?.error)?.message
  if (typeof nested === 'string' && nested) return nested
  const top = asRecord(data.error)?.message
  if (typeof top === 'string' && top) return top
  return 'responses stream error'
}

/**
 * Adapt a Responses SSE byte stream into ModelStreamDelta values.
 *
 * Contract (specs/types.md § Stream termination (native), milestone D8):
 *   - Exactly ONE terminal `done` per successfully completed stream, emitted
 *     on `response.completed` or `response.incomplete`. Frames after the
 *     terminal are consumed but produce no second done.
 *   - EOF without either terminal throws IncompleteStreamError carrying
 *     `<provider> ended without a terminal event`. Callers pass `openai` or
 *     `chatgpt-codex` (HYPHEN — the legacy chat adapter's `chatgpt_codex`
 *     token is a different, unchanged string).
 *   - Pending deltas from a frame are yielded before an error frame throws.
 *   - `tool_call_done` is authoritative; the accumulated input/argument
 *     deltas are display bookkeeping only.
 */
export async function* modelStreamAdapter(
  body: ReadableStream<Uint8Array>,
  provider: string,
): AsyncGenerator<ModelStreamDelta> {
  const itemToCallId = new Map<string, string>()
  let sawToolCall = false
  let sawTerminal = false

  const rememberOutputItem = (item: Record<string, any>): void => {
    if (item.type !== 'function_call' && item.type !== 'custom_tool_call') return
    if (typeof item.call_id !== 'string') return
    sawToolCall = true
    if (typeof item.id === 'string' && item.id !== '') itemToCallId.set(item.id, item.call_id)
  }

  const callIdFromEvent = (data: Record<string, any>): string | undefined => {
    if (typeof data.call_id === 'string') return data.call_id
    if (typeof data.item_id === 'string') return itemToCallId.get(data.item_id) ?? data.item_id
    return undefined
  }

  for await (const evt of parseSse(body)) {
    const data = evt.data
    if (!data || data === '[DONE]' || typeof data !== 'object') continue
    if (sawTerminal) continue

    switch (data.type) {
      case 'response.output_text.delta': {
        if (typeof data.delta === 'string' && data.delta !== '') {
          yield { type: 'text', delta: data.delta }
        }
        break
      }
      case 'response.reasoning_text.delta':
      case 'response.reasoning_summary_text.delta': {
        if (typeof data.delta === 'string' && data.delta !== '') {
          yield { type: 'thinking_delta', delta: data.delta }
        }
        break
      }
      case 'response.reasoning_text.done':
      case 'response.reasoning_summary_text.done': {
        const thinking =
          typeof data.text === 'string'
            ? data.text
            : typeof data.delta === 'string'
              ? data.delta
              : undefined
        if (thinking !== undefined) yield { type: 'thinking_done', thinking }
        break
      }
      case 'response.output_item.added': {
        const item = asRecord(data.item)
        if (item !== undefined) rememberOutputItem(item)
        break
      }
      case 'response.function_call_arguments.delta': {
        const callId = callIdFromEvent(data)
        if (callId !== undefined && typeof data.delta === 'string') {
          yield { type: 'function_arguments', callId, delta: data.delta }
        }
        break
      }
      case 'response.custom_tool_call_input.delta': {
        const callId = callIdFromEvent(data)
        if (callId !== undefined && typeof data.delta === 'string') {
          yield { type: 'freeform_input', callId, delta: data.delta }
        }
        break
      }
      case 'response.output_item.done': {
        const item = asRecord(data.item)
        if (item !== undefined) {
          rememberOutputItem(item)
          const call = decodeToolCall(item)
          if (call !== undefined) {
            sawToolCall = true
            yield { type: 'tool_call_done', call }
          }
        }
        break
      }
      case 'response.completed':
      case 'response.incomplete': {
        const response = asRecord(data.response)
        const usage = decodeUsage(response?.usage)
        if (
          usage.inputTokens !== 0 ||
          usage.outputTokens !== 0 ||
          usage.cacheCreationInputTokens !== undefined ||
          usage.cacheReadInputTokens !== undefined
        ) {
          yield { type: 'usage', usage }
        }
        const status = typeof response?.status === 'string' ? response.status : undefined
        yield { type: 'done', stopReason: stopReasonFromStatus(status, sawToolCall) }
        sawTerminal = true
        break
      }
      case 'error':
      case 'response.failed': {
        sawTerminal = true
        throw new StreamError(responsesStreamErrorMessage(data))
      }
      default:
        break
    }
  }

  if (!sawTerminal) {
    throw new IncompleteStreamError(`incomplete stream: ${provider} ended without a terminal event`)
  }
}
