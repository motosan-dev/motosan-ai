import type { ChatRequest, ContentBlock } from '../types.js'

const DEFAULT_MAX_TOKENS = 8192
const CACHE_CONTROL = { type: 'ephemeral' } as const

type SerializedBlock = Record<string, unknown>

function serializeContentBlock(block: ContentBlock): SerializedBlock {
  if (block.type === 'text') {
    return { type: 'text', text: block.text }
  }

  if (block.type === 'image') {
    const source = block.source
    if (source.type === 'base64') {
      return {
        type: 'image',
        source: {
          type: 'base64',
          media_type: source.mediaType,
          data: source.data,
        },
      }
    }

    return {
      type: 'image',
      source: {
        type: 'url',
        url: source.url,
      },
    }
  }

  const source = block.source
  if (source.type === 'base64') {
    return {
      type: 'document',
      source: {
        type: 'base64',
        media_type: source.mediaType,
        data: source.data,
      },
    }
  }

  return {
    type: 'document',
    source: {
      type: 'url',
      url: source.url,
    },
  }
}

function withLastBlockCache(blocks: SerializedBlock[], enabled?: boolean): SerializedBlock[] {
  if (!enabled || blocks.length === 0) {
    return blocks
  }

  const last = blocks[blocks.length - 1]
  last.cache_control = CACHE_CONTROL
  return blocks
}

function serializeSystemBlocks(
  blocks: Array<{ text: string; cacheControl?: boolean }>,
): SerializedBlock[] {
  return blocks.map((block) => {
    const serialized: SerializedBlock = {
      type: 'text',
      text: block.text,
    }

    if (block.cacheControl) {
      serialized.cache_control = CACHE_CONTROL
    }

    return serialized
  })
}

export function serializeAnthropicRequest(
  req: ChatRequest,
  model: string,
): Record<string, any> {
  const result: Record<string, any> = {
    model,
    max_tokens: req.maxTokens ?? DEFAULT_MAX_TOKENS,
    messages: [],
  }

  const messages: SerializedBlock[] = []

  for (const message of req.messages) {
    if (message.role === 'system') {
      continue
    }

    if (message.role === 'tool') {
      if (message.toolCallId) {
        messages.push({
          role: 'user',
          content: [
            {
              type: 'tool_result',
              tool_use_id: message.toolCallId,
              content: message.content,
            },
          ],
        })
      }
      continue
    }

    if (message.role === 'assistant' && message.toolCalls && message.toolCalls.length > 0) {
      const content: SerializedBlock[] = []

      if (message.content.trim().length > 0) {
        content.push({ type: 'text', text: message.content })
      }

      for (const toolCall of message.toolCalls) {
        content.push({
          type: 'tool_use',
          id: toolCall.id,
          name: toolCall.name,
          input: toolCall.input,
        })
      }

      messages.push({
        role: 'assistant',
        content: withLastBlockCache(content, message.cache),
      })
      continue
    }

    if (message.contentBlocks && message.contentBlocks.length > 0) {
      messages.push({
        role: message.role,
        content: withLastBlockCache(
          message.contentBlocks.map(serializeContentBlock),
          message.cache,
        ),
      })
      continue
    }

    if (message.cache) {
      messages.push({
        role: message.role,
        content: [
          {
            type: 'text',
            text: message.content,
            cache_control: CACHE_CONTROL,
          },
        ],
      })
      continue
    }

    messages.push({
      role: message.role,
      content: message.content,
    })
  }

  result.messages = messages

  if (req.systemBlocks && req.systemBlocks.length > 0) {
    result.system = serializeSystemBlocks(req.systemBlocks)
  } else if (req.system) {
    if (req.systemCache) {
      result.system = [
        {
          type: 'text',
          text: req.system,
          cache_control: CACHE_CONTROL,
        },
      ]
    } else {
      result.system = req.system
    }
  }

  if (req.tools && req.tools.length > 0) {
    result.tools = req.tools.map((tool) => {
      const serialized: SerializedBlock = {
        name: tool.name,
        description: tool.description,
        input_schema: tool.inputSchema,
      }

      // Per-tool cache flag, position-independent — matches Rust
      // providers/anthropic.rs (`if tool.cache { cache_control = ... }`).
      if (tool.cache) {
        serialized.cache_control = CACHE_CONTROL
      }

      return serialized
    })
  }

  if (req.thinking) {
    result.thinking = req.thinking
  }

  if (req.stopSequences && req.stopSequences.length > 0) {
    result.stop_sequences = req.stopSequences
  }

  if (req.temperature !== undefined) {
    result.temperature = req.temperature
  }

  if (req.providerOptions && typeof req.providerOptions === 'object') {
    Object.assign(result, req.providerOptions)
  }

  return result
}
