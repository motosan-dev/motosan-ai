import {
  isRetryableNetworkError,
  isRetryableStatus,
  ProviderError,
} from '../error.js'
import { postJson, postStream } from '../http/fetch.js'
import { DEFAULT_ANTHROPIC_MODEL } from '../models.js'
import { parseSse } from '../http/sse.js'
import { fullCaps, type ProviderCapabilities } from '../provider.js'
import { RetryPolicy, withRetry, type RetryClassification } from '../retry.js'
import { modelUsesAdaptiveThinking, serializeAnthropicRequest } from '../serialize/anthropic.js'
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

/**
 * Build the `anthropic-beta` header value. Mirrors Rust `build_beta_header`
 * (anthropic.rs:78-99): comma-joined (no spaces), `undefined` when empty.
 * The M4 x-api-key path emits MCP and non-adaptive thinking betas directly;
 * OAuth-only setup-token betas are wired for the future OAuth path.
 */
export function buildBetaHeader(
  hasMcp: boolean,
  isOauth: boolean,
  adaptiveThinking: boolean,
  hasThinking = false,
): string | undefined {
  const betas: string[] = []
  if (hasMcp) {
    betas.push('mcp-client-2025-11-20')
  }
  if (isOauth) {
    betas.push('claude-code-20250219')
    betas.push('oauth-2025-04-20')
    betas.push('fine-grained-tool-streaming-2025-05-14')
  }
  if (hasThinking && !adaptiveThinking) {
    betas.push('interleaved-thinking-2025-05-14')
  }
  return betas.length === 0 ? undefined : betas.join(',')
}

/** Whether a request carries any MCP config. Mirrors Rust anthropic.rs:466-467. */
function requestHasMcp(req: ChatRequest): boolean {
  return (req.mcpServers?.length ?? 0) > 0 || (req.mcpToolConfigs?.length ?? 0) > 0
}

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

function classifyHttpError(result: unknown): RetryClassification {
  if (result instanceof Error) {
    const error = result as { status?: number; retryAfterMs?: number }
    const status = error.status
    if (
      (status !== undefined && isRetryableStatus(status)) ||
      isRetryableNetworkError(result)
    ) {
      return { retryable: true, retryAfterMs: error.retryAfterMs }
    }
    throw result
  }
  return { retryable: false }
}

export class AnthropicProvider {
  private readonly model: string
  private readonly baseUrl: string
  private retryPolicy: RetryPolicy

  constructor(
    private readonly apiKey: string,
    model?: string,
    baseUrl = 'https://api.anthropic.com',
  ) {
    this.model = model ?? DEFAULT_ANTHROPIC_MODEL
    this.baseUrl = baseUrl
    this.retryPolicy = RetryPolicy.default()
  }

  withRetryPolicy(policy: RetryPolicy): this {
    this.retryPolicy = policy
    return this
  }

  private headers(extra?: Record<string, string>): Record<string, string> {
    return {
      'x-api-key': this.apiKey,
      'anthropic-version': ANTHROPIC_VERSION,
      'content-type': 'application/json',
      ...(extra ?? {}),
    }
  }

  /**
   * Build per-request headers including the beta header when applicable.
   * isOauth is always false in M4 (no setup-token path in TS).
   * adaptiveThinking is read off the serialized body, matching Rust
   * (anthropic.rs:469/802). Non-adaptive thinking enables the interleaved-thinking beta.
   */
  private requestHeaders(req: ChatRequest, body: Record<string, any>): Record<string, string> {
    const hasMcp = requestHasMcp(req)
    const hasThinking = body?.thinking !== undefined
    const adaptiveThinking = body?.thinking?.type === 'adaptive'
    const beta = buildBetaHeader(hasMcp, false, adaptiveThinking, hasThinking)
    return this.headers(beta ? { 'anthropic-beta': beta } : {})
  }

  async chat(request: ChatRequest): Promise<ChatResponse> {
    const model = request.model ?? this.model
    const body = serializeAnthropicRequest(request, model)
    const headers = this.requestHeaders(request, body)
    const payload = await withRetry(
      this.retryPolicy,
      async () => postJson<any>(`${this.baseUrl}/v1/messages`, headers, body),
      classifyHttpError,
    )

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
    const model = request.model ?? this.model
    const body = {
      ...serializeAnthropicRequest(request, model),
      stream: true,
    }
    const headers = this.requestHeaders(request, body)
    let attempt = 0
    let responseBody: ReadableStream<Uint8Array>
    while (true) {
      try {
        responseBody = await postStream(`${this.baseUrl}/v1/messages`, headers, body)
        break
      } catch (error) {
        const status = (error as { status?: number }).status
        const retryable =
          (status !== undefined && isRetryableStatus(status)) ||
          isRetryableNetworkError(error)
        if (!retryable || attempt >= this.retryPolicy.maxRetries) {
          throw error
        }
        attempt += 1
        const retryAfterMs = (error as { retryAfterMs?: number }).retryAfterMs
        const delay = this.retryPolicy.respectRetryAfter
          ? retryAfterMs ?? this.retryPolicy.delayForAttempt(attempt)
          : this.retryPolicy.delayForAttempt(attempt)
        await new Promise((resolve) => setTimeout(resolve, delay))
      }
    }

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

  capabilities(): ProviderCapabilities {
    return fullCaps()
  }
}
