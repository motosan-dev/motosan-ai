import { IncompleteStreamError, ProviderError, StreamError } from '../error.js'
import { postJson, postStream } from '../http/fetch.js'
import { DEFAULT_ANTHROPIC_MODEL } from '../models.js'
import { parseSse } from '../http/sse.js'
import { fullCaps, type ProviderCapabilities, type ProviderRequestOptions } from '../provider.js'
import { attemptWithCancellation, classifyForRetry, RetryPolicy, withRetry } from '../retry.js'
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
const ANTHROPIC_CLAUDE_CODE_USER_AGENT = 'claude-code/1.0.33'

function isSetupToken(apiKey: string): boolean {
  return apiKey.startsWith('sk-ant-oat01-')
}

function claudeCodeSystemBlock(): Record<string, unknown> {
  return {
    type: 'text',
    text: "You are Claude Code, Anthropic's official CLI for Claude.",
    cache_control: { type: 'ephemeral' },
  }
}

function withOAuthSystemIdentity(body: Record<string, any>): Record<string, any> {
  if (Array.isArray(body.system)) {
    return { ...body, system: [claudeCodeSystemBlock(), ...body.system] }
  }

  if (typeof body.system === 'string') {
    return { ...body, system: [claudeCodeSystemBlock(), { type: 'text', text: body.system }] }
  }

  if (body.system && typeof body.system === 'object') {
    return { ...body, system: [claudeCodeSystemBlock(), body.system] }
  }

  return { ...body, system: [claudeCodeSystemBlock()] }
}

/**
 * Build the `anthropic-beta` header value (comma-joined, no spaces; `undefined`
 * when empty).
 *
 * INTENTIONAL DIVERGENCE from the Rust reference: Rust (`anthropic.rs:78-99`)
 * gates `interleaved-thinking-2025-05-14` inside the OAuth branch. We instead
 * emit it on the x-api-key path whenever non-adaptive thinking is requested,
 * matching independent TS SDK practice (earendil-works/pi
 * `packages/ai/src/providers/anthropic.ts:792-799`, which adds the beta for
 * `interleavedThinking && !forceAdaptiveThinking` regardless of auth mode, and
 * defaults `interleavedThinking` to true). The beta is a GA Anthropic beta
 * accepted on standard API-key requests, so this is non-breaking. Adaptive
 * models have interleaved thinking built in, so the beta is skipped for them.
 * OAuth setup-token betas remain wired for the future OAuth path.
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

export class AnthropicProvider {
  private readonly model: string
  private readonly baseUrl: string
  private readonly providerName: string
  private retryPolicy: RetryPolicy

  constructor(
    private readonly apiKey: string,
    model?: string,
    baseUrl = 'https://api.anthropic.com',
    providerName = 'anthropic',
  ) {
    this.model = model ?? DEFAULT_ANTHROPIC_MODEL
    this.baseUrl = baseUrl
    this.providerName = providerName
    this.retryPolicy = RetryPolicy.default()
  }

  withRetryPolicy(policy: RetryPolicy): this {
    this.retryPolicy = policy
    return this
  }

  private headers(extra?: Record<string, string>): Record<string, string> {
    if (isSetupToken(this.apiKey)) {
      return {
        authorization: `Bearer ${this.apiKey}`,
        'anthropic-version': ANTHROPIC_VERSION,
        'content-type': 'application/json',
        'user-agent': ANTHROPIC_CLAUDE_CODE_USER_AGENT,
        'x-app': 'cli',
        ...(extra ?? {}),
      }
    }

    return {
      'x-api-key': this.apiKey,
      'anthropic-version': ANTHROPIC_VERSION,
      'content-type': 'application/json',
      ...(extra ?? {}),
    }
  }

  /**
   * Build per-request headers including the beta header when applicable.
   * adaptiveThinking is read off the serialized body, matching Rust
   * (anthropic.rs:469/802). Non-adaptive thinking enables the interleaved-thinking beta.
   */
  private requestHeaders(req: ChatRequest, body: Record<string, any>): Record<string, string> {
    const hasMcp = requestHasMcp(req)
    const hasThinking = body?.thinking !== undefined
    const adaptiveThinking = body?.thinking?.type === 'adaptive'
    const beta = buildBetaHeader(hasMcp, isSetupToken(this.apiKey), adaptiveThinking, hasThinking)
    return this.headers(beta ? { 'anthropic-beta': beta } : {})
  }

  async chat(request: ChatRequest, opts?: ProviderRequestOptions): Promise<ChatResponse> {
    const model = request.model ?? this.model
    const serialized = serializeAnthropicRequest(request, model)
    const body = isSetupToken(this.apiKey) ? withOAuthSystemIdentity(serialized) : serialized
    const headers = this.requestHeaders(request, body)
    const payload = await withRetry(
      this.retryPolicy,
      async () =>
        attemptWithCancellation(opts?.callerSignal, () =>
          postJson<any>(`${this.baseUrl}/v1/messages`, headers, body, {
            signal: opts?.signal,
            preHeadersTimeoutMs: opts?.preHeadersTimeoutMs,
          }),
        ),
      classifyForRetry,
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

  stream(request: ChatRequest, opts?: ProviderRequestOptions): BoxStream {
    return this.streamImpl(request, opts)
  }

  private async *streamImpl(request: ChatRequest, opts?: ProviderRequestOptions) {
    const model = request.model ?? this.model
    const serialized = {
      ...serializeAnthropicRequest(request, model),
      stream: true,
    }
    const body = isSetupToken(this.apiKey) ? withOAuthSystemIdentity(serialized) : serialized
    const headers = this.requestHeaders(request, body)
    // Retry ONLY the initial fetch. parseSse below runs outside withRetry, so
    // nothing is retried after the first emitted event (pinned by
    // tests/retry-integration.test.ts "does not retry after the response body
    // has been returned").
    const responseBody = await withRetry(
      this.retryPolicy,
      async () =>
        attemptWithCancellation(opts?.callerSignal, () =>
          postStream(`${this.baseUrl}/v1/messages`, headers, body, {
            signal: opts?.signal,
            preHeadersTimeoutMs: opts?.preHeadersTimeoutMs,
          }),
        ),
      classifyForRetry,
    )

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

        case 'error': {
          // Anthropic emits `event: error` frames mid-stream on an HTTP 200
          // (e.g. overloaded_error). Surface them as a StreamError instead
          // of swallowing them and fabricating a clean done at EOF.
          const errType = String(data?.error?.type ?? 'error')
          const errMessage = String(data?.error?.message ?? 'unknown')
          throw new StreamError(`anthropic stream error (${errType}): ${errMessage}`)
        }

        default:
          // ping / unknown events are ignored.
          break
      }
    }

    // EOF without message_stop: truncation, not completion (M3/E3 — the
    // fabricated clean done is retired). A message_delta stop_reason alone
    // is NOT terminal; only message_stop is.
    throw new IncompleteStreamError(
      `incomplete stream: ${this.providerName} ended without a terminal event`,
    )
  }

  capabilities(): ProviderCapabilities {
    return fullCaps()
  }
}
