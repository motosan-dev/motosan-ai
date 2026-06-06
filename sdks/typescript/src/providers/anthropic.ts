import { AuthError, ConfigError, NetworkError, ProviderError, RateLimitError } from '../error.js'
import type { ChatRequest, ChatResponse, Message, StreamEvent, ToolCall } from '../types.js'

interface AnthropicCtor {
  new (args: { apiKey: string }): {
    messages: {
      create: (args: Record<string, unknown>) => Promise<any>
    }
  }
}

export class AnthropicProvider {
  private clientPromise: Promise<any>
  private model: string

  constructor(
    private readonly apiKey: string,
    model?: string,
    injectedClient?: any
  ) {
    this.model = model ?? 'claude-sonnet-4-5'
    this.clientPromise = injectedClient
      ? Promise.resolve(injectedClient)
      : import('@anthropic-ai/sdk')
          .then((mod) => {
            const Ctor = (mod.default ?? (mod as any).Anthropic) as unknown as AnthropicCtor
            if (!Ctor) throw new ConfigError('Missing Anthropic SDK constructor')
            return new Ctor({ apiKey })
          })
          .catch((error) => {
            if (error instanceof ConfigError) throw error
            throw new ConfigError('Install optional peer dependency @anthropic-ai/sdk')
          })
  }

  static serializeMessages(messages: Message[], explicitSystem?: string): { messages: any[]; system?: string } {
    const outgoing: any[] = []
    const systemParts: string[] = []
    if (explicitSystem?.trim()) systemParts.push(explicitSystem)

    for (const message of messages) {
      if (message.role === 'system') {
        if (message.content.trim()) systemParts.push(message.content)
      } else if (message.role === 'user') {
        outgoing.push({ role: 'user', content: message.content })
      } else if (message.role === 'assistant') {
        if (message.toolCalls?.length) {
          const blocks: any[] = []
          if (message.content) blocks.push({ type: 'text', text: message.content })
          for (const tc of message.toolCalls) {
            blocks.push({ type: 'tool_use', id: tc.id, name: tc.name, input: tc.input })
          }
          outgoing.push({ role: 'assistant', content: blocks })
        } else {
          outgoing.push({ role: 'assistant', content: message.content })
        }
      } else if (message.role === 'tool' && message.toolCallId) {
        outgoing.push({
          role: 'user',
          content: [{ type: 'tool_result', tool_use_id: message.toolCallId, content: message.content }]
        })
      }
    }

    return {
      messages: outgoing,
      system: systemParts.length ? systemParts.join('\n\n') : undefined
    }
  }

  async chat(request: ChatRequest): Promise<ChatResponse> {
    const client = await this.clientPromise
    const serialized = AnthropicProvider.serializeMessages(request.messages, request.system)
    const body: Record<string, unknown> = {
      model: request.model ?? this.model,
      messages: serialized.messages,
      max_tokens: request.maxTokens ?? 1024
    }
    if (serialized.system) body.system = serialized.system
    if (request.temperature != null) body.temperature = request.temperature
    if (request.tools?.length) {
      body.tools = request.tools.map((t) => ({
        name: t.name,
        description: t.description ?? '',
        input_schema: t.inputSchema ?? { type: 'object', properties: {} }
      }))
    }
    if (request.providerOptions) Object.assign(body, request.providerOptions)

    let payload: any
    try {
      payload = await client.messages.create(body)
      if (typeof payload?.model_dump === 'function') payload = payload.model_dump()
    } catch (error: any) {
      const name = String(error?.constructor?.name ?? '').toLowerCase()
      if (name.includes('authentication')) throw new AuthError(String(error))
      if (name.includes('ratelimit') || name.includes('rate_limit')) throw new RateLimitError(String(error))
      throw new NetworkError(String(error))
    }

    const contentBlocks = Array.isArray(payload?.content) ? payload.content : []
    const content = contentBlocks
      .filter((b: any) => b?.type === 'text')
      .map((b: any) => String(b?.text ?? ''))
      .join('')
    const toolCalls: ToolCall[] = contentBlocks
      .filter((b: any) => b?.type === 'tool_use')
      .map((b: any) => ({ id: String(b?.id ?? ''), name: String(b?.name ?? ''), input: (b?.input ?? {}) as Record<string, unknown> }))

    const stopReasonMap: Record<string, ChatResponse['stopReason']> = {
      end_turn: 'end_turn',
      max_tokens: 'max_tokens',
      tool_use: 'tool_use',
      stop: 'stop'
    }

    return {
      content,
      toolCalls,
      model: String(payload?.model ?? this.model),
      usage: {
        inputTokens: Number(payload?.usage?.input_tokens ?? 0),
        outputTokens: Number(payload?.usage?.output_tokens ?? 0)
      },
      stopReason: stopReasonMap[String(payload?.stop_reason ?? '')] ?? 'other'
    }
  }

  async *stream(request: ChatRequest): AsyncGenerator<StreamEvent> {
    const client = await this.clientPromise
    const serialized = AnthropicProvider.serializeMessages(request.messages, request.system)
    const body: Record<string, unknown> = {
      model: request.model ?? this.model,
      messages: serialized.messages,
      max_tokens: request.maxTokens ?? 1024,
      stream: true
    }
    if (serialized.system) body.system = serialized.system
    if (request.providerOptions) Object.assign(body, request.providerOptions)

    let events: any
    try {
      events = await client.messages.create(body)
    } catch (error: any) {
      const name = String(error?.constructor?.name ?? '').toLowerCase()
      if (name.includes('authentication')) throw new AuthError(String(error))
      if (name.includes('ratelimit') || name.includes('rate_limit')) throw new RateLimitError(String(error))
      throw new ProviderError(String(error))
    }

    for await (const raw of events) {
      const event = typeof raw?.model_dump === 'function' ? raw.model_dump() : raw
      if (event?.type === 'content_block_delta') {
        const text = String(event?.delta?.text ?? '')
        if (text) yield { content: text, done: false, eventType: 'text' }
      } else if (event?.type === 'message_stop') {
        yield { content: '', done: true, eventType: 'text' }
        return
      }
    }
  }
}
