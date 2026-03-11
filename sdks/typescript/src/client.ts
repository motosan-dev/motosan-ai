import { ConfigError } from './error.js'
import { AnthropicProvider } from './providers/anthropic.js'
import { MinimaxProvider } from './providers/minimax.js'
import { OpenAIProvider } from './providers/openai.js'
import type { ChatRequest, ChatResponse, StreamEvent } from './types.js'

export type ProviderName = 'anthropic' | 'openai' | 'minimax'

export interface ProviderLike {
  chat(request: ChatRequest): Promise<ChatResponse>
  stream(request: ChatRequest): AsyncGenerator<StreamEvent>
}

export class Client {
  private provider: ProviderLike

  constructor(options: {
    provider: ProviderName | ProviderLike
    apiKey?: string
    model?: string
    minimaxEndpoint?: string
  }) {
    if (typeof options.provider !== 'string') {
      this.provider = options.provider
      return
    }

    const provider = options.provider
    const envMap: Record<ProviderName, string> = {
      anthropic: 'ANTHROPIC_API_KEY',
      openai: 'OPENAI_API_KEY',
      minimax: 'MINIMAX_API_KEY'
    }

    const apiKey = options.apiKey ?? process.env[envMap[provider]]
    if (!apiKey) {
      throw new ConfigError(`Missing API key for provider ${provider}`)
    }

    if (provider === 'anthropic') {
      this.provider = new AnthropicProvider(apiKey, options.model)
    } else if (provider === 'openai') {
      this.provider = new OpenAIProvider(apiKey, options.model)
    } else {
      this.provider = new MinimaxProvider(apiKey, options.model, options.minimaxEndpoint)
    }
  }

  async chat(request: ChatRequest): Promise<ChatResponse> {
    return this.provider.chat(request)
  }

  stream(request: ChatRequest): AsyncGenerator<StreamEvent> {
    return this.provider.stream(request)
  }
}
