import { describe, expect, it } from 'vitest'
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
})
