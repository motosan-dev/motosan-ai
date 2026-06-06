import {
  isRetryableNetworkError,
  isRetryableStatus,
  NetworkError,
  ProviderError,
  mapHttpError,
} from '../error.js'
import { DEFAULT_MINIMAX_MODEL } from '../models.js'
import { textOnly, type ProviderCapabilities } from '../provider.js'
import { RetryPolicy, withRetry, type RetryClassification } from '../retry.js'
import { serializeOpenAiRequest } from '../serialize/openai.js'
import type { ChatRequest, ChatResponse, StreamEvent } from '../types.js'

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

export class MinimaxProvider {
  private model: string
  private endpoint: string
  private retryPolicy: RetryPolicy

  constructor(
    private readonly apiKey: string,
    model?: string,
    endpoint?: string,
    private readonly fetcher: typeof fetch = fetch
  ) {
    this.model = model ?? DEFAULT_MINIMAX_MODEL
    this.endpoint = endpoint ?? 'https://api.minimax.chat/v1/text/chatcompletion_v2'
    this.retryPolicy = RetryPolicy.default()
  }

  withRetryPolicy(policy: RetryPolicy): this {
    this.retryPolicy = policy
    return this
  }

  async chat(request: ChatRequest): Promise<ChatResponse> {
    const resolvedModel = request.model ?? this.model
    const body = serializeOpenAiRequest(request, resolvedModel)

    try {
      return await withRetry(
        this.retryPolicy,
        async () => {
          const response = await this.fetcher(this.endpoint, {
            method: 'POST',
            headers: {
              authorization: `Bearer ${this.apiKey}`,
              'content-type': 'application/json',
            },
            body: JSON.stringify(body),
          })

          const text = await response.text()
          let payload: any = {}
          try {
            payload = text ? JSON.parse(text) : {}
          } catch {
            payload = {}
          }

          if (!response.ok) {
            const message = String(
              payload?.error?.message ?? text ?? 'minimax request failed',
            )
            throw mapHttpError(response.status, message, response.headers.get('retry-after'))
          }

          const choice = payload?.choices?.[0] ?? {}
          const msg = choice?.message ?? {}
          const toolCalls = (msg?.tool_calls ?? []).map((tc: any) => {
            const args = String(tc?.function?.arguments ?? '{}')
            let parsed = {}
            try {
              parsed = JSON.parse(args)
            } catch {
              parsed = {}
            }
            return {
              id: String(tc?.id ?? ''),
              name: String(tc?.function?.name ?? ''),
              input: parsed,
            }
          })

          const stopReason: ChatResponse['stopReason'] =
            choice?.finish_reason === 'tool_calls'
              ? 'tool_use'
              : choice?.finish_reason === 'length'
                ? 'max_tokens'
                : choice?.finish_reason === 'stop'
                  ? 'stop'
                  : 'other'

          return {
            content: String(msg?.content ?? ''),
            toolCalls,
            model: String(payload?.model ?? this.model),
            usage: {
              inputTokens: Number(payload?.usage?.prompt_tokens ?? 0),
              outputTokens: Number(payload?.usage?.completion_tokens ?? 0),
            },
            stopReason,
          }
        },
        classifyHttpError,
      )
    } catch (error) {
      if (isRetryableNetworkError(error)) {
        throw new NetworkError(String(error))
      }
      throw error
    }
  }

  async *stream(request: ChatRequest): AsyncGenerator<StreamEvent> {
    const resolvedModel = request.model ?? this.model
    const body = serializeOpenAiRequest(request, resolvedModel)
    body.stream = true

    let attempt = 0
    let response: Response
    while (true) {
      try {
        response = await this.fetcher(this.endpoint, {
          method: 'POST',
          headers: {
            authorization: `Bearer ${this.apiKey}`,
            'content-type': 'application/json',
          },
          body: JSON.stringify(body),
        })
      } catch (error) {
        if (!isRetryableNetworkError(error) || attempt >= this.retryPolicy.maxRetries) {
          throw new NetworkError(String(error))
        }
        attempt += 1
        await new Promise((resolve) =>
          setTimeout(resolve, this.retryPolicy.delayForAttempt(attempt)),
        )
        continue
      }

      if (response.ok) {
        break
      }

      const text = await response.text()
      const error = mapHttpError(
        response.status,
        text || 'minimax stream failed',
        response.headers.get('retry-after'),
      )
      const retryable = isRetryableStatus(response.status)
      if (!retryable || attempt >= this.retryPolicy.maxRetries) {
        throw error
      }
      attempt += 1
      const retryAfterMs = error.retryAfterMs
      const delay = this.retryPolicy.respectRetryAfter
        ? retryAfterMs ?? this.retryPolicy.delayForAttempt(attempt)
        : this.retryPolicy.delayForAttempt(attempt)
      await new Promise((resolve) => setTimeout(resolve, delay))
    }

    if (!response.body) throw new ProviderError('Missing response body for stream')

    const reader = response.body.getReader()
    const decoder = new TextDecoder()
    let buffer = ''

    while (true) {
      const { value, done } = await reader.read()
      if (done) break
      buffer += decoder.decode(value, { stream: true })
      const parts = buffer.split('\n\n')
      buffer = parts.pop() ?? ''

      for (const part of parts) {
        const line = part
          .split('\n')
          .find((l) => l.startsWith('data:'))
          ?.slice(5)
          .trim()
        if (!line) continue
        if (line === '[DONE]') {
          yield { content: '', done: true, eventType: 'text' }
          return
        }
        try {
          const payload = JSON.parse(line)
          const text = String(payload?.choices?.[0]?.delta?.content ?? '')
          if (text) yield { content: text, done: false, eventType: 'text' }
        } catch {
          continue
        }
      }
    }

    yield { content: '', done: true, eventType: 'text' }
  }

  capabilities(): ProviderCapabilities {
    return textOnly()
  }
}
