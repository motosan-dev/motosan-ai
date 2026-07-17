import { afterEach, describe, expect, it, vi } from 'vitest'
import { Client, ClientBuilder } from '../src/client.js'
import { CancelledError, StreamReadTimeoutError } from '../src/error.js'
import { readTimeoutStream } from '../src/provider.js'
import { RetryPolicy } from '../src/retry.js'

function immediateRetryPolicy(): RetryPolicy {
  return new RetryPolicy({
    maxRetries: 2,
    baseDelayMs: 0,
    maxDelayMs: 0,
    jitter: false,
    respectRetryAfter: false,
  })
}

function anthropicPayload(text: string): string {
  return JSON.stringify({
    content: [{ type: 'text', text }],
    model: 'claude-sonnet-4-6',
    usage: { input_tokens: 1, output_tokens: 2 },
    stop_reason: 'end_turn',
  })
}

function abortError(): Error {
  const error = new Error('This operation was aborted')
  error.name = 'AbortError'
  return error
}

describe('E7: readTimeoutStream throws on idle expiry', () => {
  it('throws StreamReadTimeoutError instead of ending silently', async () => {
    const stalled = (async function* () {
      await new Promise(() => {})
      yield { content: 'never', done: false, eventType: 'text' as const }
    })()

    await expect(async () => {
      for await (const _ of readTimeoutStream(stalled, 0.05)) void _
    }).rejects.toThrow(StreamReadTimeoutError)
  })

  it('Client.stream applies readIdleMs by default and throws on a stalled provider', async () => {
    const fakeProvider = {
      capabilities: () => ({ supportsImage: false, supportsDocument: false, supportsMcp: false }),
      async chat(): Promise<never> {
        throw new Error('unused')
      },
      async *stream() {
        yield { content: 'a', done: false, eventType: 'text' as const }
        await new Promise(() => {})
      },
    }

    const client = new Client(fakeProvider, { readIdleMs: 50 })
    await expect(async () => {
      for await (const _ of client.stream({ messages: [{ role: 'user', content: 'hi' }] })) void _
    }).rejects.toThrow(StreamReadTimeoutError)
  })
})

describe('E6/E4: per-request cancellation and timeout composition', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('caller abort mid-request throws CancelledError after exactly 1 fetch (never retried)', async () => {
    const controller = new AbortController()
    let calls = 0
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, init?: RequestInit) => {
        calls += 1
        expect(init?.signal).toBeDefined()
        controller.abort()
        throw abortError()
      }),
    )

    const client = new ClientBuilder()
      .provider('anthropic')
      .apiKey('test-key')
      .retryPolicy(immediateRetryPolicy())
      .build()

    await expect(
      client.chat({ messages: [{ role: 'user', content: 'hi' }] }, { signal: controller.signal }),
    ).rejects.toThrow(CancelledError)
    expect(calls).toBe(1)
  })

  it('AbortSignal.timeout expiry (no caller signal) stays retryable', async () => {
    let calls = 0
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        calls += 1
        if (calls === 1) {
          const error = new Error('The operation was aborted due to timeout')
          error.name = 'TimeoutError'
          throw error
        }
        return new Response(anthropicPayload('ok'), { status: 200 })
      }),
    )

    const client = new ClientBuilder()
      .provider('anthropic')
      .apiKey('test-key')
      .retryPolicy(immediateRetryPolicy())
      .timeouts({ totalMs: 5_000 })
      .build()

    const response = await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(response.content).toBe('ok')
    expect(calls).toBe(2)
  })

  it('default timeouts never abort a slow-but-successful chat (50ms to headers)', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        await new Promise((resolve) => setTimeout(resolve, 50))
        return new Response(anthropicPayload('ok'), { status: 200 })
      }),
    )
    const client = new ClientBuilder().provider('anthropic').apiKey('test-key').build()
    const response = await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(response.content).toBe('ok')
  })

  it('connect budget disarms at headers: a body slower than connectMs+readIdleMs still succeeds', async () => {
    let capturedSignal: AbortSignal | undefined
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, init?: RequestInit) => {
        capturedSignal = init?.signal ?? undefined
        const body = new ReadableStream<Uint8Array>({
          async start(c) {
            await new Promise((resolve) => setTimeout(resolve, 80))
            c.enqueue(new TextEncoder().encode(anthropicPayload('slow ok')))
            c.close()
          },
        })
        return new Response(body, { status: 200 })
      }),
    )

    const client = new ClientBuilder()
      .provider('anthropic')
      .apiKey('test-key')
      .retryPolicy(immediateRetryPolicy())
      .timeouts({ connectMs: 10, readIdleMs: 20 })
      .build()

    const response = await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(response.content).toBe('slow ok')
    expect(capturedSignal?.aborted).toBe(false)
  })

  it('caller abort mid-stream surfaces CancelledError, not a raw AbortError', async () => {
    const controller = new AbortController()
    const sse =
      'event: content_block_delta\ndata: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}\n\n'
    const body = new ReadableStream<Uint8Array>({
      start(c) {
        c.enqueue(new TextEncoder().encode(sse))
      },
      pull() {
        controller.abort()
        throw abortError()
      },
    })
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(body, { status: 200, headers: { 'content-type': 'text/event-stream' } })),
    )

    const client = new ClientBuilder()
      .provider('anthropic')
      .apiKey('test-key')
      .retryPolicy(immediateRetryPolicy())
      .build()

    await expect(async () => {
      for await (const _ of client.stream(
        { messages: [{ role: 'user', content: 'hi' }] },
        { signal: controller.signal },
      ))
        void _
    }).rejects.toThrow(CancelledError)
  })
})
