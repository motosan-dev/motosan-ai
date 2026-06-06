import type { ChatRequest, ContentBlock, Message } from '../types.js'

/**
 * OpenAI Chat Completions request serializer.
 *
 * Mirrors the structure/idioms of serialize/anthropic.ts but projects the
 * provider-agnostic ChatRequest onto the OpenAI `/v1/chat/completions` wire,
 * which diverges from Anthropic in four load-bearing ways (CLAUDE.md #1 bug
 * source):
 *
 *   1. System prompt is a `role: "system"` MESSAGE in the messages array,
 *      NOT a top-level `system` field. (system_blocks are JOINED into one
 *      system message — OpenAI has no per-block cache_control; cache flags
 *      are silently ignored here.)
 *   2. Tools are FLAT: `{type:"function", function:{name,description,parameters}}`,
 *      NOT Anthropic's nested `{name,description,input_schema}`.
 *   3. Assistant tool calls are `message.tool_calls[]` with
 *      `function.arguments` as a JSON STRING (not a parsed object), NOT
 *      Anthropic `tool_use` content blocks.
 *   4. tool_choice maps: auto→"auto", required→"required", none→"none"
 *      (string forms — OpenAI HAS a real "none"), tool→
 *      {type:"function", function:{name}}.
 *
 * Stop sequences serialize to `stop`; `max_tokens` is only emitted when the
 * caller set it (OpenAI has no SDK-side default). providerOptions are merged
 * into the request root last, mirroring the Anthropic serializer.
 *
 * This is the CANONICAL OpenAI serializer export. providers/openai.ts and
 * providers/minimax.ts both import serializeOpenAiRequest from HERE.
 */

type SerializedMessage = Record<string, unknown>

function serializeUserContentBlock(block: ContentBlock): Record<string, unknown> {
  if (block.type === 'text') {
    return { type: 'text', text: block.text }
  }

  if (block.type === 'image') {
    const source = block.source
    if (source.type === 'base64') {
      return {
        type: 'image_url',
        image_url: { url: `data:${source.mediaType};base64,${source.data}` },
      }
    }

    return {
      type: 'image_url',
      image_url: { url: source.url },
    }
  }

  // Document blocks are not supported by OpenAI chat completions; capability
  // validation rejects them before serialization (matches Rust's
  // `ContentBlock::Document { .. } => unreachable!()`). Defensive throw keeps
  // this total without an `any` cast.
  throw new Error('OpenAI does not support document content blocks')
}

function serializeMessage(message: Message): SerializedMessage | null {
  if (message.role === 'tool') {
    if (!message.toolCallId) {
      return null
    }
    return {
      role: 'tool',
      tool_call_id: message.toolCallId,
      content: message.content,
    }
  }

  if (message.role === 'system') {
    return { role: 'system', content: message.content }
  }

  if (message.role === 'assistant') {
    if (message.toolCalls && message.toolCalls.length > 0) {
      return {
        role: 'assistant',
        content: message.content,
        tool_calls: message.toolCalls.map((toolCall) => ({
          id: toolCall.id,
          type: 'function',
          function: {
            name: toolCall.name,
            // OpenAI requires arguments as a JSON STRING, not an object.
            arguments: JSON.stringify(toolCall.input),
          },
        })),
      }
    }
    return { role: 'assistant', content: message.content }
  }

  // role === 'user'
  if (message.contentBlocks && message.contentBlocks.length > 0) {
    return {
      role: 'user',
      content: message.contentBlocks.map(serializeUserContentBlock),
    }
  }

  return { role: 'user', content: message.content }
}

export function serializeOpenAiRequest(
  req: ChatRequest,
  model: string,
): Record<string, unknown> {
  const messages: SerializedMessage[] = []

  // System prompt becomes the FIRST message (role: system). Priority:
  // systemBlocks > system string (matches Rust OpenAIRequestBuilder). OpenAI
  // has no per-block cache_control, so blocks are joined with a single newline
  // (the chat-completions wire; the Responses API — deferred to M3 — uses \n\n).
  if (req.systemBlocks && req.systemBlocks.length > 0) {
    const joined = req.systemBlocks.map((block) => block.text).join('\n')
    if (joined.length > 0) {
      messages.push({ role: 'system', content: joined })
    }
  } else if (req.system) {
    messages.push({ role: 'system', content: req.system })
  }

  for (const message of req.messages) {
    const serialized = serializeMessage(message)
    if (serialized !== null) {
      messages.push(serialized)
    }
  }

  const result: Record<string, unknown> = {
    model,
    messages,
  }

  if (req.temperature !== undefined) {
    result.temperature = req.temperature
  }

  if (req.maxTokens !== undefined) {
    result.max_tokens = req.maxTokens
  }

  if (req.tools && req.tools.length > 0) {
    result.tools = req.tools.map((tool) => ({
      type: 'function',
      function: {
        name: tool.name,
        description: tool.description ?? '',
        parameters: tool.inputSchema ?? { type: 'object', properties: {} },
      },
    }))
  }

  if (req.toolChoice) {
    switch (req.toolChoice.type) {
      case 'auto':
        result.tool_choice = 'auto'
        break
      case 'required':
        result.tool_choice = 'required'
        break
      case 'none':
        result.tool_choice = 'none'
        break
      case 'tool':
        result.tool_choice = {
          type: 'function',
          function: { name: req.toolChoice.name },
        }
        break
    }
  }

  if (req.stopSequences && req.stopSequences.length > 0) {
    result.stop = req.stopSequences
  }

  if (req.providerOptions && typeof req.providerOptions === 'object') {
    Object.assign(result, req.providerOptions)
  }

  return result
}
