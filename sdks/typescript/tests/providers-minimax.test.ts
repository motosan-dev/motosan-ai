import { afterEach, describe, expect, it, vi } from 'vitest'
import { DEFAULT_MINIMAX_MODEL } from '../src/models.js'
import { MinimaxProvider } from '../src/providers/minimax.js'
import { textOnly } from '../src/provider.js'
import type { ChatRequest } from '../src/types.js'

function anthropicResponse(model = DEFAULT_MINIMAX_MODEL) {
  return new Response(
    JSON.stringify({
      id: 'msg_1',
      type: 'message',
      role: 'assistant',
      content: [{ type: 'text', text: 'ok' }],
      model,
      stop_reason: 'end_turn',
      usage: { input_tokens: 10, output_tokens: 5 },
    }),
    { status: 200, headers: { 'content-type': 'application/json' } },
  )
}

describe('MinimaxProvider (Anthropic-compat wire)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('posts an Anthropic-wire body to the default {base}/v1/messages', async () => {
    let url = ''
    let headers: Record<string, string> = {}
    let body: any
    vi.stubGlobal(
      'fetch',
      vi.fn(async (u: string, options?: RequestInit) => {
        url = u
        headers = (options?.headers as Record<string, string>) ?? {}
        body = JSON.parse(String(options?.body ?? '{}'))
        return anthropicResponse()
      }),
    )

    const provider = new MinimaxProvider('mm-key')
    const request: ChatRequest = {
      messages: [{ role: 'user', content: 'What is 2+2?' }],
      system: 'You are helpful.',
      maxTokens: 256,
    }
    const response = await provider.chat(request)

    // Default URL is the Anthropic-compat base + /v1/messages, NOT the legacy endpoint.
    expect(url).toBe('https://api.minimax.io/anthropic/v1/messages')
    expect(url).not.toContain('chatcompletion_v2')

    // x-api-key auth (NOT Authorization: Bearer), plus anthropic-version.
    expect(headers['x-api-key']).toBe('mm-key')
    expect('authorization' in headers).toBe(false)
    expect(headers['anthropic-version']).toBe('2023-06-01')

    // Anthropic wire body: top-level `system` string, default model, messages array.
    expect(body.model).toBe(DEFAULT_MINIMAX_MODEL)
    expect(body.system).toBe('You are helpful.')
    expect(body.max_tokens).toBe(256)
    expect(body.messages[0]).toEqual({ role: 'user', content: 'What is 2+2?' })

    // Anthropic-style response parse.
    expect(response.content).toBe('ok')
    expect(response.stopReason).toBe('end_turn')
    expect(response.usage.inputTokens).toBe(10)
    expect(response.usage.outputTokens).toBe(5)
  })

  it('default model is MiniMax-M2.7', async () => {
    let body: any
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_u: string, options?: RequestInit) => {
        body = JSON.parse(String(options?.body ?? '{}'))
        return anthropicResponse()
      }),
    )
    await new MinimaxProvider('mm-key').chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(body.model).toBe('MiniMax-M2.7')
    expect(DEFAULT_MINIMAX_MODEL).toBe('MiniMax-M2.7')
  })

  it('respects a custom base URL → {custom}/v1/messages', async () => {
    let url = ''
    vi.stubGlobal(
      'fetch',
      vi.fn(async (u: string) => {
        url = u
        return anthropicResponse()
      }),
    )
    const provider = new MinimaxProvider('mm-key', 'MiniMax-M2.7', 'https://proxy.example/anthropic')
    await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(url).toBe('https://proxy.example/anthropic/v1/messages')
  })

  it('reports text-only capabilities', () => {
    const provider = new MinimaxProvider('mm-key')
    expect(provider.capabilities()).toEqual(textOnly())
  })

  it('streams via the Anthropic SSE adapter and posts stream:true', async () => {
    let body: any
    const sse =
      'event: message_start\ndata: {"message":{"usage":{"input_tokens":1,"output_tokens":0}}}\n\n' +
      'event: content_block_delta\ndata: {"delta":{"type":"text_delta","text":"hello"}}\n\n' +
      'event: message_stop\ndata: {}\n\n'
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_u: string, options?: RequestInit) => {
        body = JSON.parse(String(options?.body ?? '{}'))
        return new Response(sse, {
          status: 200,
          headers: { 'content-type': 'text/event-stream' },
        })
      }),
    )
    const provider = new MinimaxProvider('mm-key')
    const texts: string[] = []
    for await (const e of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      if (e.eventType === 'text' && e.content) texts.push(e.content)
    }
    expect(body.stream).toBe(true)
    expect(texts.join('')).toBe('hello')
  })
})

// Env-gated live test — skipped unless MINIMAX_API_KEY is set.
const liveKey = process.env.MINIMAX_API_KEY
const liveDescribe = liveKey ? describe : describe.skip
liveDescribe('MinimaxProvider live', () => {
  it('chats against the real MiniMax Anthropic-compat endpoint', async () => {
    const provider = new MinimaxProvider(liveKey as string)
    const response = await provider.chat({
      messages: [{ role: 'user', content: 'Reply with the single word: pong' }],
      maxTokens: 16,
    })
    expect(typeof response.content).toBe('string')
    expect(response.content.length).toBeGreaterThan(0)
  })
})
