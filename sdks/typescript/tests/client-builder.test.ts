import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { Client, ClientBuilder } from '../src/client.js'
import { ConfigError, StreamReadTimeoutError, UnsupportedFeatureError } from '../src/error.js'
import { dispatchChat, readTimeoutStream } from '../src/provider.js'
import { RetryPolicy } from '../src/retry.js'
import { ChatGptCodexProvider } from '../src/providers/chatgpt_codex.js'
import type { ChatRequest } from '../src/types.js'

describe('ClientBuilder', () => {
  beforeEach(() => {
    delete process.env.ANTHROPIC_API_KEY
    delete process.env.OPENAI_API_KEY
    delete process.env.MINIMAX_API_KEY
    delete process.env.OLLAMA_API_KEY
    delete process.env.GEMINI_API_KEY
  })

  it('throws ConfigError when provider is not set', () => {
    expect(() => new ClientBuilder().build()).toThrowError(ConfigError)
    expect(() => new ClientBuilder().build()).toThrowError('provider is required')
  })

  it('throws ConfigError when API key is missing and not in env', () => {
    const builder = new ClientBuilder().provider('anthropic')
    expect(() => builder.build()).toThrowError(ConfigError)
    expect(() => builder.build()).toThrowError('Missing API key for provider anthropic')
  })

  it('uses API key from environment when not provided explicitly', () => {
    process.env.ANTHROPIC_API_KEY = 'test-key-from-env'
    const client = new ClientBuilder().provider('anthropic').build()
    expect(client).toBeInstanceOf(Client)
  })

  it('prefers explicit API key over environment', () => {
    process.env.ANTHROPIC_API_KEY = 'env-key'
    const client = new ClientBuilder().provider('anthropic').apiKey('explicit-key').build()
    expect(client).toBeInstanceOf(Client)
  })

  it('builds a Client for each HTTP provider', () => {
    for (const p of ['anthropic', 'openai', 'minimax', 'gemini'] as const) {
      const client = new ClientBuilder().provider(p).apiKey('test-key').build()
      expect(client).toBeInstanceOf(Client)
    }
  })

  it('builds a Client for the gemini provider with an explicit key', () => {
    const client = new ClientBuilder().provider('gemini').apiKey('test-key').build()
    expect(client).toBeInstanceOf(Client)
  })

  it('throws ConfigError when GEMINI_API_KEY is missing for gemini', () => {
    const builder = new ClientBuilder().provider('gemini')
    expect(() => builder.build()).toThrowError(ConfigError)
    expect(() => builder.build()).toThrowError('Missing API key for provider gemini')
  })

  it('uses GEMINI_API_KEY from the environment for gemini', () => {
    process.env.GEMINI_API_KEY = 'gemini-env-key'
    const client = new ClientBuilder().provider('gemini').build()
    expect(client).toBeInstanceOf(Client)
  })

  it('supports a geminiBaseUrl override fluently', () => {
    const client = new ClientBuilder()
      .provider('gemini')
      .apiKey('test-key')
      .geminiBaseUrl('https://gemini.proxy.internal/v1beta')
      .retryPolicy(new RetryPolicy({ maxRetries: 1 }))
      .model('gemini-2.5-pro')
      .build()
    expect(client).toBeInstanceOf(Client)
  })

  it('supports fluent chaining of all base setters', () => {
    const client = new ClientBuilder()
      .provider('anthropic')
      .apiKey('test-key')
      .model('my-model')
      .retryPolicy(new RetryPolicy({ maxRetries: 2 }))
      .streamReadTimeoutSecs(15)
      .anthropicBaseUrl('https://custom.anthropic.com')
      .build()
    expect(client).toBeInstanceOf(Client)
  })

  it('supports minimaxBaseUrl override', () => {
    const client = new ClientBuilder()
      .provider('minimax')
      .apiKey('test-key')
      .minimaxBaseUrl('https://custom.minimax.api/anthropic')
      .build()
    expect(client).toBeInstanceOf(Client)
  })
})

describe('Client capability validation (dispatch)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('rejects image content on a text-only provider before any HTTP call', async () => {
    const fetchSpy = vi.fn()
    vi.stubGlobal('fetch', fetchSpy)
    const client = new ClientBuilder().provider('minimax').apiKey('test-key').build()
    const request: ChatRequest = {
      messages: [
        {
          role: 'user',
          content: '',
          contentBlocks: [
            { type: 'image', source: { type: 'url', url: 'https://example.com/image.jpg' } },
          ],
        },
      ],
    }
    await expect(client.chat(request)).rejects.toThrow(UnsupportedFeatureError)
    await expect(client.chat(request)).rejects.toThrow('provider does not support image input')
    expect(fetchSpy).not.toHaveBeenCalled()
  })

  it('rejects document content on a with-image provider before any HTTP call', async () => {
    const fetchSpy = vi.fn()
    vi.stubGlobal('fetch', fetchSpy)
    const client = new ClientBuilder().provider('openai').apiKey('test-key').build()
    const request: ChatRequest = {
      messages: [
        {
          role: 'user',
          content: '',
          contentBlocks: [
            { type: 'document', source: { type: 'url', url: 'https://example.com/doc.pdf' } },
          ],
        },
      ],
    }
    await expect(client.chat(request)).rejects.toThrow(UnsupportedFeatureError)
    await expect(client.chat(request)).rejects.toThrow('provider does not support document input')
    expect(fetchSpy).not.toHaveBeenCalled()
  })

  it('passes text-only requests through to the provider', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(
            JSON.stringify({
              id: 'msg_1',
              type: 'message',
              role: 'assistant',
              content: [{ type: 'text', text: 'Hi!' }],
              model: 'MiniMax-M2.7',
              stop_reason: 'end_turn',
              usage: { input_tokens: 1, output_tokens: 1 },
            }),
            { status: 200 },
          ),
      ),
    )
    const client = new ClientBuilder().provider('minimax').apiKey('test-key').build()
    const request: ChatRequest = {
      messages: [
        { role: 'user', content: 'Hello!', contentBlocks: [{ type: 'text', text: 'Hello!' }] },
      ],
    }
    const response = await client.chat(request)
    expect(response.content).toBe('Hi!')
  })

  it('dispatchChat does not retry provider-level retryable errors', async () => {
    let calls = 0
    const provider = {
      capabilities: () => ({ supportsImage: true, supportsDocument: true, supportsMcp: false }),
      async chat() {
        calls += 1
        const err = new Error('retryable from provider') as Error & { status?: number }
        err.status = 429
        throw err
      },
      stream() {
        throw new Error('unused')
      },
    }

    await expect(
      dispatchChat(provider, { messages: [{ role: 'user', content: 'hi' }] }),
    ).rejects.toThrow('retryable from provider')
    expect(calls).toBe(1)
  })

  it('treats custom providers without capabilities as text-only by default', async () => {
    let calls = 0
    const client = new Client({
      async chat() {
        calls += 1
        return {
          content: 'unexpected',
          toolCalls: [],
          model: 'custom',
          usage: { inputTokens: 0, outputTokens: 0 },
          stopReason: 'stop',
        }
      },
      async *stream() {},
    })

    await expect(
      client.chat({
        messages: [
          {
            role: 'user',
            content: '',
            contentBlocks: [
              { type: 'image', source: { type: 'url', url: 'https://example.com/image.jpg' } },
            ],
          },
        ],
      }),
    ).rejects.toThrow('provider does not support image input')
    expect(calls).toBe(0)
  })
})

describe('readTimeoutStream', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    vi.useRealTimers()
  })

  it('throws StreamReadTimeoutError on a stalled stream (silent-end retired in M3)', async () => {
    const stalled = (async function* () {
      await new Promise(() => {})
      yield { content: 'never', done: false, eventType: 'text' as const }
    })()

    await expect(async () => {
      for await (const _ of readTimeoutStream(stalled, 0.05)) void _
    }).rejects.toThrow(StreamReadTimeoutError)
  })

  it('clears the pending timeout before yielding an event', async () => {
    vi.useFakeTimers()
    const clearTimeoutSpy = vi.spyOn(globalThis, 'clearTimeout')
    const quick = (async function* () {
      yield { content: 'hello', done: false, eventType: 'text' as const }
    })()

    const iterator = readTimeoutStream(quick, 10)[Symbol.asyncIterator]()
    await expect(iterator.next()).resolves.toMatchObject({
      done: false,
      value: { content: 'hello' },
    })

    expect(clearTimeoutSpy).toHaveBeenCalled()
    await iterator.return?.()
  })

  it('clears the pending timeout when the inner stream rejects', async () => {
    vi.useFakeTimers()
    const clearTimeoutSpy = vi.spyOn(globalThis, 'clearTimeout')
    const throwing = (async function* () {
      throw new Error('inner stream failed')
      yield { content: 'never', done: false, eventType: 'text' as const }
    })()

    await expect(readTimeoutStream(throwing, 10).next()).rejects.toThrow(
      'inner stream failed',
    )
    expect(clearTimeoutSpy).toHaveBeenCalled()
  })

  it('suppresses rejected return promises when a timeout cancels the inner iterator', async () => {
    const returnSpy = vi.fn(() => Promise.reject(new Error('return failed')))
    const stalled = {
      [Symbol.asyncIterator]() {
        return {
          next: async () => new Promise<IteratorResult<never>>(() => {}),
          return: returnSpy,
        }
      },
    }

    await expect(async () => {
      for await (const _ of readTimeoutStream(stalled, 0.05)) void _
    }).rejects.toThrow(StreamReadTimeoutError)
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(returnSpy).toHaveBeenCalledOnce()
  })

  it('calls return on the inner iterator when a read timeout terminates the stream', async () => {
    const returnSpy = vi.fn(async () => ({ done: true, value: undefined }))
    const stalled = {
      [Symbol.asyncIterator]() {
        return {
          next: async () => new Promise<IteratorResult<never>>(() => {}),
          return: returnSpy,
        }
      },
    }

    await expect(async () => {
      for await (const _ of readTimeoutStream(stalled, 0.05)) void _
    }).rejects.toThrow(StreamReadTimeoutError)

    expect(returnSpy).toHaveBeenCalledOnce()
  })

  it('passes through a responsive stream within the deadline', async () => {
    const quick = (async function* () {
      yield { content: 'hello', done: false, eventType: 'text' as const }
      yield { content: '', done: true, eventType: 'text' as const }
    })()

    const wrapped = readTimeoutStream(quick, 10)
    const results: { content: string; done: boolean; eventType: string }[] = []
    for await (const event of wrapped) {
      results.push(event as { content: string; done: boolean; eventType: string })
    }
    expect(results.length).toBe(2)
    expect(results[0].content).toBe('hello')
  })
})

describe('Client stream wiring (stripThink)', () => {
  it('strips <think> tags from a provider stream via the Client', async () => {
    const fakeProvider = {
      capabilities: () => ({ supportsImage: false, supportsDocument: false, supportsMcp: false }),
      async chat() {
        throw new Error('unused')
      },
      async *stream() {
        yield { content: 'a<think>secret</think>b', done: false, eventType: 'text' as const }
        yield { content: '', done: true, eventType: 'text' as const }
      },
    }
    const client = new Client(fakeProvider)
    let out = ''
    for await (const evt of client.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      if (evt.eventType === 'text' && !evt.done) out += evt.content
    }
    expect(out).toBe('ab')
  })
})

describe('ClientBuilder OpenAI auth + chat URL', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  function stubOpenAiOk(captured: { url?: string; headers?: Record<string, string> }) {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, options?: RequestInit) => {
        captured.url = url
        captured.headers = (options?.headers as Record<string, string>) ?? {}
        return new Response(
          JSON.stringify({
            id: 'chatcmpl_builder',
            model: 'gpt-4o',
            choices: [{ index: 0, message: { content: 'ok' }, finish_reason: 'stop' }],
            usage: { prompt_tokens: 1, completion_tokens: 1 },
          }),
          { status: 200 },
        )
      }),
    )
  }

  it('chains openaiAuthXApiKey and openaiChatUrl into requests', async () => {
    const captured: { url?: string; headers?: Record<string, string> } = {}
    stubOpenAiOk(captured)

    const client = new ClientBuilder()
      .provider('openai')
      .apiKey('sk-test123')
      .model('gpt-4o')
      .openaiAuthXApiKey()
      .openaiChatUrl('https://api.custom.com/v1/chat/completions/')
      .build()

    await client.chat({ messages: [{ role: 'user', content: 'test' }] })
    expect(captured.url).toBe('https://api.custom.com/v1/chat/completions')
    expect(captured.headers?.['x-api-key']).toBe('sk-test123')
    expect(captured.headers?.authorization).toBeUndefined()
  })

  it('chains openaiAuthCustomHeader into requests', async () => {
    const captured: { url?: string; headers?: Record<string, string> } = {}
    stubOpenAiOk(captured)

    const client = new ClientBuilder()
      .provider('openai')
      .apiKey('custom-key')
      .openaiAuthCustomHeader('X-Provider-Key')
      .build()

    await client.chat({ messages: [{ role: 'user', content: 'test' }] })
    expect(captured.headers?.['X-Provider-Key']).toBe('custom-key')
    expect(captured.headers?.authorization).toBeUndefined()
    expect(captured.headers?.['x-api-key']).toBeUndefined()
  })

  it('can reset the auth style back to Bearer', async () => {
    const captured: { url?: string; headers?: Record<string, string> } = {}
    stubOpenAiOk(captured)

    const client = new ClientBuilder()
      .provider('openai')
      .apiKey('bearer-key')
      .openaiAuthXApiKey()
      .openaiAuthBearer()
      .build()

    await client.chat({ messages: [{ role: 'user', content: 'test' }] })
    expect(captured.headers?.authorization).toBe('Bearer bearer-key')
    expect(captured.headers?.['x-api-key']).toBeUndefined()
  })
})

describe('ClientBuilder Ollama routing (native vs compat)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  function nativeOk() {
    const urls: string[] = []
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, options?: RequestInit) => {
        urls.push(url)
        // Return an Ollama native NDJSON-ish body that also parses as a
        // single JSON object for chat() (chat uses postJson; one object).
        const isStream = options?.body ? JSON.parse(String(options.body)).stream : false
        if (isStream) {
          return new Response('{"message":{"content":"hi"},"done":false}\n{"done":true}\n', {
            status: 200,
          })
        }
        return new Response(
          JSON.stringify({ model: 'llama3.2', message: { content: 'hi' }, done: true }),
          { status: 200 },
        )
      }),
    )
    return urls
  }

  function compatOk() {
    const urls: string[] = []
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, options?: RequestInit) => {
        urls.push(url)
        const isStream = options?.body ? JSON.parse(String(options.body)).stream : false
        if (isStream) {
          return new Response(
            'data: {"choices":[{"index":0,"delta":{"content":"hi"},"finish_reason":"stop"}]}\n\ndata: [DONE]\n\n',
            { status: 200, headers: { 'content-type': 'text/event-stream' } },
          )
        }
        return new Response(
          JSON.stringify({
            model: 'llama3.2',
            choices: [{ index: 0, message: { content: 'hi' }, finish_reason: 'stop' }],
            usage: { prompt_tokens: 1, completion_tokens: 1 },
          }),
          { status: 200 },
        )
      }),
    )
    return urls
  }

  it('routes to native /api/chat for BOTH chat and stream when ollamaNative(true)', async () => {
    const urls = nativeOk()
    const client = new ClientBuilder()
      .provider('ollama')
      .ollamaBaseUrl('http://localhost:11434')
      .ollamaNative(true)
      .build()

    await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
    for await (const _e of client.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      // drain
    }

    // SAME instance, SAME endpoint for chat and stream (never split).
    expect(urls).toEqual([
      'http://localhost:11434/api/chat',
      'http://localhost:11434/api/chat',
    ])
  })

  it('routes to native when ANY tuning field is set (ollamaThink)', async () => {
    const urls = nativeOk()
    const client = new ClientBuilder().provider('ollama').ollamaThink('high').build()
    await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(urls[0]).toBe('http://localhost:11434/api/chat')
  })

  it('routes to native when ollamaKeepAlive is set', async () => {
    const urls = nativeOk()
    const client = new ClientBuilder().provider('ollama').ollamaKeepAlive('5m').build()
    await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(urls[0]).toBe('http://localhost:11434/api/chat')
  })

  it('routes to native when ollamaNumCtx is set', async () => {
    const urls = nativeOk()
    const client = new ClientBuilder().provider('ollama').ollamaNumCtx(4096).build()
    await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(urls[0]).toBe('http://localhost:11434/api/chat')
  })

  it('routes to OpenAI-compat /v1/chat/completions by default (no tuning, no native flag)', async () => {
    const urls = compatOk()
    const client = new ClientBuilder()
      .provider('ollama')
      .ollamaBaseUrl('http://localhost:11434')
      .build()
    await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
    for await (const _e of client.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      // drain
    }
    expect(urls).toEqual([
      'http://localhost:11434/v1/chat/completions',
      'http://localhost:11434/v1/chat/completions',
    ])
  })

  it('compat path sends Bearer with an empty key (harmless against Ollama)', async () => {
    let authHeader: string | undefined
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, options?: RequestInit) => {
        authHeader = (options?.headers as Record<string, string>)?.authorization
        return new Response(
          JSON.stringify({
            model: 'llama3.2',
            choices: [{ index: 0, message: { content: 'hi' }, finish_reason: 'stop' }],
            usage: { prompt_tokens: 1, completion_tokens: 1 },
          }),
          { status: 200 },
        )
      }),
    )
    const client = new ClientBuilder().provider('ollama').build()
    await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(authHeader).toBe('Bearer ')
  })

  it('defaults the base URL to http://localhost:11434 when unset', async () => {
    const urls = compatOk()
    const client = new ClientBuilder().provider('ollama').build()
    await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(urls[0]).toBe('http://localhost:11434/v1/chat/completions')
  })
})

describe('ClientBuilder.chatgptCodex', () => {
  it('builds a Client without an api key', () => {
    const client = new ClientBuilder().chatgptCodex('tok', 'acct').build()
    expect(client).toBeInstanceOf(Client)
  })

  it('constructs a ChatGptCodexProvider with the given token/accountId/model', () => {
    const prov = new ClientBuilder().chatgptCodex('tok', 'acct', 'gpt-x').buildProviderForTest()
    expect(prov).toBeInstanceOf(ChatGptCodexProvider)
    expect((prov as ChatGptCodexProvider).modelId()).toBe('gpt-x')
  })

  it('threads the reasoning effort default', () => {
    const prov = new ClientBuilder()
      .chatgptCodex('tok', 'acct', undefined, { reasoningEffort: 'high' })
      .buildProviderForTest() as ChatGptCodexProvider
    const body = prov.buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, prov.modelId())
    expect(body.reasoning).toEqual({ effort: 'high', summary: 'auto' })
  })

  it('does not require an api key (no ConfigError)', () => {
    expect(() => new ClientBuilder().chatgptCodex('tok', 'acct').build()).not.toThrow()
  })
})

describe('ClientBuilder Ollama api-key-not-required seam', () => {
  beforeEach(() => {
    delete process.env.OLLAMA_API_KEY
  })

  it('build() succeeds for provider("ollama") with NO apiKey', () => {
    const client = new ClientBuilder()
      .provider('ollama')
      .ollamaBaseUrl('http://localhost:11434')
      .build()
    expect(client).toBeInstanceOf(Client)
  })

  it('build() succeeds for provider("ollama") with no apiKey and no base url', () => {
    const client = new ClientBuilder().provider('ollama').build()
    expect(client).toBeInstanceOf(Client)
  })
})

describe('ClientBuilder Ollama tuning-field validation (non-ollama provider)', () => {
  beforeEach(() => {
    delete process.env.OPENAI_API_KEY
  })

  it('throws ConfigError naming ollama_think on a non-ollama provider', () => {
    const builder = new ClientBuilder().provider('openai').apiKey('sk-test').ollamaThink('high')
    expect(() => builder.build()).toThrowError(ConfigError)
    expect(() => builder.build()).toThrowError('ollama_think can only be used with Provider::Ollama')
  })

  it('throws ConfigError naming ollama_keep_alive on a non-ollama provider', () => {
    const builder = new ClientBuilder().provider('openai').apiKey('sk-test').ollamaKeepAlive('5m')
    expect(() => builder.build()).toThrowError('ollama_keep_alive can only be used with Provider::Ollama')
  })

  it('throws ConfigError naming ollama_num_ctx on a non-ollama provider', () => {
    const builder = new ClientBuilder().provider('openai').apiKey('sk-test').ollamaNumCtx(4096)
    expect(() => builder.build()).toThrowError('ollama_num_ctx can only be used with Provider::Ollama')
  })

  it('joins multiple misused tuning fields in Rust order', () => {
    const builder = new ClientBuilder()
      .provider('openai')
      .apiKey('sk-test')
      .ollamaThink('high')
      .ollamaKeepAlive('5m')
      .ollamaNumCtx(4096)
    // Rust pushes keep_alive, num_ctx, think in that order (client.rs:982-990).
    expect(() => builder.build()).toThrowError(
      'ollama_keep_alive, ollama_num_ctx, ollama_think can only be used with Provider::Ollama',
    )
  })

  it('does NOT validate ollamaNative or ollamaBaseUrl on a non-ollama provider', () => {
    // Rust only guards the three TUNING fields, not native/base_url.
    const client = new ClientBuilder()
      .provider('openai')
      .apiKey('sk-test')
      .ollamaNative(true)
      .ollamaBaseUrl('http://x')
      .build()
    expect(client).toBeInstanceOf(Client)
  })
})

describe('ClientBuilder Ollama native route rejects image input before HTTP', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  const imageReq: ChatRequest = {
    messages: [
      {
        role: 'user',
        content: '',
        contentBlocks: [
          { type: 'image', source: { type: 'url', url: 'https://example.com/i.jpg' } },
        ],
      },
    ],
  }

  it('chat() throws UnsupportedFeatureError on the native route (tuning field set)', async () => {
    const fetchSpy = vi.fn()
    vi.stubGlobal('fetch', fetchSpy)
    const client = new ClientBuilder().provider('ollama').ollamaThink('high').build()
    await expect(client.chat(imageReq)).rejects.toThrow(UnsupportedFeatureError)
    await expect(client.chat(imageReq)).rejects.toThrow('provider does not support image input')
    expect(fetchSpy).not.toHaveBeenCalled()
  })

  it('stream() throws UnsupportedFeatureError on the native route (ollamaNative)', async () => {
    const fetchSpy = vi.fn()
    vi.stubGlobal('fetch', fetchSpy)
    const client = new ClientBuilder().provider('ollama').ollamaNative(true).build()
    await expect(async () => {
      for await (const _e of client.stream(imageReq)) {
        // drain
      }
    }).rejects.toThrow('provider does not support image input')
    expect(fetchSpy).not.toHaveBeenCalled()
  })

  it('compat route ALLOWS image input (OpenAIProvider withImage())', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(
            JSON.stringify({
              model: 'llama3.2',
              choices: [{ index: 0, message: { content: 'ok' }, finish_reason: 'stop' }],
              usage: { prompt_tokens: 1, completion_tokens: 1 },
            }),
            { status: 200 },
          ),
      ),
    )
    const client = new ClientBuilder().provider('ollama').build() // no tuning → compat
    const res = await client.chat(imageReq)
    expect(res.content).toBe('ok')
  })
})
