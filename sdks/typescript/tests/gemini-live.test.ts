import { describe, it, expect } from 'vitest'
import { GeminiProvider } from '../src/providers/gemini.js'

const KEY = process.env.GEMINI_API_KEY
const live = KEY ? describe : describe.skip

live('GeminiProvider (live)', () => {
  it('completes a simple chat', async () => {
    const provider = new GeminiProvider(KEY as string)
    const resp = await provider.chat({
      messages: [{ role: 'user', content: 'Reply with the single word: ok' }],
      // gemini-2.5-flash does dynamic "thinking" by default; a tiny budget can
      // be consumed entirely by thinking (finishReason MAX_TOKENS, no text part),
      // so give enough room for thinking + the text reply.
      maxTokens: 256,
    })
    expect(resp.content.length).toBeGreaterThan(0)
    expect(['end_turn', 'max_tokens', 'other']).toContain(resp.stopReason)
  }, 30_000)
})
