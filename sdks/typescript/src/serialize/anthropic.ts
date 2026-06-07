import type { ChatRequest, ContentBlock, McpToolConfig } from '../types.js'

const DEFAULT_MAX_TOKENS = 8192
const CACHE_CONTROL = { type: 'ephemeral' } as const

/**
 * Models that use ADAPTIVE thinking (Anthropic chooses the budget; the older
 * budget-token shape is rejected). Mirrors Rust `model_uses_adaptive_thinking`
 * (anthropic.rs:195-200). Opus 4.x is adaptive; 4-7 is kept though absent from
 * ANTHROPIC_MODELS, matching the Rust literal set.
 */
const ADAPTIVE_THINKING_MODELS = new Set(['claude-opus-4-8', 'claude-opus-4-7', 'claude-opus-4-6'])

/** Whether `model` uses adaptive thinking. Exported for the beta-header builder (Task 3). */
export function modelUsesAdaptiveThinking(model: string): boolean {
  return ADAPTIVE_THINKING_MODELS.has(model)
}

/**
 * Apply thinking config onto the result body. Mirrors Rust `apply_thinking_config`
 * (anthropic.rs:202-220). Adaptive → thinking{type:adaptive,display:summarized} +
 * output_config{effort:high}, no budget_tokens. Otherwise → thinking{type:enabled,
 * budget_tokens,display:summarized}. `display:"summarized"` is unconditional.
 */
function applyThinkingConfig(
  result: Record<string, any>,
  model: string,
  thinking: { budgetTokens: number },
): void {
  if (modelUsesAdaptiveThinking(model)) {
    result.thinking = { type: 'adaptive', display: 'summarized' }
    result.output_config = { effort: 'high' }
  } else {
    result.thinking = {
      type: 'enabled',
      budget_tokens: thinking.budgetTokens,
      display: 'summarized',
    }
  }
}

/**
 * Serialize one McpToolConfig to an Anthropic `mcp_toolset` tools-array entry.
 * Mirrors Rust `serialize_mcp_tool_config` (anthropic.rs:170-193). Wire keys are
 * snake_case.
 */
function serializeMcpToolConfig(config: McpToolConfig): SerializedBlock {
  if (config.kind === 'all') {
    return { type: 'mcp_toolset', mcp_server_name: config.mcpServerName }
  }
  if (config.kind === 'allowed') {
    return {
      type: 'mcp_toolset',
      mcp_server_name: config.mcpServerName,
      allowed_tools: config.allowedTools,
    }
  }
  return {
    type: 'mcp_toolset',
    mcp_server_name: config.mcpServerName,
    denied_tools: config.deniedTools,
  }
}

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

  // Combined tools array: regular tools first, then mcp_toolset items.
  // Mirrors Rust `all_tools` assembly (anthropic.rs:386-392); body.tools is set
  // iff the combined array is non-empty (`!all_tools.is_empty()`).
  const allTools: SerializedBlock[] = []
  if (req.tools && req.tools.length > 0) {
    for (const tool of req.tools) {
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
      allTools.push(serialized)
    }
  }
  if (req.mcpToolConfigs && req.mcpToolConfigs.length > 0) {
    for (const config of req.mcpToolConfigs) {
      allTools.push(serializeMcpToolConfig(config))
    }
  }
  if (allTools.length > 0) {
    result.tools = allTools
  }

  if (req.toolChoice) {
    switch (req.toolChoice.type) {
      case 'auto':
        result.tool_choice = { type: 'auto' }
        break
      case 'required':
        result.tool_choice = { type: 'any' }
        break
      case 'none':
        // Anthropic has no native "none"; removing tools prevents calls.
        delete result.tools
        break
      case 'tool':
        result.tool_choice = { type: 'tool', name: req.toolChoice.name }
        break
    }
  }

  // Thinking/temperature collision. Mirrors Rust anthropic.rs:352-367:
  // when thinking is set, non-adaptive forces temperature=1.0 and the user
  // temperature is NOT applied (it lives only in the else-if branch).
  if (req.thinking) {
    if (!modelUsesAdaptiveThinking(model)) {
      result.temperature = 1.0
    }
    applyThinkingConfig(result, model, req.thinking)
  } else if (req.temperature !== undefined) {
    result.temperature = req.temperature
  }

  if (req.stopSequences && req.stopSequences.length > 0) {
    result.stop_sequences = req.stopSequences
  }

  // mcp_servers body key. Mirrors Rust anthropic.rs:417-435. Set only when non-empty.
  if (req.mcpServers && req.mcpServers.length > 0) {
    result.mcp_servers = req.mcpServers.map((s) => {
      const obj: SerializedBlock = { type: s.type, url: s.url, name: s.name }
      if (s.authorizationToken !== undefined) {
        obj.authorization_token = s.authorizationToken
      }
      return obj
    })
  }

  if (req.providerOptions && typeof req.providerOptions === 'object') {
    Object.assign(result, req.providerOptions)
  }

  return result
}
