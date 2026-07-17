import { IncompleteStreamError } from '../error.js'
import { postJson, postStream } from '../http/fetch.js'
import { parseSse } from '../http/sse.js'
import { DEFAULT_OPENAI_MODEL } from '../models.js'
import { withImage, type ProviderCapabilities, type ProviderRequestOptions } from '../provider.js'
import { attemptWithCancellation, classifyForRetry, RetryPolicy, withRetry } from '../retry.js'
import { serializeOpenAiRequest } from '../serialize/openai.js'
import {
  doneEvent,
  doneWithStopReason,
  textEvent,
  toolCallArgsWithId,
  toolCallEndWithId,
  toolCallStart,
  usageEvent,
  type BoxStream,
} from '../stream.js'
import type { ChatRequest, ChatResponse, StopReason, ToolCall } from '../types.js'

/** OpenAI authentication style. Mirrors Rust `OpenAIAuthStyle`. */
export type OpenAIAuthStyle =
  | { kind: 'bearer' }
  | { kind: 'xApiKey' }
  | { kind: 'custom'; header: string }

/** Default chat-completions endpoint. */
export const DEFAULT_OPENAI_CHAT_URL = 'https://api.openai.com/v1/chat/completions'

/** Default Responses-API endpoint. */
export const DEFAULT_OPENAI_RESPONSES_URL = 'https://api.openai.com/v1/responses'

const FINISH_REASON_MAP: Record<string, StopReason> = {
  stop: 'stop',
  length: 'max_tokens',
  tool_calls: 'tool_use',
}

export class OpenAIProvider {
  private readonly model: string
  private readonly baseUrl: string
  private retryPolicy: RetryPolicy
  private authStyle: OpenAIAuthStyle = { kind: 'bearer' }
  private chatUrl: string = DEFAULT_OPENAI_CHAT_URL
  private responsesUrl: string = DEFAULT_OPENAI_RESPONSES_URL
  private responsesFallback = false

  constructor(
    private readonly apiKey: string,
    model?: string,
    baseUrl = 'https://api.openai.com/v1'
  ) {
    this.model = model ?? DEFAULT_OPENAI_MODEL
    this.baseUrl = baseUrl.replace(/\/+$/, '') // trim trailing slash(es), like the URL setters
    this.retryPolicy = RetryPolicy.default()
    if (this.baseUrl !== 'https://api.openai.com/v1') {
      this.chatUrl = `${this.baseUrl}/chat/completions`
      this.responsesUrl = `${this.baseUrl}/responses`
    }
  }

  withRetryPolicy(policy: RetryPolicy): this {
    this.retryPolicy = policy
    return this
  }

  withAuthStyle(style: OpenAIAuthStyle): this {
    this.authStyle = style
    return this
  }

  withChatUrl(url: string): this {
    this.chatUrl = url.replace(/\/+$/, '')
    return this
  }

  withResponsesFallback(enabled: boolean): this {
    this.responsesFallback = enabled
    return this
  }

  withResponsesUrl(url: string): this {
    this.responsesUrl = url.replace(/\/+$/, '')
    return this
  }

  private applyAuth(headers: Record<string, string>): Record<string, string> {
    const result = { ...headers }
    switch (this.authStyle.kind) {
      case 'bearer':
        result.authorization = `Bearer ${this.apiKey}`
        break
      case 'xApiKey':
        result['x-api-key'] = this.apiKey
        break
      case 'custom':
        result[this.authStyle.header] = this.apiKey
        break
    }
    return result
  }

  private headers(): Record<string, string> {
    return this.applyAuth({ 'content-type': 'application/json' })
  }

  private extractResponsesText(payload: any): string {
    const outputText = payload?.output_text
    if (typeof outputText === 'string' && outputText.trim()) {
      return outputText.trim()
    }

    const output = payload?.output
    if (Array.isArray(output) && output.length > 0) {
      const first = output[0]
      const content =
        first && typeof first === 'object'
          ? (first as { content?: unknown }).content
          : undefined
      if (Array.isArray(content)) {
        for (const item of content) {
          if (item && typeof item === 'object') {
            const text = (item as { text?: unknown }).text
            if (typeof text === 'string' && text.trim()) {
              return text.trim()
            }
            const nested = (item as { content?: unknown }).content
            if (Array.isArray(nested)) {
              for (const nestedItem of nested) {
                if (nestedItem && typeof nestedItem === 'object') {
                  const nestedText = (nestedItem as { text?: unknown }).text
                  if (typeof nestedText === 'string' && nestedText.trim()) {
                    return nestedText.trim()
                  }
                }
              }
            }
          }
        }
      }
    }

    return ''
  }

  private async chatViaResponses(
    request: ChatRequest,
    opts?: ProviderRequestOptions,
  ): Promise<ChatResponse> {
    const model = request.model ?? this.model
    const instructionsParts: string[] = []

    if (request.systemBlocks) {
      for (const block of request.systemBlocks) {
        const trimmed = block.text.trim()
        if (trimmed) instructionsParts.push(trimmed)
      }
    } else if (request.system) {
      const trimmed = request.system.trim()
      if (trimmed) instructionsParts.push(trimmed)
    }

    const input: unknown[] = []
    for (const message of request.messages) {
      switch (message.role) {
        case 'system': {
          const trimmed = message.content.trim()
          if (trimmed) instructionsParts.push(trimmed)
          break
        }
        case 'assistant': {
          if (message.toolCalls && message.toolCalls.length > 0) {
            const toolCalls = message.toolCalls.map((tc) => ({
              id: tc.id,
              type: 'function',
              function: { name: tc.name, arguments: JSON.stringify(tc.input) },
            }))
            input.push({
              role: 'assistant',
              content: message.content,
              tool_calls: toolCalls,
            })
          } else {
            input.push({ role: 'assistant', content: message.content })
          }
          break
        }
        case 'user': {
          input.push({ role: 'user', content: message.content })
          break
        }
        case 'tool': {
          if (message.toolCallId) {
            input.push({
              role: 'tool',
              tool_call_id: message.toolCallId,
              content: message.content,
            })
          }
          break
        }
      }
    }

    const body: Record<string, unknown> = { model, input }
    if (instructionsParts.length > 0) {
      body.instructions = instructionsParts.join('\n\n')
    }
    if (request.providerOptions) {
      for (const [key, value] of Object.entries(request.providerOptions)) {
        body[key] = value
      }
    }

    const payload = await withRetry(
      this.retryPolicy,
      async () =>
        attemptWithCancellation(opts?.callerSignal, () =>
          postJson<any>(this.responsesUrl, this.headers(), body, {
            signal: opts?.signal,
            preHeadersTimeoutMs: opts?.preHeadersTimeoutMs,
          }),
        ),
      classifyForRetry,
    )
    const content = this.extractResponsesText(payload)
    const responseModel =
      typeof payload?.model === 'string' ? payload.model : DEFAULT_OPENAI_MODEL
    const usage = payload?.usage ?? {}
    const inputTokens =
      typeof usage?.input_tokens === 'number'
        ? usage.input_tokens
        : typeof usage?.prompt_tokens === 'number'
          ? usage.prompt_tokens
          : 0
    const outputTokens =
      typeof usage?.output_tokens === 'number'
        ? usage.output_tokens
        : typeof usage?.completion_tokens === 'number'
          ? usage.completion_tokens
          : 0

    return {
      content,
      toolCalls: [],
      model: responseModel,
      usage: { inputTokens: Number(inputTokens), outputTokens: Number(outputTokens) },
      stopReason: 'stop',
    }
  }

  async chat(request: ChatRequest, opts?: ProviderRequestOptions): Promise<ChatResponse> {
    const resolvedModel = request.model ?? this.model
    const body = serializeOpenAiRequest(request, resolvedModel)

    let payload: any
    try {
      payload = await withRetry(
        this.retryPolicy,
        async () =>
          attemptWithCancellation(opts?.callerSignal, () =>
            postJson<any>(this.chatUrl, this.headers(), body, {
              signal: opts?.signal,
              preHeadersTimeoutMs: opts?.preHeadersTimeoutMs,
            }),
          ),
        classifyForRetry,
      )
    } catch (error) {
      if (
        this.responsesFallback &&
        error instanceof Error &&
        (error as { status?: number }).status === 404
      ) {
        return this.chatViaResponses(request, opts)
      }
      throw error
    }

    const choice = payload?.choices?.[0] ?? {}
    const message = choice?.message ?? {}
    // Fall back to reasoning_content when content is empty/whitespace, matching
    // Rust extract_chat_content (content.or(reasoning)) and the stream path.
    const rawContent = String(message?.content ?? '')
    const content =
      rawContent.trim() !== '' ? rawContent : String(message?.reasoning_content ?? '')

    const toolCalls: ToolCall[] = (message?.tool_calls ?? []).map((tc: any) => {
      const args = String(tc?.function?.arguments ?? '{}')
      let input: Record<string, unknown> = {}
      try {
        input = JSON.parse(args)
      } catch {
        input = {}
      }
      return {
        id: String(tc?.id ?? ''),
        name: String(tc?.function?.name ?? ''),
        input,
      }
    })

    const stopReason =
      FINISH_REASON_MAP[String(choice?.finish_reason ?? '')] ?? 'other'

    return {
      content,
      toolCalls,
      model: String(payload?.model ?? resolvedModel),
      usage: {
        inputTokens: Number(payload?.usage?.prompt_tokens ?? 0),
        outputTokens: Number(payload?.usage?.completion_tokens ?? 0),
      },
      stopReason,
    }
  }

  stream(request: ChatRequest, opts?: ProviderRequestOptions): BoxStream {
    return this.streamImpl(request, opts)
  }

  capabilities(): ProviderCapabilities {
    return withImage()
  }

  private async *streamImpl(request: ChatRequest, opts?: ProviderRequestOptions) {
    const resolvedModel = request.model ?? this.model
    const body = serializeOpenAiRequest(request, resolvedModel)
    body.stream = true

    // Retry ONLY the initial fetch; no retry after the first emitted event.
    const responseBody = await withRetry(
      this.retryPolicy,
      async () =>
        attemptWithCancellation(opts?.callerSignal, () =>
          postStream(this.chatUrl, this.headers(), body, {
            signal: opts?.signal,
            preHeadersTimeoutMs: opts?.preHeadersTimeoutMs,
          }),
        ),
      classifyForRetry,
    )

    // Per-index tool-call tracking (only one tool open at a time for collectStream).
    const toolBuffer: Map<number, { id: string; name: string }> = new Map()
    let openToolIndex: number | undefined
    let pendingStopReason: StopReason | undefined
    let doneEmitted = false

    for await (const evt of parseSse(responseBody)) {
      const data = evt.data

      // [DONE] sentinel
      if (data === '[DONE]') {
        if (!doneEmitted) {
          // Flush any still-open tool (the provider sent no finish_reason).
          if (openToolIndex !== undefined) {
            const openId = toolBuffer.get(openToolIndex)?.id
            if (openId) {
              yield toolCallEndWithId(openId)
            }
            openToolIndex = undefined
          }
          doneEmitted = true
          yield pendingStopReason !== undefined
            ? doneWithStopReason(pendingStopReason)
            : doneEvent()
        }
        break
      }

      if (!data || typeof data !== 'object') continue

      const choice = data?.choices?.[0]
      if (!choice) continue

      const delta = choice?.delta
      if (!delta) continue

      // Text content (fall back to reasoning_content). No trimming — preserve
      // whitespace exactly, matching Rust's is_empty() check (openai.rs).
      const content = typeof delta.content === 'string' ? delta.content : ''
      const reasoning =
        typeof delta.reasoning_content === 'string' ? delta.reasoning_content : ''
      const text = content !== '' ? content : reasoning
      if (text !== '') {
        yield textEvent(text)
      }

      // Tool calls (indexed per spec).
      if (Array.isArray(delta.tool_calls)) {
        for (const tc of delta.tool_calls) {
          const tcIndex = tc?.index
          if (tcIndex === undefined) continue

          const tcId = tc?.id
          const tcName = tc?.function?.name
          const tcArgs = tc?.function?.arguments

          // First delta for this index: has id + name.
          if (tcId && tcName) {
            // Close any open tool from a different index.
            if (openToolIndex !== undefined && openToolIndex !== tcIndex) {
              const openId = toolBuffer.get(openToolIndex)?.id
              if (openId) {
                yield toolCallEndWithId(openId)
              }
            }

            // Open this tool.
            toolBuffer.set(tcIndex, { id: tcId, name: tcName })
            openToolIndex = tcIndex
            yield toolCallStart(tcId, tcName)
          }

          // Arguments fragment.
          if (tcArgs) {
            const bufferedTool = toolBuffer.get(tcIndex)
            if (bufferedTool) {
              yield toolCallArgsWithId(bufferedTool.id, tcArgs)
            }
          }
        }
      }

      // Stash finish_reason for the terminal done event.
      if (choice?.finish_reason) {
        pendingStopReason =
          FINISH_REASON_MAP[String(choice.finish_reason)] ?? 'other'

        // If finish_reason is tool_calls, close the open tool now.
        if (choice.finish_reason === 'tool_calls' && openToolIndex !== undefined) {
          const openId = toolBuffer.get(openToolIndex)?.id
          if (openId) {
            yield toolCallEndWithId(openId)
            openToolIndex = undefined
          }
        }
      }

      // Usage event (if present in final chunk with stream_options).
      const usage = data?.usage
      if (usage) {
        yield usageEvent({
          inputTokens: Number(usage?.prompt_tokens ?? 0),
          outputTokens: Number(usage?.completion_tokens ?? 0),
        })
      }
    }

    // EOF without the [DONE] sentinel (M3, amended): a stashed finish_reason
    // means the stream is SEMANTICALLY complete — [DONE] is only the
    // transport epilogue — so emit the terminal done carrying it, open-tool
    // flush included (mirrors the [DONE] branch; narrow survival of the
    // pre-M3 EOF fabrication, ONLY when finish_reason was seen). EOF with
    // NEITHER signal is truncation: the no-signal fabrication is retired.
    if (!doneEmitted) {
      if (pendingStopReason !== undefined) {
        if (openToolIndex !== undefined) {
          const openId = toolBuffer.get(openToolIndex)?.id
          if (openId) {
            yield toolCallEndWithId(openId)
          }
          openToolIndex = undefined
        }
        doneEmitted = true
        yield doneWithStopReason(pendingStopReason)
        return
      }
      throw new IncompleteStreamError('incomplete stream: openai ended without a terminal event')
    }
  }
}
