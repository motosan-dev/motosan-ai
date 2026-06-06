import { afterEach, describe, expect, it, vi } from 'vitest'
import { InvalidRequestError, mapHttpError } from '../src/error.js'
import { AnthropicProvider } from '../src/providers/anthropic.js'
import { MinimaxProvider } from '../src/providers/minimax.js'
import { OpenAIProvider } from '../src/providers/openai.js'
import { RetryPolicy } from '../src/retry.js'
import type { StreamEvent } from '../src/types.js'

function immediateRetryPolicy(): RetryPolicy {
  return new RetryPolicy({
    maxRetries: 2,
    baseDelayMs: 0,
    maxDelayMs: 0,
    jitter: false,
    respectRetryAfter: false,
  })
}

function anthropicSuccess(): Response {
  return new Response(
    JSON.stringify({
      content: [{ type: 'text', text: 'anthropic ok' }],
      model: 'claude-sonnet-4-6',
      usage: { input_tokens: 1, output_tokens: 2 },
      stop_reason: 'end_turn',
    }),
    { status: 200 },
  )
}

function openAiSuccess(): Response {
  return new Response(
    JSON.stringify({
      choices: [
        { message: { content: 'openai ok', tool_calls: [] }, finish_reason: 'stop' },
      ],
      model: 'gpt-4o',
      usage: { prompt_tokens: 1, completion_tokens: 2 },
    }),
    { status: 200 },
  )
}

function minimaxSuccess(): Response {
  return new Response(
    JSON.stringify({
      choices: [
        { message: { content: 'minimax ok', tool_calls: [] }, finish_reason: 'stop' },
      ],
      model: 'MiniMax-M2.7',
      usage: { prompt_tokens: 1, completion_tokens: 2 },
    }),
    { status: 200 },
  )
}

function rateLimit(headers?: HeadersInit): Response {
  return new Response(JSON.stringify({ error: { message: 'rate limited' } }), {
    status: 429,
    headers,
  })
}

function anthropicSseResponse(): Response {
  const sse =
    'event: content_block_start\n' +
    'data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\n' +
    'event: content_block_delta\n' +
    'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"stream ok"}}\n\n' +
    'event: content_block_stop\n' +
    'data: {"type":"content_block_stop","index":0}\n\n' +
    'event: message_stop\n' +
    'data: {"type":"message_stop"}\n\n'
  return new Response(sse, {
    status: 200,
    headers: { 'content-type': 'text/event-stream' },
  })
}

describe('retry provider integration', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    vi.clearAllMocks()
    vi.useRealTimers()
  })

  it('mapHttpError attaches numeric status and retry-after for retry classifiers', () => {
    const rateLimitError = mapHttpError(429, 'rate limited', '2') as {
      status?: number
      retryAfterMs?: number
    }
    expect(rateLimitError.status).toBe(429)
    expect(rateLimitError.retryAfterMs).toBe(2000)
    expect((mapHttpError(400, 'bad request') as { status?: number }).status).toBe(400)
  })

  it('Anthropic chat honors Retry-After when retrying a 429 response', async () => {
    vi.useFakeTimers()
    let calls = 0
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        calls += 1
        return calls === 1 ? rateLimit({ 'retry-after': '2' }) : anthropicSuccess()
      }),
    )
    const setTimeoutSpy = vi.spyOn(globalThis, 'setTimeout')

    const provider = new AnthropicProvider('test-key').withRetryPolicy(
      new RetryPolicy({
        maxRetries: 1,
        baseDelayMs: 0,
        maxDelayMs: 0,
        jitter: false,
        respectRetryAfter: true,
      }),
    )
    const responsePromise = provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
    await vi.runAllTimersAsync()

    await expect(responsePromise).resolves.toMatchObject({ content: 'anthropic ok' })
    expect(calls).toBe(2)
    expect(setTimeoutSpy).toHaveBeenCalledWith(expect.any(Function), 2000)
  })

  it('Anthropic chat retries a 429 response and succeeds on the next attempt', async () => {
    let calls = 0
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        calls += 1
        return calls === 1 ? rateLimit() : anthropicSuccess()
      }),
    )

    const provider = new AnthropicProvider('test-key').withRetryPolicy(immediateRetryPolicy())
    const response = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })

    expect(calls).toBe(2)
    expect(response.content).toBe('anthropic ok')
  })

  it('Anthropic chat does not retry a 400 response', async () => {
    let calls = 0
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        calls += 1
        return new Response(JSON.stringify({ error: { message: 'invalid request' } }), {
          status: 400,
        })
      }),
    )

    const provider = new AnthropicProvider('test-key').withRetryPolicy(immediateRetryPolicy())

    await expect(provider.chat({ messages: [{ role: 'user', content: 'hi' }] })).rejects.toThrow(
      InvalidRequestError,
    )
    expect(calls).toBe(1)
  })

  it('OpenAI chat retries a 429 response and succeeds on the next attempt', async () => {
    let calls = 0
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        calls += 1
        return calls === 1 ? rateLimit() : openAiSuccess()
      }),
    )

    const provider = new OpenAIProvider('test-key').withRetryPolicy(immediateRetryPolicy())
    const response = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })

    expect(calls).toBe(2)
    expect(response.content).toBe('openai ok')
  })

  it('Minimax chat retries a 429 response and succeeds on the next attempt', async () => {
    let calls = 0
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        calls += 1
        return calls === 1 ? rateLimit() : minimaxSuccess()
      }),
    )

    const provider = new MinimaxProvider('test-key').withRetryPolicy(immediateRetryPolicy())
    const response = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })

    expect(calls).toBe(2)
    expect(response.content).toBe('minimax ok')
  })

  it('Anthropic stream retries the initial fetch after 429', async () => {
    let calls = 0
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        calls += 1
        return calls === 1 ? rateLimit() : anthropicSseResponse()
      }),
    )

    const provider = new AnthropicProvider('test-key').withRetryPolicy(immediateRetryPolicy())
    const events: StreamEvent[] = []
    for await (const event of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      events.push(event)
    }

    expect(calls).toBe(2)
    expect(events.filter((event) => !event.done).map((event) => event.content)).toEqual([
      'stream ok',
    ])
  })

  it('Anthropic stream does not retry after the response body has been returned', async () => {
    let calls = 0
    const bytes = new TextEncoder().encode(
      'event: content_block_delta\n' +
        'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"before break"}}\n\n',
    )
    const brokenStream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(bytes)
        controller.error(new Error('mid-stream break'))
      },
    })
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        calls += 1
        return new Response(brokenStream, {
          status: 200,
          headers: { 'content-type': 'text/event-stream' },
        })
      }),
    )

    const provider = new AnthropicProvider('test-key').withRetryPolicy(immediateRetryPolicy())
    const events: StreamEvent[] = []
    try {
      for await (const event of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        events.push(event)
      }
    } catch (error) {
      expect(error).toBeInstanceOf(Error)
    }

    expect(calls).toBe(1)
  })

  // --- OpenAI / MiniMax retry coverage (the plan's named high-risk spot) ---

  function openAiSse(): Response {
    const sse =
      'data: {"choices":[{"index":0,"delta":{"content":"stream ok"},"finish_reason":null}]}\n\n' +
      'data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}\n\n' +
      'data: [DONE]\n\n'
    return new Response(sse, { status: 200, headers: { 'content-type': 'text/event-stream' } })
  }

  function errorResponse(status: number): Response {
    return new Response(JSON.stringify({ error: { message: `http ${status}` } }), { status })
  }

  for (const status of [400, 401]) {
    it(`OpenAI chat does not retry a ${status} response`, async () => {
      let calls = 0
      vi.stubGlobal('fetch', vi.fn(async () => { calls += 1; return errorResponse(status) }))
      const provider = new OpenAIProvider('test-key').withRetryPolicy(immediateRetryPolicy())
      await expect(provider.chat({ messages: [{ role: 'user', content: 'hi' }] })).rejects.toThrow()
      expect(calls).toBe(1)
    })
  }

  it('OpenAI stream retries the initial fetch after 429', async () => {
    let calls = 0
    vi.stubGlobal('fetch', vi.fn(async () => { calls += 1; return calls === 1 ? rateLimit() : openAiSse() }))
    const provider = new OpenAIProvider('test-key').withRetryPolicy(immediateRetryPolicy())
    const events: StreamEvent[] = []
    for await (const event of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      events.push(event)
    }
    expect(calls).toBe(2)
    expect(events.filter((e) => !e.done).map((e) => e.content)).toContain('stream ok')
  })

  it('OpenAI stream does not retry a 400 initial fetch', async () => {
    let calls = 0
    vi.stubGlobal('fetch', vi.fn(async () => { calls += 1; return errorResponse(400) }))
    const provider = new OpenAIProvider('test-key').withRetryPolicy(immediateRetryPolicy())
    await expect(async () => {
      for await (const _ of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) { void _ }
    }).rejects.toThrow()
    expect(calls).toBe(1)
  })

  it('OpenAI stream does not retry after the response body has been returned', async () => {
    let calls = 0
    const broken = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode('data: {"choices":[{"delta":{"content":"x"}}]}\n\n'))
        controller.error(new Error('mid-stream break'))
      },
    })
    vi.stubGlobal('fetch', vi.fn(async () => {
      calls += 1
      return new Response(broken, { status: 200, headers: { 'content-type': 'text/event-stream' } })
    }))
    const provider = new OpenAIProvider('test-key').withRetryPolicy(immediateRetryPolicy())
    try {
      for await (const _ of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) { void _ }
    } catch (error) {
      expect(error).toBeInstanceOf(Error)
    }
    expect(calls).toBe(1)
  })

  it('OpenAI stream rethrows a 404 and never falls back to the Responses API', async () => {
    let calls = 0
    vi.stubGlobal('fetch', vi.fn(async () => { calls += 1; return errorResponse(404) }))
    const provider = new OpenAIProvider('test-key')
      .withRetryPolicy(immediateRetryPolicy())
      .withResponsesFallback(true)
    await expect(async () => {
      for await (const _ of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) { void _ }
    }).rejects.toThrow()
    expect(calls).toBe(1) // no second call to the Responses endpoint
  })

  it('Minimax chat does not retry a 400 response', async () => {
    let calls = 0
    vi.stubGlobal('fetch', vi.fn(async () => { calls += 1; return errorResponse(400) }))
    const provider = new MinimaxProvider('test-key').withRetryPolicy(immediateRetryPolicy())
    await expect(provider.chat({ messages: [{ role: 'user', content: 'hi' }] })).rejects.toThrow()
    expect(calls).toBe(1)
  })

  it('Minimax stream retries the initial fetch after 429', async () => {
    let calls = 0
    vi.stubGlobal('fetch', vi.fn(async () => { calls += 1; return calls === 1 ? rateLimit() : openAiSse() }))
    const provider = new MinimaxProvider('test-key').withRetryPolicy(immediateRetryPolicy())
    const events: StreamEvent[] = []
    for await (const event of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      events.push(event)
    }
    expect(calls).toBe(2)
    expect(events.filter((e) => !e.done).map((e) => e.content)).toContain('stream ok')
  })
})
