import { AuthError, ConfigError, NetworkError, ProviderError, RateLimitError } from '../error.js'
import type { ChatRequest, ChatResponse, Message, StreamEvent, ToolCall } from '../types.js'

interface OpenAICtor {
  new (args: { apiKey: string }): {
    chat: {
      completions: {
        create: (args: Record<string, unknown>) => Promise<any>
      }
    }
  }
}

export class OpenAIProvider {
  private clientPromise: Promise<any>
  private model: string

  constructor(
    private readonly apiKey: string,
    model?: string,
    injectedClient?: any
  ) {
    this.model = model ?? 'gpt-4o'
    this.clientPromise = injectedClient
      ? Promise.resolve(injectedClient)
      : import('openai')
          .then((mod) => {
            const Ctor = (mod.default ?? (mod as any).OpenAI) as unknown as OpenAICtor
            if (!Ctor) throw new ConfigError('Missing OpenAI SDK constructor')
            return new Ctor({ apiKey })
          })
          .catch((error) => {
            if (error instanceof ConfigError) throw error
            throw new ConfigError('Install optional peer dependency openai')
          })
  }

  static serializeMessages(messages: Message[], system?: string): any[] {
    const outgoing: any[] = []
    if (system) outgoing.push({ role: 'system', content: system })
    for (const message of messages) {
      if (message.role === 'system') {
        outgoing.push({ role: 'system', content: message.content })
      } else if (message.role === 'user') {
        outgoing.push({ role: 'user', content: message.content })
      } else if (message.role === 'assistant') {
        if (message.toolCalls?.length) {
          outgoing.push({
            role: 'assistant',
            content: message.content,
            tool_calls: message.toolCalls.map((tc) => ({
              id: tc.id,
              type: 'function',
              function: { name: tc.name, arguments: JSON.stringify(tc.input) }
            }))
          })
        } else {
          outgoing.push({ role: 'assistant', content: message.content })
        }
      } else if (message.role === 'tool' && message.toolCallId) {
        outgoing.push({ role: 'tool', tool_call_id: message.toolCallId, content: message.content })
      }
    }
    return outgoing
  }

  async chat(request: ChatRequest): Promise<ChatResponse> {
    const client = await this.clientPromise
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

    let payload: any
    try {
      payload = await client.chat.completions.create(body)
      if (typeof payload?.model_dump === 'function') payload = payload.model_dump()
    } catch (error: any) {
      const name = String(error?.constructor?.name ?? '').toLowerCase()
      if (name.includes('authentication')) throw new AuthError(String(error))
      if (name.includes('ratelimit') || name.includes('rate_limit')) throw new RateLimitError(String(error))
      throw new NetworkError(String(error))
    }

    const choice = payload?.choices?.[0] ?? {}
    const message = choice?.message ?? {}
    const toolCalls: ToolCall[] = (message?.tool_calls ?? []).map((tc: any) => {
      const args = String(tc?.function?.arguments ?? '{}')
      let parsed: Record<string, unknown> = {}
      try {
        parsed = JSON.parse(args)
      } catch {
        parsed = {}
      }
      return {
        id: String(tc?.id ?? ''),
        name: String(tc?.function?.name ?? ''),
        input: parsed
      }
    })

    const finishReasonMap: Record<string, ChatResponse['stopReason']> = {
      stop: 'stop',
      length: 'max_tokens',
      tool_calls: 'tool_use'
    }

    return {
      content: String(message?.content ?? ''),
      toolCalls,
      model: String(payload?.model ?? this.model),
      usage: {
        inputTokens: Number(payload?.usage?.prompt_tokens ?? 0),
        outputTokens: Number(payload?.usage?.completion_tokens ?? 0)
      },
      stopReason: finishReasonMap[String(choice?.finish_reason ?? '')] ?? 'other'
    }
  }

  async *stream(request: ChatRequest): AsyncGenerator<StreamEvent> {
    const client = await this.clientPromise
    const body: Record<string, unknown> = {
      model: request.model ?? this.model,
      messages: OpenAIProvider.serializeMessages(request.messages, request.system),
      stream: true
    }
    if (request.providerOptions) Object.assign(body, request.providerOptions)

    let stream: any
    try {
      stream = await client.chat.completions.create(body)
    } catch (error: any) {
      throw new ProviderError(String(error))
    }

    for await (const raw of stream) {
      const chunk = typeof raw?.model_dump === 'function' ? raw.model_dump() : raw
      const choice = chunk?.choices?.[0]
      const text = String(choice?.delta?.content ?? '')
      if (text) yield { content: text, done: false }
      if (choice?.finish_reason) {
        yield { content: '', done: true }
        return
      }
    }
  }
}
