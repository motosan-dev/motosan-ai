import { ProviderError } from '../error.js'
import { postJson, postStream } from '../http/fetch.js'
import { parseSse } from '../http/sse.js'
import { serializeAnthropicRequest } from '../serialize/anthropic.js'
import {
  doneEvent,
  doneWithStopReason,
  textEvent,
  thinkingDelta,
  thinkingDone,
  toolCallArgsWithId,
  toolCallEndWithId,
  toolCallStart,
  usageEvent,
  type BoxStream,
} from '../stream.js'
import type {
  ChatRequest,
  ChatResponse,
  StopReason,
  ToolCall,
  Usage,
} from '../types.js'

const ANTHROPIC_VERSION = '2023-06-01'

function parseStopReason(reason: unknown): StopReason {
  switch (reason) {
    case 'end_turn':
      return 'end_turn'
    case 'max_tokens':
      return 'max_tokens'
    case 'tool_use':
      return 'tool_use'
    case 'stop_sequence':
      return 'stop_sequence'
    case 'stop':
      return 'stop'
    default:
      return 'other'
  }
}

/** Build a Usage object, omitting cache fields unless explicitly provided. */
function toUsage(raw: any, includeCache: boolean): Usage {
  const usage: Usage = {
    inputTokens: Number(raw?.input_tokens ?? 0),
    outputTokens: Number(raw?.output_tokens ?? 0),
  }
  if (includeCache) {
    if (raw?.cache_creation_input_tokens != null) {
      usage.cacheCreationInputTokens = Number(raw.cache_creation_input_tokens)
    }
    if (raw?.cache_read_input_tokens != null) {
      usage.cacheReadInputTokens = Number(raw.cache_read_input_tokens)
    }
  }
  return usage
}

interface StreamState {
  currentToolId?: string
  currentThinkingBuf?: string
  stopReason?: StopReason
}

export class AnthropicProvider {
  private readonly model: string
  private readonly baseUrl: string

  constructor(
    private readonly apiKey: string,
    model?: string,
    baseUrl = 'https://api.anthropic.com',
  ) {
    this.model = model ?? 'claude-3-5-sonnet-20241022'
    this.baseUrl = baseUrl
  }

  private headers(): Record<string, string> {
    return {
      'x-api-key': this.apiKey,
      'anthropic-version': ANTHROPIC_VERSION,
      'content-type': 'application/json',
    }
  }

  async chat(request: ChatRequest): Promise<ChatResponse> {
    const body = serializeAnthropicRequest(request, request.model ?? this.model)
    const payload = await postJson<any>(`${this.baseUrl}/v1/messages`, this.headers(), body)

    const blocks: any[] = Array.isArray(payload?.content) ? payload.content : []

    const content = blocks
      .filter((b) => b?.type === 'text')
      .map((b) => String(b?.text ?? ''))
      .join('')

    const thinking =
      blocks
        .filter((b) => b?.type === 'thinking')
        .map((b) => String(b?.thinking ?? ''))
        .join('') || undefined

    const toolCalls: ToolCall[] = blocks
      .filter((b) => b?.type === 'tool_use')
      .map((b) => ({
        id: String(b?.id ?? ''),
        name: String(b?.name ?? ''),
        input: (b?.input ?? {}) as Record<string, unknown>,
      }))
      .filter((tc) => tc.id && tc.name)

    return {
      content,
      ...(thinking ? { thinking } : {}),
      toolCalls,
      model: String(payload?.model ?? this.model),
      usage: toUsage(payload?.usage ?? {}, true),
      stopReason: parseStopReason(payload?.stop_reason),
    }
  }

  stream(request: ChatRequest): BoxStream {
    return this.streamImpl(request)
  }

  private async *streamImpl(request: ChatRequest) {
    const body = {
      ...serializeAnthropicRequest(request, request.model ?? this.model),
      stream: true,
    }
    const responseBody = await postStream(`${this.baseUrl}/v1/messages`, this.headers(), body)

    const state: StreamState = {}

    for await (const evt of parseSse(responseBody)) {
      const data = evt.data
      if (!data) continue

      switch (evt.event) {
        case 'message_start': {
          const usage = data?.message?.usage
          if (usage) {
            yield usageEvent(toUsage(usage, true))
          }
          break
        }

        case 'content_block_start': {
          const block = data?.content_block
          if (block?.type === 'tool_use') {
            const id = String(block.id ?? '')
            const name = String(block.name ?? '')
            state.currentToolId = id
            yield toolCallStart(id, name)
          } else if (block?.type === 'thinking') {
            state.currentThinkingBuf = ''
          }
          // redacted_thinking and text blocks: nothing to emit on start.
          break
        }

        case 'content_block_delta': {
          const delta = data?.delta
          if (!delta) break

          if (delta.type === 'text_delta') {
            const text = String(delta.text ?? '')
            if (text) yield textEvent(text)
          } else if (delta.type === 'input_json_delta') {
            const partial = String(delta.partial_json ?? '')
            if (partial && state.currentToolId !== undefined) {
              yield toolCallArgsWithId(state.currentToolId, partial)
            }
          } else if (delta.type === 'thinking_delta') {
            const text = String(delta.thinking ?? '')
            if (text) {
              if (state.currentThinkingBuf !== undefined) {
                state.currentThinkingBuf += text
              }
              yield thinkingDelta(text)
            }
          }
          // signature_delta and any other delta types are ignored.
          break
        }

        case 'content_block_stop': {
          if (state.currentToolId !== undefined) {
            const id = state.currentToolId
            state.currentToolId = undefined
            yield toolCallEndWithId(id)
          }
          if (state.currentThinkingBuf !== undefined) {
            const buf = state.currentThinkingBuf
            state.currentThinkingBuf = undefined
            // Only emit thinkingDone for a block that produced content; an empty
            // thinking block yields no event (collectStream maps it to undefined).
            if (buf.length > 0) {
              yield thinkingDone(buf)
            }
          }
          break
        }

        case 'message_delta': {
          const reason = data?.delta?.stop_reason
          if (reason) state.stopReason = parseStopReason(reason)
          const usage = data?.usage
          if (usage) {
            // message_delta usage carries NO cache tokens.
            yield usageEvent(toUsage(usage, false))
          }
          break
        }

        case 'message_stop': {
          yield state.stopReason !== undefined
            ? doneWithStopReason(state.stopReason)
            : doneEvent()
          return
        }

        default:
          // ping / unknown events are ignored.
          break
      }
    }

    // Defensive: terminate even if message_stop never arrived.
    if (state.stopReason !== undefined) {
      yield doneWithStopReason(state.stopReason)
    } else {
      yield doneEvent()
    }
  }

  capabilities(): { supportsImage: boolean; supportsDocument: boolean } {
    return { supportsImage: true, supportsDocument: true }
  }
}
