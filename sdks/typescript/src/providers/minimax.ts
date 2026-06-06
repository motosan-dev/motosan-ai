import { NetworkError, ProviderError, mapHttpError } from '../error.js'
import { OpenAIProvider } from './openai.js'
import type { ChatRequest, ChatResponse, StreamEvent } from '../types.js'

export class MinimaxProvider {
  private model: string
  private endpoint: string

  constructor(
    private readonly apiKey: string,
    model?: string,
    endpoint?: string,
    private readonly fetcher: typeof fetch = fetch
  ) {
    this.model = model ?? 'MiniMax-Text-01'
    this.endpoint = endpoint ?? 'https://api.minimax.chat/v1/text/chatcompletion_v2'
  }

  async chat(request: ChatRequest): Promise<ChatResponse> {
    const body: Record<string, unknown> = {
      model: request.model ?? this.model,
      messages: OpenAIProvider.serializeMessages(request.messages, request.system)
    }
    if (request.maxTokens != null) body.max_tokens = request.maxTokens
    if (request.temperature != null) body.temperature = request.temperature
    if (request.tools?.length) {
      body.tools = request.tools.map((t) => ({
        type: 'function',
        function: {
          name: t.name,
          description: t.description ?? '',
          parameters: t.inputSchema ?? { type: 'object', properties: {} }
        }
      }))
    }
    if (request.providerOptions) Object.assign(body, request.providerOptions)

    let response: Response
    try {
      response = await this.fetcher(this.endpoint, {
        method: 'POST',
        headers: {
          authorization: `Bearer ${this.apiKey}`,
          'content-type': 'application/json'
        },
        body: JSON.stringify(body)
      })
    } catch (error: any) {
      throw new NetworkError(String(error))
    }

    const text = await response.text()
    let payload: any = {}
    try {
      payload = text ? JSON.parse(text) : {}
    } catch {
      payload = {}
    }

    if (!response.ok) {
      const message = String(payload?.error?.message ?? text ?? 'minimax request failed')
      throw mapHttpError(response.status, message)
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
      return { id: String(tc?.id ?? ''), name: String(tc?.function?.name ?? ''), input: parsed }
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
        outputTokens: Number(payload?.usage?.completion_tokens ?? 0)
      },
      stopReason
    }
  }

  async *stream(request: ChatRequest): AsyncGenerator<StreamEvent> {
    const body = {
      model: request.model ?? this.model,
      messages: OpenAIProvider.serializeMessages(request.messages, request.system),
      stream: true,
      ...(request.providerOptions ?? {})
    }

    let response: Response
    try {
      response = await this.fetcher(this.endpoint, {
        method: 'POST',
        headers: {
          authorization: `Bearer ${this.apiKey}`,
          'content-type': 'application/json'
        },
        body: JSON.stringify(body)
      })
    } catch (error: any) {
      throw new NetworkError(String(error))
    }

    if (!response.ok) {
      const text = await response.text()
      throw mapHttpError(response.status, text || 'minimax stream failed')
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
}
