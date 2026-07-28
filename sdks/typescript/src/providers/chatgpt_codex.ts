/**
 * ChatGPT-Codex provider: streams the OpenAI Responses API at
 * `https://chatgpt.com/backend-api/codex/responses` using a caller-supplied
 * OAuth `accessToken` + `accountId` (codex CLI headers; no api key). Mirrors the
 * verified Python `chatgpt_codex.py` (a port of authoritative Rust
 * `chatgpt_codex.rs`) in idiomatic TS: a mid-stream `error` / `response.failed`
 * frame throws a `StreamError` (parity with Rust `MotosanError::Stream` and
 * the Python mid-stream `StreamError` raise).
 */

import { IncompleteStreamError, StreamError } from '../error.js'
import { postStream } from '../http/fetch.js'
import { parseSse } from '../http/sse.js'
import { DEFAULT_CHATGPT_CODEX_MODEL } from '../models.js'
import {
  validateModelRequest,
  withFreeformTools,
  type ProviderCapabilities,
  type ProviderRequestOptions,
} from '../provider.js'
import { attemptWithCancellation, classifyForRetry, RetryPolicy, withRetry } from '../retry.js'
import { buildModelRequestBody, modelStreamAdapter } from '../serialize/responses.js'
import {
  collectModelStream,
  collectStream,
  doneWithStopReason,
  textEvent,
  thinkingDelta,
  toolCallArgsWithId,
  toolCallEndWithId,
  toolCallStart,
  usageEvent,
  type BoxModelStream,
  type BoxStream,
} from '../stream.js'
import type {
  ChatRequest,
  ChatResponse,
  ModelChatRequest,
  ModelChatResponse,
  ModelStreamDelta,
  StopReason,
  StreamEvent,
  Usage,
} from '../types.js'

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
 * A caller-supplied async bearer-token source, consulted once per request
 * attempt. Each retry re-resolves it, so a refreshed OAuth access token is
 * picked up mid-retry.
 */
export type TokenSource = () => Promise<string>

/**
 * No-api-key OAuth-Bearer HTTP provider over the OpenAI Responses API.
 * Constructor `(accessToken, accountId, model?, baseUrl?)` mirrors Python
 * `ChatGptCodexProvider.__init__`; `accessToken` is a static string or an
 * async `TokenSource` resolved once per attempt. Text-only capabilities.
 */
export class ChatGptCodexProvider {
  private readonly model: string
  private readonly baseUrl: string
  private retryPolicy: RetryPolicy
  private _reasoningEffort?: string

  constructor(
    private readonly accessToken: string | TokenSource,
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
    return withFreeformTools()
  }

  /** Resolve the bearer token for one attempt: static string, or one TokenSource call. */
  private async resolveToken(): Promise<string> {
    return typeof this.accessToken === 'function' ? this.accessToken() : this.accessToken
  }

  private headers(token: string): Record<string, string> {
    return {
      authorization: `Bearer ${token}`,
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

  /** Build the native Responses request body. Public for unit tests. */
  buildModelResponsesBody(request: ModelChatRequest): Record<string, unknown> {
    const body = buildModelRequestBody(request, this.model, true, 'You are a helpful assistant.')
    body.store = false
    body.include = ['reasoning.encrypted_content']
    body.tool_choice = 'auto'
    body.parallel_tool_calls = true

    let effort: string | undefined
    const candidate = request.providerOptions?.reasoning_effort
    if (typeof candidate === 'string') effort = candidate
    if (effort === undefined) effort = this._reasoningEffort
    if (effort !== undefined) {
      body.reasoning = { effort, summary: 'auto' }
      delete body.reasoning_effort
    }

    return body
  }

  async chat(request: ChatRequest, opts?: ProviderRequestOptions): Promise<ChatResponse> {
    const model = request.model ?? this.model
    const response = await collectStream(this.stream(request, opts))
    if (!response.model) response.model = model
    return response
  }

  stream(request: ChatRequest, opts?: ProviderRequestOptions): BoxStream {
    return this.streamImpl(request, opts)
  }

  private async *streamImpl(
    request: ChatRequest,
    opts?: ProviderRequestOptions,
  ): AsyncGenerator<StreamEvent> {
    const model = request.model ?? this.model
    const body = this.buildResponsesBody(request, model)

    // Retry ONLY the initial fetch via the shared engine (same guard as the
    // other providers: nothing is retried after the first emitted event).
    // Headers are rebuilt inside the attempt closure so a TokenSource is
    // re-resolved on every attempt while the shared retry engine stays intact.
    const responseBody = await withRetry(
      this.retryPolicy,
      async () =>
        attemptWithCancellation(opts?.callerSignal, async () =>
          postStream(this.baseUrl, this.headers(await this.resolveToken()), body, {
            signal: opts?.signal,
            preHeadersTimeoutMs: opts?.preHeadersTimeoutMs,
          }),
        ),
      classifyForRetry,
    )

    // Only `sawToolCall` drives the terminal stop_reason (parity with Rust/Python,
    // which also track a seen-ids set that is write-only — dropped here per plan R1).
    let sawToolCall = false

    // Real wire: output_item.added carries BOTH item.id ("fc_…") and call_id
    // ("call_…"), but function_call_arguments.delta frames are keyed by item_id
    // only. Map item.id → call_id so every tool event carries the call_id.
    const itemIdToCallId = new Map<string, string>()

    // Fatal `error` / `response.failed` frames throw a StreamError; other
    // body errors propagate (M3 removed the swallow).
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
            if (item.id) itemIdToCallId.set(String(item.id), String(item.call_id))
            yield toolCallStart(String(item.call_id), String(item.name ?? ''))
          }
          break
        }
        case 'response.function_call_arguments.delta': {
          const itemId = data.item_id
          const delta = data.delta
          if (itemId && typeof delta === 'string') {
            const callId = itemIdToCallId.get(String(itemId)) ?? String(itemId)
            yield toolCallArgsWithId(callId, delta)
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

    // EOF without response.completed: truncation, not completion (M3/E2/E3 —
    // the defensive doneEvent() is retired). chat() = collectStream(stream()),
    // so a truncated chat() now rejects with IncompleteStreamError too.
    throw new IncompleteStreamError(
      'incomplete stream: chatgpt_codex ended without a terminal event',
    )
  }

  async modelChat(
    request: ModelChatRequest,
    opts?: ProviderRequestOptions,
  ): Promise<ModelChatResponse> {
    const model = request.model ?? this.model
    const response = await collectModelStream(this.modelStream(request, opts))
    if (!response.model) response.model = model
    return response
  }

  modelStream(request: ModelChatRequest, opts?: ProviderRequestOptions): BoxModelStream {
    validateModelRequest(request, this.capabilities())
    return this.modelStreamImpl(request, opts)
  }

  private async *modelStreamImpl(
    request: ModelChatRequest,
    opts?: ProviderRequestOptions,
  ): AsyncGenerator<ModelStreamDelta> {
    const body = this.buildModelResponsesBody(request)
    const responseBody = await withRetry(
      this.retryPolicy,
      async () =>
        attemptWithCancellation(opts?.callerSignal, async () =>
          postStream(this.baseUrl, this.headers(await this.resolveToken()), body, {
            signal: opts?.signal,
            preHeadersTimeoutMs: opts?.preHeadersTimeoutMs,
          }),
        ),
      classifyForRetry,
    )
    yield* modelStreamAdapter(responseBody, 'chatgpt-codex')
  }
}
