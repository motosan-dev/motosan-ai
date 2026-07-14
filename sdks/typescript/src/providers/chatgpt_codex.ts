/**
 * ChatGPT-Codex provider: streams the OpenAI Responses API at
 * `https://chatgpt.com/backend-api/codex/responses` using a caller-supplied
 * OAuth `accessToken` + `accountId` (codex CLI headers; no api key). Mirrors the
 * verified Python `chatgpt_codex.py` (a port of authoritative Rust
 * `chatgpt_codex.rs`) in idiomatic TS: a mid-stream `error` / `response.failed`
 * frame throws a `StreamError` (parity with Rust `MotosanError::Stream` and
 * the Python mid-stream `StreamError` raise).
 */

import { StreamError, isRetryableNetworkError, isRetryableStatus } from '../error.js'
import { postStream } from '../http/fetch.js'
import { parseSse } from '../http/sse.js'
import { DEFAULT_CHATGPT_CODEX_MODEL } from '../models.js'
import { textOnly, type ProviderCapabilities } from '../provider.js'
import { RetryPolicy } from '../retry.js'
import {
  collectStream,
  doneEvent,
  doneWithStopReason,
  textEvent,
  thinkingDelta,
  toolCallArgsWithId,
  toolCallEndWithId,
  toolCallStart,
  usageEvent,
  type BoxStream,
} from '../stream.js'
import type { ChatRequest, ChatResponse, StopReason, StreamEvent, Usage } from '../types.js'

export const DEFAULT_CHATGPT_CODEX_URL = 'https://chatgpt.com/backend-api/codex/responses'
const CHATGPT_CODEX_ORIGINATOR = 'codex_cli_rs'

// Re-exported here (load-bearing): the T1 unit test imports the default model
// from this provider module path.
export { DEFAULT_CHATGPT_CODEX_MODEL }

/**
 * Extract the error message from an `error` / `response.failed` Responses frame.
 * First non-empty wins: top-level `message` → `response.error.message` →
 * `error.message` → fallback. Used by the stream adapter to build the
 * `StreamError` for fatal frames. NOT re-exported from `src/index.ts`.
 *
 * @internal
 */
export function chatGptCodexErrorMessage(chunk: any): string {
  if (typeof chunk?.message === 'string' && chunk.message) return chunk.message
  const nested = chunk?.response?.error?.message
  if (typeof nested === 'string' && nested) return nested
  const top = chunk?.error?.message
  if (typeof top === 'string' && top) return top
  return 'ChatGPT-backend stream error'
}

/**
 * No-api-key OAuth-Bearer HTTP provider over the OpenAI Responses API.
 * Constructor `(accessToken, accountId, model?, baseUrl?)` mirrors Python
 * `ChatGptCodexProvider.__init__`. Text-only capabilities.
 */
export class ChatGptCodexProvider {
  private readonly model: string
  private readonly baseUrl: string
  private retryPolicy: RetryPolicy
  private _reasoningEffort?: string

  constructor(
    private readonly accessToken: string,
    private readonly accountId: string,
    model?: string,
    baseUrl: string = DEFAULT_CHATGPT_CODEX_URL,
  ) {
    this.model = model ?? DEFAULT_CHATGPT_CODEX_MODEL
    this.baseUrl = baseUrl
    this.retryPolicy = RetryPolicy.default()
  }

  withRetryPolicy(policy: RetryPolicy): this {
    this.retryPolicy = policy
    return this
  }

  /** Set the provider-default reasoning effort. Pass undefined to clear. Returns this. */
  reasoningEffort(effort: string | undefined): this {
    this._reasoningEffort = effort
    return this
  }

  /** Test/introspection accessor: the resolved default model id. */
  modelId(): string {
    return this.model
  }

  /** The full POST endpoint (base URL verbatim). */
  endpointUrl(): string {
    return this.baseUrl
  }

  capabilities(): ProviderCapabilities {
    return textOnly()
  }

  private headers(): Record<string, string> {
    return {
      authorization: `Bearer ${this.accessToken}`,
      'chatgpt-account-id': this.accountId,
      originator: CHATGPT_CODEX_ORIGINATOR,
      'openai-beta': 'responses=experimental',
      accept: 'text/event-stream',
      'content-type': 'application/json',
    }
  }

  /** Build the OpenAI Responses request body. Public for unit tests. */
  buildResponsesBody(request: ChatRequest, model: string): Record<string, any> {
    const instructionsParts: string[] = []
    if (request.systemBlocks !== undefined) {
      for (const block of request.systemBlocks) {
        const trimmed = block.text.trim()
        if (trimmed) instructionsParts.push(trimmed)
      }
    } else if (request.system !== undefined) {
      const trimmed = request.system.trim()
      if (trimmed) instructionsParts.push(trimmed)
    }

    const inputItems: Array<Record<string, any>> = []
    for (const message of request.messages) {
      switch (message.role) {
        case 'system': {
          const trimmed = message.content.trim()
          if (trimmed) instructionsParts.push(trimmed)
          break
        }
        case 'user':
          inputItems.push({
            type: 'message',
            role: 'user',
            content: [{ type: 'input_text', text: message.content }],
          })
          break
        case 'assistant': {
          if (message.content) {
            inputItems.push({
              type: 'message',
              role: 'assistant',
              content: [{ type: 'output_text', text: message.content }],
            })
          }
          for (const tc of message.toolCalls ?? []) {
            inputItems.push({
              type: 'function_call',
              call_id: tc.id,
              name: tc.name,
              arguments: JSON.stringify(tc.input),
            })
          }
          break
        }
        case 'tool':
          if (message.toolCallId !== undefined) {
            inputItems.push({
              type: 'function_call_output',
              call_id: message.toolCallId,
              output: message.content,
            })
          }
          break
      }
    }

    const instructions =
      instructionsParts.length > 0 ? instructionsParts.join('\n\n') : 'You are a helpful assistant.'

    const body: Record<string, any> = {
      model,
      store: false,
      stream: true,
      instructions,
      input: inputItems,
      include: ['reasoning.encrypted_content'],
      tool_choice: 'auto',
      parallel_tool_calls: true,
    }

    if (request.tools !== undefined) {
      const mapped = request.tools.map((tool) => ({
        type: 'function',
        name: tool.name,
        description: tool.description ?? null,
        parameters: tool.inputSchema ?? null,
        strict: null,
      }))
      if (mapped.length > 0) body.tools = mapped
    }

    // Reasoning effort: a per-request provider_options string value wins; else
    // the provider-level default; else the `reasoning` object is omitted.
    let effort: string | undefined
    const candidate = request.providerOptions?.reasoning_effort
    if (typeof candidate === 'string') effort = candidate
    if (effort === undefined) effort = this._reasoningEffort
    if (effort !== undefined) body.reasoning = { effort, summary: 'auto' }

    if (request.temperature !== undefined) body.temperature = request.temperature

    return body
  }

  async chat(request: ChatRequest): Promise<ChatResponse> {
    const model = request.model ?? this.model
    const response = await collectStream(this.stream(request))
    if (!response.model) response.model = model
    return response
  }

  stream(request: ChatRequest): BoxStream {
    return this.streamImpl(request)
  }

  private async *streamImpl(request: ChatRequest): AsyncGenerator<StreamEvent> {
    const model = request.model ?? this.model
    const body = this.buildResponsesBody(request, model)
    const headers = this.headers()

    // Retry ONLY the initial fetch (mirrors anthropic.ts:259-288 / ollama.ts:300-321).
    let attempt = 0
    let responseBody: ReadableStream<Uint8Array>
    while (true) {
      try {
        responseBody = await postStream(this.baseUrl, headers, body)
        break
      } catch (error) {
        const status = (error as { status?: number }).status
        const retryable =
          (status !== undefined && isRetryableStatus(status)) || isRetryableNetworkError(error)
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

    // Only `sawToolCall` drives the terminal stop_reason (parity with Rust/Python,
    // which also track a seen-ids set that is write-only — dropped here per plan R1).
    let sawToolCall = false

    // Fatal `error` / `response.failed` frames throw a StreamError (Rust/
    // Python parity). Other post-start body errors still end silently (M3).
    try {
      for await (const evt of parseSse(responseBody)) {
        const data = evt.data
        if (!data || data === '[DONE]' || typeof data !== 'object') continue

        switch (data.type) {
          case 'response.output_text.delta': {
            const delta = data.delta
            if (typeof delta === 'string' && delta) yield textEvent(delta)
            break
          }
          case 'response.reasoning_text.delta':
          case 'response.reasoning_summary_text.delta': {
            const delta = data.delta
            if (typeof delta === 'string' && delta) yield thinkingDelta(delta)
            break
          }
          case 'response.output_item.added': {
            const item = data.item
            if (item && item.type === 'function_call' && item.call_id) {
              sawToolCall = true
              yield toolCallStart(String(item.call_id), String(item.name ?? ''))
            }
            break
          }
          case 'response.function_call_arguments.delta': {
            const itemId = data.item_id
            const delta = data.delta
            if (itemId && typeof delta === 'string') {
              yield toolCallArgsWithId(String(itemId), delta)
            }
            break
          }
          case 'response.output_item.done': {
            const item = data.item
            if (item && item.type === 'function_call' && item.call_id) {
              yield toolCallEndWithId(String(item.call_id))
            }
            break
          }
          case 'response.completed': {
            const response =
              data.response && typeof data.response === 'object' ? data.response : {}
            const usage = response.usage
            if (usage && typeof usage === 'object') {
              const u: Usage = {
                inputTokens: Number(usage.input_tokens ?? 0),
                outputTokens: Number(usage.output_tokens ?? 0),
              }
              const cached = Number(usage.input_tokens_details?.cached_tokens ?? 0)
              if (cached > 0) u.cacheReadInputTokens = cached
              yield usageEvent(u)
            }
            const status = response.status ?? 'completed'
            const stop: StopReason = sawToolCall
              ? 'tool_use'
              : status === 'incomplete'
                ? 'max_tokens'
                : 'end_turn'
            yield doneWithStopReason(stop)
            return
          }
          case 'error':
          case 'response.failed':
            // Fatal stream error frame: surface it (Rust MotosanError::Stream
            // / Python StreamError parity).
            throw new StreamError(chatGptCodexErrorMessage(data))
          default:
            break
        }
      }
    } catch (error) {
      if (error instanceof StreamError) {
        throw error
      }
      // Ignore other post-start stream-body errors; end without a terminal
      // done (mirrors ollama.ts:362-366). Surfacing these is milestone M3.
      return
    }

    // Defensive terminal for a clean EOF without response.completed
    // (mirrors anthropic.ts:386-391). response.completed returns earlier.
    yield doneEvent()
  }
}
