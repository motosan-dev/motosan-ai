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

import type {
  FunctionCallOutputContentItem,
  FunctionCallOutputPayload,
  Message,
  ModelContextItem,
  ModelToolCall,
  ModelToolOutput,
  ModelToolSpec,
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
