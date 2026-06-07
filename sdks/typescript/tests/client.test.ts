import { afterEach, describe, expect, it, vi } from 'vitest'
import { Client } from '../src/client.js'
import type { ChatRequest, ChatResponse, StreamEvent } from '../src/types.js'

class FakeProvider {
  lastRequest?: ChatRequest

  async chat(request: ChatRequest): Promise<ChatResponse> {
    this.lastRequest = request
    return {
      content: 'ok',
      toolCalls: [],
      model: 'fake',
      usage: { inputTokens: 1, outputTokens: 1 },
      stopReason: 'stop'
    }
  }

  async *stream(request: ChatRequest): AsyncGenerator<StreamEvent> {
    this.lastRequest = request
    yield { content: 'h', done: false }
    yield { content: 'i', done: false }
    yield { content: '', done: true }
  }
}

describe('client', () => {
  it('routes chat/stream to provider', async () => {
    const fake = new FakeProvider()
    const client = new Client({ provider: fake })

    const response = await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(response.content).toBe('ok')

    const chunks: string[] = []
    for await (const ev of client.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      if (!ev.done) chunks.push(ev.content)
    }
    expect(chunks.join('')).toBe('hi')
  })

  it('Client.builder() builds a Client for a configured provider', () => {
    const client = (Client as unknown as {
      builder(): { provider(p: 'anthropic'): { apiKey(k: string): { build(): Client } } }
    }).builder().provider('anthropic').apiKey('test').build()

    expect(client).toBeInstanceOf(Client)
  })
})

describe('client anthropic routing', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('routes provider:"anthropic" to the self-hosted AnthropicProvider', async () => {
    let capturedUrl = ''
    let capturedHeaders: Record<string, string> = {}
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, options?: RequestInit) => {
        capturedUrl = url
        capturedHeaders = (options?.headers as Record<string, string>) ?? {}
        return new Response(
          JSON.stringify({
            id: 'msg_1',
            type: 'message',
            role: 'assistant',
            content: [{ type: 'text', text: 'ok' }],
            model: 'claude-3-5-sonnet-20241022',
            stop_reason: 'end_turn',
            usage: { input_tokens: 1, output_tokens: 1 },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        )
      }),
    )

    const client = new Client({ provider: 'anthropic', apiKey: 'test-key' })
    const response = await client.chat({ messages: [{ role: 'user', content: 'hi' }] })

    // Self-hosted provider hits /v1/messages directly with x-api-key.
    expect(capturedUrl).toContain('/v1/messages')
    expect(capturedHeaders['x-api-key']).toBe('test-key')
    expect(capturedHeaders['anthropic-version']).toBe('2023-06-01')
    expect(response.content).toBe('ok')
  })
})

describe('client openai/minimax routing (no npm deps)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('routes provider:"openai" to the self-hosted OpenAIProvider (no npm openai)', async () => {
    let capturedUrl = ''
    let capturedHeaders: Record<string, string> = {}
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, options?: RequestInit) => {
        capturedUrl = url
        capturedHeaders = (options?.headers as Record<string, string>) ?? {}
        return new Response(
          JSON.stringify({
            id: 'chatcmpl_1',
            object: 'chat.completion',
            created: 1234567890,
            model: 'gpt-4o',
            choices: [{ index: 0, message: { role: 'assistant', content: 'ok' }, finish_reason: 'stop' }],
            usage: { prompt_tokens: 1, completion_tokens: 1 },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        )
      }),
    )

    const client = new Client({ provider: 'openai', apiKey: 'sk-test' })
    const response = await client.chat({ messages: [{ role: 'user', content: 'hi' }] })

    expect(capturedUrl).toContain('api.openai.com/v1/chat/completions')
    expect(capturedHeaders['authorization']).toBe('Bearer sk-test')
    expect(response.content).toBe('ok')
  })

  it('routes provider:"minimax" to the Anthropic-compatible MiniMax endpoint (no npm deps)', async () => {
    let capturedUrl = ''
    let capturedHeaders: Record<string, string> = {}
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, options?: RequestInit) => {
        capturedUrl = url
        capturedHeaders = (options?.headers as Record<string, string>) ?? {}
        return new Response(
          JSON.stringify({
            id: 'msg_1',
            type: 'message',
            role: 'assistant',
            content: [{ type: 'text', text: 'ok' }],
            model: 'MiniMax-M2.7',
            stop_reason: 'end_turn',
            usage: { input_tokens: 1, output_tokens: 1 },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        )
      }),
    )

    const client = new Client({ provider: 'minimax', apiKey: 'mk-test' })
    const response = await client.chat({ messages: [{ role: 'user', content: 'hi' }] })

    // Default MiniMax Anthropic-compat base + /v1/messages, with x-api-key auth.
    expect(capturedUrl).toBe('https://api.minimax.io/anthropic/v1/messages')
    expect(capturedHeaders['x-api-key']).toBe('mk-test')
    expect('authorization' in capturedHeaders).toBe(false)
    expect(response.content).toBe('ok')
  })
})
