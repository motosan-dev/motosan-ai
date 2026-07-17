import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { GeminiProvider } from '../src/providers/gemini.js'
import { collectStream } from '../src/stream.js'
import { IncompleteStreamError, UnsupportedFeatureError } from '../src/error.js'
import { validateRequest } from '../src/provider.js'
import type { ChatRequest, StreamEvent } from '../src/types.js'

const ID_RE = /^call_\d+$/

describe('GeminiProvider — capabilities', () => {
  it('supports image, not document, not MCP (gemini.rs:315-317; post-M4 withImage)', () => {
    const caps = new GeminiProvider('key').capabilities()
    expect(caps.supportsImage).toBe(true)
    expect(caps.supportsDocument).toBe(false)
    expect(caps.supportsMcp).toBe(false)
  })
})

describe('GeminiProvider — chat URL, auth, and text parse', () => {
  let captured: { url: string; headers: Record<string, string>; body: any } | null = null
  beforeEach(() => {
    captured = null
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('uses :generateContent path, x-goog-api-key auth, and model in the URL (gemini.rs:64-77)', async () => {
    const mockFetch = vi.fn(async (url: string, options?: RequestInit) => {
      captured = {
        url,
        headers: (options?.headers as Record<string, string>) ?? {},
        body: options?.body ? JSON.parse(String(options.body)) : null,
      }
      return new Response(
        JSON.stringify({
          candidates: [
            {
              content: { parts: [{ text: 'Hi!' }], role: 'model' },
              finishReason: 'STOP',
            },
          ],
          usageMetadata: { promptTokenCount: 5, candidatesTokenCount: 2 },
          modelVersion: 'gemini-2.5-flash',
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      )
    })
    vi.stubGlobal('fetch', mockFetch)

    const provider = new GeminiProvider('fake-key')
    const resp = await provider.chat({ messages: [{ role: 'user', content: 'Hello' }] })

    expect(captured?.url).toBe(
      'https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent',
    )
    expect(captured?.headers['x-goog-api-key']).toBe('fake-key')
    expect(captured?.headers['authorization']).toBeUndefined()
    // model lives in URL path, NOT in body
    expect(captured?.body.model).toBeUndefined()
    expect(resp.content).toBe('Hi!')
    expect(resp.stopReason).toBe('end_turn') // STOP -> end_turn (NOT 'stop')
    expect(resp.model).toBe('gemini-2.5-flash')
    expect(resp.usage.inputTokens).toBe(5)
    expect(resp.usage.outputTokens).toBe(2)
    expect(resp.toolCalls).toEqual([])
  })

  it('request.model overrides the provider default in the URL path (gemini.rs:64,69)', async () => {
    const mockFetch = vi.fn(async (url: string) => {
      captured = { url, headers: {}, body: null }
      return new Response(
        JSON.stringify({ candidates: [{ content: { parts: [{ text: 'ok' }] }, finishReason: 'STOP' }] }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      )
    })
    vi.stubGlobal('fetch', mockFetch)
    const provider = new GeminiProvider('k')
    await provider.chat({ messages: [{ role: 'user', content: 'hi' }], model: 'gemini-2.5-pro' })
    expect(captured?.url).toContain('/models/gemini-2.5-pro:generateContent')
  })

  it('parses a functionCall into a ToolCall with a client-generated id (gemini.rs:677-695)', async () => {
    const mockFetch = vi.fn(async () =>
      new Response(
        JSON.stringify({
          candidates: [
            {
              content: {
                parts: [{ functionCall: { name: 'search', args: { q: 'rust' } } }],
                role: 'model',
              },
              finishReason: 'STOP',
            },
          ],
          usageMetadata: { promptTokenCount: 8, candidatesTokenCount: 3 },
          modelVersion: 'gemini-2.5-flash',
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      ),
    )
    vi.stubGlobal('fetch', mockFetch)
    const resp = await new GeminiProvider('k').chat({
      messages: [{ role: 'user', content: 'find' }],
    })
    expect(resp.toolCalls.length).toBe(1)
    expect(resp.toolCalls[0].name).toBe('search')
    expect(resp.toolCalls[0].input.q).toBe('rust') // input from wire `args`
    expect(resp.toolCalls[0].id).toMatch(ID_RE)
    expect(resp.stopReason).toBe('tool_use') // STOP + tool calls
  })

  it('MAX_TOKENS finishReason -> max_tokens (gemini.rs:697-708)', async () => {
    const mockFetch = vi.fn(async () =>
      new Response(
        JSON.stringify({
          candidates: [
            { content: { parts: [{ text: 'truncated' }] }, finishReason: 'MAX_TOKENS' },
          ],
          usageMetadata: { promptTokenCount: 5, candidatesTokenCount: 100 },
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      ),
    )
    vi.stubGlobal('fetch', mockFetch)
    const resp = await new GeminiProvider('k').chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(resp.stopReason).toBe('max_tokens')
  })

  it('SAFETY finishReason (no tool calls) -> other (gemini.rs:1032-1043)', async () => {
    const mockFetch = vi.fn(async () =>
      new Response(
        JSON.stringify({
          candidates: [{ content: { parts: [{ text: '' }] }, finishReason: 'SAFETY' }],
          usageMetadata: { promptTokenCount: 2, candidatesTokenCount: 0 },
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      ),
    )
    vi.stubGlobal('fetch', mockFetch)
    const resp = await new GeminiProvider('k').chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(resp.stopReason).toBe('other')
  })

  it('multiple tool calls get unique non-empty ids (gemini.rs:943-983)', async () => {
    const mockFetch = vi.fn(async () =>
      new Response(
        JSON.stringify({
          candidates: [
            {
              content: {
                parts: [
                  { functionCall: { name: 'a', args: {} } },
                  { functionCall: { name: 'b', args: {} } },
                ],
              },
              finishReason: 'STOP',
            },
          ],
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      ),
    )
    vi.stubGlobal('fetch', mockFetch)
    const resp = await new GeminiProvider('k').chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(resp.toolCalls.length).toBe(2)
    expect(resp.toolCalls[0].id).toMatch(ID_RE)
    expect(resp.toolCalls[1].id).toMatch(ID_RE)
    expect(resp.toolCalls[0].id).not.toBe(resp.toolCalls[1].id)
  })

  it('mixed text + functionCall -> tool_use, content preserved (gemini.rs:985-1004)', async () => {
    const mockFetch = vi.fn(async () =>
      new Response(
        JSON.stringify({
          candidates: [
            {
              content: {
                parts: [
                  { text: 'Let me search for that.' },
                  { functionCall: { name: 'search', args: { q: 'test' } } },
                ],
              },
              finishReason: 'STOP',
            },
          ],
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      ),
    )
    vi.stubGlobal('fetch', mockFetch)
    const resp = await new GeminiProvider('k').chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(resp.content).toBe('Let me search for that.')
    expect(resp.toolCalls.length).toBe(1)
    expect(resp.stopReason).toBe('tool_use')
  })

  it('missing usageMetadata yields zero tokens (gemini.rs:1020-1030)', async () => {
    const mockFetch = vi.fn(async () =>
      new Response(
        JSON.stringify({
          candidates: [{ content: { parts: [{ text: 'hi' }] }, finishReason: 'STOP' }],
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      ),
    )
    vi.stubGlobal('fetch', mockFetch)
    const resp = await new GeminiProvider('k').chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(resp.usage.inputTokens).toBe(0)
    expect(resp.usage.outputTokens).toBe(0)
  })

  it('falls back to the resolved request model when modelVersion absent (gemini.rs:298-302)', async () => {
    const mockFetch = vi.fn(async () =>
      new Response(
        JSON.stringify({
          candidates: [{ content: { parts: [{ text: 'hi' }] }, finishReason: 'STOP' }],
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      ),
    )
    vi.stubGlobal('fetch', mockFetch)
    const resp = await new GeminiProvider('k', 'gemini-1.5-pro').chat({
      messages: [{ role: 'user', content: 'hi' }],
    })
    expect(resp.model).toBe('gemini-1.5-pro')
  })

  it('sends a base64 image as inlineData (no validation error, image cap)', async () => {
    const mockFetch = vi.fn(async (_url: string, options?: RequestInit) => {
      captured = {
        url: _url,
        headers: {},
        body: options?.body ? JSON.parse(String(options.body)) : null,
      }
      return new Response(
        JSON.stringify({ candidates: [{ content: { parts: [{ text: 'seen' }] }, finishReason: 'STOP' }] }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      )
    })
    vi.stubGlobal('fetch', mockFetch)
    const req: ChatRequest = {
      messages: [
        {
          role: 'user',
          content: '',
          contentBlocks: [
            { type: 'image', source: { type: 'base64', mediaType: 'image/png', data: 'abc' } },
          ],
        },
      ],
    }
    const resp = await new GeminiProvider('k').chat(req)
    const part = captured?.body.contents[0].parts[0]
    expect(part.inlineData.mimeType).toBe('image/png')
    expect(part.inlineData.data).toBe('abc')
    expect(resp.content).toBe('seen')
  })
})

describe('GeminiProvider — document rejection (validateRequest gate)', () => {
  it('document blocks are rejected by validateRequest before any HTTP call', () => {
    const provider = new GeminiProvider('k')
    const req: ChatRequest = {
      messages: [
        {
          role: 'user',
          content: '',
          contentBlocks: [
            { type: 'document', source: { type: 'url', url: 'https://x.com/d.pdf' } },
          ],
        },
      ],
    }
    // The provider's own capabilities drive validateRequest (provider.ts:59-71).
    expect(() => validateRequest(req, provider.capabilities())).toThrow(UnsupportedFeatureError)
  })
})

describe('GeminiProvider — SSE stream (no [DONE]; finishReason terminates)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('emits text events then a finishReason-driven done; collectStream reassembles (gemini.rs:1127-1154)', async () => {
    const mockFetch = vi.fn(async () => {
      const sse = [
        'data: {"candidates":[{"content":{"parts":[{"text":"Hi"}],"role":"model"}}]}\n\n',
        'data: {"candidates":[{"content":{"parts":[{"text":" there"}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2}}\n\n',
      ].join('')
      return new Response(sse, { status: 200, headers: { 'content-type': 'text/event-stream' } })
    })
    vi.stubGlobal('fetch', mockFetch)

    const provider = new GeminiProvider('fake-key')
    const events: StreamEvent[] = []
    for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'Hello' }] })) {
      events.push(evt)
    }

    const texts = events.filter((e) => e.eventType === 'text' && !e.done)
    expect(texts.map((e) => e.content).join('')).toBe('Hi there')
    const last = events[events.length - 1]
    expect(last.done).toBe(true)
    expect(last.stopReason).toBe('end_turn')

    // Round-trip through collectStream (re-drive a fresh stream).
    const resp = await collectStream(
      provider.stream({ messages: [{ role: 'user', content: 'Hello' }] }),
    )
    expect(resp.content).toBe('Hi there')
    expect(resp.stopReason).toBe('end_turn')
  })

  it('synthesizes start/argsWithId/end for a streamed functionCall in ONE args chunk (gemini.rs:477-493)', async () => {
    const mockFetch = vi.fn(async () => {
      const sse = [
        'data: {"candidates":[{"content":{"parts":[{"functionCall":{"name":"search","args":{"q":"x"}}}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1}}\n\n',
      ].join('')
      return new Response(sse, { status: 200, headers: { 'content-type': 'text/event-stream' } })
    })
    vi.stubGlobal('fetch', mockFetch)

    const provider = new GeminiProvider('k')
    const events: StreamEvent[] = []
    for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'find' }] })) {
      events.push(evt)
    }

    const start = events.find((e) => e.eventType === 'tool_call_start')
    const args = events.find((e) => e.eventType === 'tool_call_args')
    const end = events.find((e) => e.eventType === 'tool_call_end')
    expect(start?.toolCallId).toMatch(ID_RE)
    expect(start?.toolCallName).toBe('search')
    // args is the WHOLE JSON serialized in one shot (not incremental)
    expect(args?.toolCallArgsDelta).toBe('{"q":"x"}')
    expect(args?.toolCallId).toBe(start?.toolCallId)
    expect(end?.toolCallId).toBe(start?.toolCallId)
    const last = events[events.length - 1]
    expect(last.done).toBe(true)
    expect(last.stopReason).toBe('tool_use') // STOP + tool calls

    // collectStream reassembles the ToolCall with input from the serialized args.
    const resp = await collectStream(
      provider.stream({ messages: [{ role: 'user', content: 'find' }] }),
    )
    expect(resp.toolCalls.length).toBe(1)
    expect(resp.toolCalls[0].name).toBe('search')
    expect(resp.toolCalls[0].input.q).toBe('x')
  })

  it('skips a defensive [DONE] line and throws IncompleteStreamError on EOF without finishReason (M3/E2)', async () => {
    const mockFetch = vi.fn(async () => {
      // No finishReason anywhere; a stray [DONE] must be ignored; stream ends on EOF.
      const sse = [
        'data: {"candidates":[{"content":{"parts":[{"text":"partial"}],"role":"model"}}]}\n\n',
        'data: [DONE]\n\n',
      ].join('')
      return new Response(sse, { status: 200, headers: { 'content-type': 'text/event-stream' } })
    })
    vi.stubGlobal('fetch', mockFetch)

    const provider = new GeminiProvider('k')
    const events: StreamEvent[] = []
    let error: unknown
    try {
      for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        events.push(evt)
      }
    } catch (e) {
      error = e
    }
    // Exactly one text event; NO fabricated done (Gemini adapter never emits a
    // defensive EOF done — only finishReason drives it).
    expect(events.filter((e) => e.eventType === 'text' && !e.done).map((e) => e.content)).toEqual([
      'partial',
    ])
    expect(events.some((e) => e.done)).toBe(false)
    expect(error).toBeInstanceOf(IncompleteStreamError)

    await expect(
      collectStream(provider.stream({ messages: [{ role: 'user', content: 'hi' }] })),
    ).rejects.toBeInstanceOf(IncompleteStreamError)
  })

  it('emits a usage event from usageMetadata (gemini.rs:496-511)', async () => {
    const mockFetch = vi.fn(async () => {
      const sse =
        'data: {"candidates":[{"content":{"parts":[{"text":"x"}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":7,"candidatesTokenCount":4}}\n\n'
      return new Response(sse, { status: 200, headers: { 'content-type': 'text/event-stream' } })
    })
    vi.stubGlobal('fetch', mockFetch)
    const provider = new GeminiProvider('k')
    const events: StreamEvent[] = []
    for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      events.push(evt)
    }
    const usage = events.find((e) => e.eventType === 'usage')
    expect(usage?.usage?.inputTokens).toBe(7)
    expect(usage?.usage?.outputTokens).toBe(4)
  })
})

describe('GeminiProvider — retry', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('chat retries on 429 then succeeds (gemini.rs:1086-1125)', async () => {
    let calls = 0
    const mockFetch = vi.fn(async () => {
      calls += 1
      if (calls === 1) {
        return new Response(JSON.stringify({ error: { message: 'rate limited' } }), {
          status: 429,
          headers: { 'content-type': 'application/json' },
        })
      }
      return new Response(
        JSON.stringify({
          candidates: [{ content: { parts: [{ text: 'ok' }] }, finishReason: 'STOP' }],
          usageMetadata: { promptTokenCount: 1, candidatesTokenCount: 1 },
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      )
    })
    vi.stubGlobal('fetch', mockFetch)

    const { RetryPolicy } = await import('../src/retry.js')
    const provider = new GeminiProvider('k').withRetryPolicy(
      new RetryPolicy({ maxRetries: 2, baseDelayMs: 1, maxDelayMs: 10, jitter: false }),
    )
    const resp = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(resp.content).toBe('ok')
    expect(calls).toBe(2)
  })

  it('chat throws a mapped error on a non-retryable 400', async () => {
    const mockFetch = vi.fn(async () =>
      new Response(JSON.stringify({ error: { message: 'bad' } }), {
        status: 400,
        headers: { 'content-type': 'application/json' },
      }),
    )
    vi.stubGlobal('fetch', mockFetch)
    await expect(
      new GeminiProvider('k').chat({ messages: [{ role: 'user', content: 'hi' }] }),
    ).rejects.toMatchObject({ status: 400 })
  })
})
