import { describe, expect, it } from 'vitest'
import { Client } from '../src/client.js'

;(process.env.OPENAI_API_KEY ? describe : describe.skip)('openai integration', () => {
  it('chat works with real API key', async () => {
    const client = new Client({ provider: 'openai', apiKey: process.env.OPENAI_API_KEY, model: 'gpt-4o-mini' })
    const response = await client.chat({ messages: [{ role: 'user', content: 'reply with OK' }], maxTokens: 32 })
    expect(response.content.length).toBeGreaterThan(0)
  }, 60_000)
})

;(process.env.MINIMAX_API_KEY ? describe : describe.skip)('minimax integration', () => {
  it('chat works with real API key', async () => {
    const client = new Client({ provider: 'minimax', apiKey: process.env.MINIMAX_API_KEY, model: 'MiniMax-Text-01' })
    const response = await client.chat({ messages: [{ role: 'user', content: 'reply with OK' }], maxTokens: 32 })
    expect(response.content.length).toBeGreaterThan(0)
  }, 60_000)
})
