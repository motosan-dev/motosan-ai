import { ConfigError } from './error.js'
import {
  dispatchChat,
  dispatchStream,
  readTimeoutStream,
  textOnly,
  type Provider,
  type ProviderImpl as DispatchProvider,
} from './provider.js'
import { RetryPolicy } from './retry.js'
import { collectStream, type BoxStream } from './stream.js'
import { stripThink } from './think_stripper.js'
import { AnthropicProvider } from './providers/anthropic.js'
import { MinimaxProvider } from './providers/minimax.js'
import { OllamaProvider } from './providers/ollama.js'
import { OpenAIProvider, type OpenAIAuthStyle } from './providers/openai.js'
import { GeminiProvider } from './providers/gemini.js'
import { DEFAULT_OLLAMA_MODEL } from './models.js'
import type { ChatRequest, ChatResponse, StreamEvent } from './types.js'

export type ProviderName = Provider

export interface ProviderLike {
  capabilities?(): ReturnType<DispatchProvider['capabilities']>
  chat(request: ChatRequest): Promise<ChatResponse>
  stream(request: ChatRequest): AsyncIterable<StreamEvent>
}

/**
 * HTTP providers that REQUIRE an API key. Reserved seam (mirrors Rust
 * client.rs `api_key_required = !matches!(provider, ClaudeCode | ...)`):
 * future CLI backends opt out by NOT being in this set, without touching build().
 */
const HTTP_PROVIDERS: ReadonlySet<ProviderName> = new Set([
  'anthropic',
  'openai',
  'minimax',
  'gemini',
])

const ENV_KEY_BY_PROVIDER: Record<ProviderName, string> = {
  anthropic: 'ANTHROPIC_API_KEY',
  openai: 'OPENAI_API_KEY',
  minimax: 'MINIMAX_API_KEY',
  // Ollama needs no env key; '' keeps the Record total and
  // process.env[''] yields undefined (harmless — apiKey not required).
  ollama: '',
  gemini: 'GEMINI_API_KEY',
}

function asDispatchProvider(provider: ProviderLike): DispatchProvider {
  if (provider.capabilities) {
    return provider as DispatchProvider
  }

  return {
    capabilities: textOnly,
    chat: (request: ChatRequest) => provider.chat(request),
    stream: (request: ChatRequest) => provider.stream(request),
  }
}

/**
 * Fluent builder for a fully-configured Client. Mirrors Rust `ClientBuilder`,
 * trimmed to TS HTTP providers. Mutating-`this` (single-use).
 */
export class ClientBuilder {
  protected _provider?: ProviderName
  protected _apiKey?: string
  protected _model?: string
  protected _retryPolicy: RetryPolicy = RetryPolicy.default()
  protected _streamReadTimeoutSecs?: number
  protected _anthropicBaseUrl?: string
  protected _minimaxBaseUrl?: string
  protected _geminiBaseUrl?: string
  protected _openaiAuthStyle: OpenAIAuthStyle = { kind: 'bearer' }
  protected _openaiChatUrl?: string
  protected _openaiResponsesFallback = false
  protected _openaiResponsesUrl?: string
  protected _ollamaBaseUrl?: string
  protected _ollamaNative?: boolean
  protected _ollamaThink?: string
  protected _ollamaKeepAlive?: string
  protected _ollamaNumCtx?: number

  provider(p: Provider): this {
    this._provider = p
    return this
  }

  apiKey(k: string): this {
    this._apiKey = k
    return this
  }

  model(m: string): this {
    this._model = m
    return this
  }

  retryPolicy(rp: RetryPolicy): this {
    this._retryPolicy = rp
    return this
  }

  streamReadTimeoutSecs(n: number): this {
    this._streamReadTimeoutSecs = n
    return this
  }

  anthropicBaseUrl(u: string): this {
    this._anthropicBaseUrl = u
    return this
  }

  geminiBaseUrl(u: string): this {
    this._geminiBaseUrl = u
    return this
  }

  /** MiniMax Anthropic-compat BASE URL; the final URL is `{base}/v1/messages`. */
  minimaxBaseUrl(u: string): this {
    this._minimaxBaseUrl = u
    return this
  }

  openaiAuthBearer(): this {
    this._openaiAuthStyle = { kind: 'bearer' }
    return this
  }

  openaiAuthXApiKey(): this {
    this._openaiAuthStyle = { kind: 'xApiKey' }
    return this
  }

  openaiAuthCustomHeader(name: string): this {
    this._openaiAuthStyle = { kind: 'custom', header: name }
    return this
  }

  openaiChatUrl(u: string): this {
    this._openaiChatUrl = u
    return this
  }

  openaiResponsesFallback(enabled: boolean): this {
    this._openaiResponsesFallback = enabled
    return this
  }

  openaiResponsesUrl(u: string): this {
    this._openaiResponsesUrl = u
    return this
  }

  ollamaBaseUrl(u: string): this {
    this._ollamaBaseUrl = u
    return this
  }

  ollamaNative(native: boolean): this {
    this._ollamaNative = native
    return this
  }

  ollamaThink(think: string): this {
    this._ollamaThink = think
    return this
  }

  ollamaKeepAlive(duration: string): this {
    this._ollamaKeepAlive = duration
    return this
  }

  ollamaNumCtx(tokens: number): this {
    this._ollamaNumCtx = tokens
    return this
  }

  /**
   * Construct the configured provider with its existing constructor signature,
   * then chain Task 5's mutating `withRetryPolicy(policy): this` setter.
   * Subclass tasks (7/8) override the 'openai' arm to apply auth-style /
   * chat-url / responses config.
   */
  protected buildProvider(provider: ProviderName, apiKey: string): DispatchProvider {
    if (provider === 'anthropic') {
      return new AnthropicProvider(apiKey, this._model, this._anthropicBaseUrl).withRetryPolicy(
        this._retryPolicy,
      )
    }
    if (provider === 'openai') {
      let openai = new OpenAIProvider(apiKey, this._model)
        .withRetryPolicy(this._retryPolicy)
        .withAuthStyle(this._openaiAuthStyle)
      if (this._openaiChatUrl) {
        openai = openai.withChatUrl(this._openaiChatUrl)
      }
      if (this._openaiResponsesUrl) {
        openai = openai.withResponsesUrl(this._openaiResponsesUrl)
      }
      if (this._openaiResponsesFallback) {
        openai = openai.withResponsesFallback(this._openaiResponsesFallback)
      }
      return openai
    }
    if (provider === 'ollama') {
      const ollamaBaseUrl = this._ollamaBaseUrl ?? 'http://localhost:11434'

      // Auto-routing (contract §4 / client.rs:245-248). The SAME constructed
      // instance serves chat and stream, so routing can never split.
      const needsNative =
        this._ollamaNative === true ||
        this._ollamaKeepAlive !== undefined ||
        this._ollamaNumCtx !== undefined ||
        this._ollamaThink !== undefined

      if (needsNative) {
        // Native /api/chat (text-only). Mirrors build_ollama_native_provider
        // (client.rs:610-621).
        const model = this._model ?? DEFAULT_OLLAMA_MODEL
        return new OllamaProvider(model, ollamaBaseUrl)
          .withThink(this._ollamaThink)
          .withKeepAlive(this._ollamaKeepAlive)
          .withNumCtx(this._ollamaNumCtx)
          .withRetryPolicy(this._retryPolicy)
      }

      // OpenAI-compat /v1/chat/completions (reuses OpenAIProvider, empty key,
      // Bearer). Mirrors build_ollama_provider (client.rs:599-607). Declares
      // withImage() capabilities (model-dependent accuracy).
      const model = this._model ?? DEFAULT_OLLAMA_MODEL
      return new OpenAIProvider('', model)
        .withChatUrl(`${ollamaBaseUrl.replace(/\/+$/, '')}/v1/chat/completions`)
        .withAuthStyle({ kind: 'bearer' })
        .withRetryPolicy(this._retryPolicy)
    }
    if (provider === 'gemini') {
      return new GeminiProvider(apiKey, this._model, this._geminiBaseUrl).withRetryPolicy(
        this._retryPolicy,
      )
    }
    return new MinimaxProvider(apiKey, this._model, this._minimaxBaseUrl).withRetryPolicy(
      this._retryPolicy,
    )
  }

  /** Build a Client. Throws ConfigError on missing provider / missing key for HTTP providers. */
  build(): Client {
    if (!this._provider) {
      throw new ConfigError('provider is required')
    }

    const apiKeyRequired = HTTP_PROVIDERS.has(this._provider)
    const apiKey = this._apiKey ?? process.env[ENV_KEY_BY_PROVIDER[this._provider]]
    if (apiKeyRequired && !apiKey) {
      throw new ConfigError(`Missing API key for provider ${this._provider}`)
    }

    if (this._provider !== 'ollama') {
      const misused: string[] = []
      if (this._ollamaKeepAlive !== undefined) misused.push('ollama_keep_alive')
      if (this._ollamaNumCtx !== undefined) misused.push('ollama_num_ctx')
      if (this._ollamaThink !== undefined) misused.push('ollama_think')
      if (misused.length > 0) {
        throw new ConfigError(`${misused.join(', ')} can only be used with Provider::Ollama`)
      }
    }

    const provider = this.buildProvider(this._provider, apiKey ?? '')
    return new Client(provider, this._streamReadTimeoutSecs)
  }
}

/**
 * Client for interacting with LLMs via pluggable providers. Keeps the M1/M2
 * options-object constructor; additionally accepts a built provider instance
 * (from ClientBuilder) plus an optional stream-read-timeout. Routes through
 * dispatch (validate → provider) → readTimeoutStream → stripThink.
 */
export class Client {
  private provider: DispatchProvider
  private streamReadTimeoutSecs?: number

  static builder(): ClientBuilder {
    return new ClientBuilder()
  }

  constructor(
    options:
      | {
          provider: ProviderName | ProviderLike
          apiKey?: string
          model?: string
          minimaxBaseUrl?: string
        }
      | ProviderLike,
    streamReadTimeoutSecs?: number,
  ) {
    this.streamReadTimeoutSecs = streamReadTimeoutSecs

    if (typeof (options as ProviderLike).chat === 'function') {
      this.provider = asDispatchProvider(options as ProviderLike)
      return
    }

    const opts = options as {
      provider: ProviderName | ProviderLike
      apiKey?: string
      model?: string
      minimaxBaseUrl?: string
    }

    if (typeof opts.provider !== 'string') {
      this.provider = asDispatchProvider(opts.provider)
      return
    }

    const provider = opts.provider
    const apiKey = opts.apiKey ?? process.env[ENV_KEY_BY_PROVIDER[provider]]
    if (!apiKey && provider !== 'ollama') {
      throw new ConfigError(`Missing API key for provider ${provider}`)
    }

    const resolvedApiKey = apiKey ?? ''
    if (provider === 'anthropic') {
      this.provider = new AnthropicProvider(resolvedApiKey, opts.model)
    } else if (provider === 'openai') {
      this.provider = new OpenAIProvider(resolvedApiKey, opts.model)
    } else if (provider === 'ollama') {
      // Legacy ctor has no tuning fields → OpenAI-compat path (empty key,
      // Bearer) against the default Ollama base URL. For native/tuning, use
      // Client.builder().
      this.provider = new OpenAIProvider(resolvedApiKey, opts.model ?? DEFAULT_OLLAMA_MODEL)
        .withChatUrl('http://localhost:11434/v1/chat/completions')
        .withAuthStyle({ kind: 'bearer' })
    } else if (provider === 'gemini') {
      this.provider = new GeminiProvider(resolvedApiKey, opts.model)
    } else {
      this.provider = new MinimaxProvider(resolvedApiKey, opts.model, opts.minimaxBaseUrl)
    }
  }

  /** Send a chat request; validates capabilities BEFORE any HTTP call. */
  async chat(request: ChatRequest): Promise<ChatResponse> {
    return dispatchChat(this.provider, request)
  }

  /**
   * Stream a chat request: dispatch (validate → provider.stream) → optional
   * readTimeoutStream → stripThink. Matches Rust ordering.
   */
  stream(request: ChatRequest): AsyncIterable<StreamEvent> {
    let stream: BoxStream = dispatchStream(this.provider, request)

    if (this.streamReadTimeoutSecs !== undefined) {
      stream = readTimeoutStream(stream, this.streamReadTimeoutSecs)
    }

    stream = stripThink(stream)
    return stream
  }

  /** Stream and collect the full response into a ChatResponse. */
  async streamCollect(request: ChatRequest): Promise<ChatResponse> {
    return collectStream(this.stream(request))
  }

  /** Stream and collect, preferring the request's model override in the result. */
  async streamCollectWith(request: ChatRequest): Promise<ChatResponse> {
    const response = await collectStream(this.stream(request))
    if (request.model) {
      response.model = request.model
    }
    return response
  }
}
