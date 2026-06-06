import { isRetryableNetworkError, isRetryableStatus } from '../error.js'
import { postJson, postStream } from '../http/fetch.js'
import { parseSse } from '../http/sse.js'
import { DEFAULT_OPENAI_MODEL } from '../models.js'
import { withImage, type ProviderCapabilities } from '../provider.js'
import { RetryPolicy, withRetry, type RetryClassification } from '../retry.js'
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

const FINISH_REASON_MAP: Record<string, StopReason> = {
  stop: 'stop',
  length: 'max_tokens',
  tool_calls: 'tool_use',
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

export class OpenAIProvider {
  private readonly model: string
  private readonly baseUrl: string
  private retryPolicy: RetryPolicy

  constructor(
    private readonly apiKey: string,
    model?: string,
    baseUrl = 'https://api.openai.com/v1'
  ) {
    this.model = model ?? DEFAULT_OPENAI_MODEL
    this.baseUrl = baseUrl.replace(/\/$/, '') // trim trailing slash
    this.retryPolicy = RetryPolicy.default()
  }

  withRetryPolicy(policy: RetryPolicy): this {
    this.retryPolicy = policy
    return this
  }

  private headers(): Record<string, string> {
    return {
      authorization: `Bearer ${this.apiKey}`,
      'content-type': 'application/json',
    }
  }

  async chat(request: ChatRequest): Promise<ChatResponse> {
    const resolvedModel = request.model ?? this.model
    const body = serializeOpenAiRequest(request, resolvedModel)

    const payload = await withRetry(
      this.retryPolicy,
      async () =>
        postJson<any>(`${this.baseUrl}/chat/completions`, this.headers(), body),
      classifyHttpError,
    )

    const choice = payload?.choices?.[0] ?? {}
    const message = choice?.message ?? {}
    const content = String(message?.content ?? '')

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

  stream(request: ChatRequest): BoxStream {
    return this.streamImpl(request)
  }

  capabilities(): ProviderCapabilities {
    return withImage()
  }

  private async *streamImpl(request: ChatRequest) {
    const resolvedModel = request.model ?? this.model
    const body = serializeOpenAiRequest(request, resolvedModel)
    body.stream = true

    let attempt = 0
    let responseBody: ReadableStream<Uint8Array>
    while (true) {
      try {
        responseBody = await postStream(
          `${this.baseUrl}/chat/completions`,
          this.headers(),
          body,
        )
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

    // Defensive: EOF without [DONE] — emit terminal once.
    if (!doneEmitted) {
      // If a tool is still open (no finish_reason/[DONE] closed it), flush it.
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
  }
}
