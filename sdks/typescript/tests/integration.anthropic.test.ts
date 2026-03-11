import { describe, expect, it } from 'vitest'
import { Client } from '../src/client.js'

const run = !!process.env.ANTHROPIC_API_KEY

;(run ? describe : describe.skip)('anthropic integration', () => {
  it('chat works with real API key', async () => {
    const client = new Client({ provider: 'anthropic', apiKey: process.env.ANTHROPIC_API_KEY, model: 'claude-sonnet-4-5' })
    const response = await client.chat({ messages: [{ role: 'user', content: 'reply with OK' }], maxTokens: 32 })
    expect(response.content.length).toBeGreaterThan(0)
  }, 60_000)
})
