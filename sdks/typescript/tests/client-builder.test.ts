import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { Client, ClientBuilder } from '../src/client.js'
import { ConfigError, UnsupportedFeatureError } from '../src/error.js'
import { dispatchChat, readTimeoutStream } from '../src/provider.js'
import { RetryPolicy } from '../src/retry.js'
import type { ChatRequest } from '../src/types.js'

describe('ClientBuilder', () => {
  beforeEach(() => {
    delete process.env.ANTHROPIC_API_KEY
    delete process.env.OPENAI_API_KEY
    delete process.env.MINIMAX_API_KEY
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
    for (const p of ['anthropic', 'openai', 'minimax'] as const) {
      const client = new ClientBuilder().provider(p).apiKey('test-key').build()
      expect(client).toBeInstanceOf(Client)
    }
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

  it('silently terminates a stalled stream without throwing', async () => {
    const stalled = (async function* () {
      await new Promise(() => {})
      yield { content: 'never', done: false, eventType: 'text' as const }
    })()

    const wrapped = readTimeoutStream(stalled, 0.05)
    const results: unknown[] = []
    for await (const event of wrapped) {
      results.push(event)
    }
    expect(results.length).toBe(0)
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

    const results: unknown[] = []
    for await (const event of readTimeoutStream(stalled, 0.05)) {
      results.push(event)
    }
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(results).toEqual([])
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

    const results: unknown[] = []
    for await (const event of readTimeoutStream(stalled, 0.05)) {
      results.push(event)
    }

    expect(results).toEqual([])
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
