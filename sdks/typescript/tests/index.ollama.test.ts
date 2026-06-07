import { afterEach, describe, expect, it, vi } from 'vitest'

describe('index.ts Ollama public exports (M5)', () => {
  it('re-exports OllamaProvider and DEFAULT_OLLAMA_MODEL from the package root', async () => {
    const mod = await import('../src/index.js')
    expect(typeof mod.OllamaProvider).toBe('function')
    expect(mod.DEFAULT_OLLAMA_MODEL).toBe('llama3.2')
  })

  it('constructs an OllamaProvider via the package root export (text-only caps)', async () => {
    const { OllamaProvider } = await import('../src/index.js')
    const provider = new OllamaProvider('llama3.2', 'http://localhost:11434')
    const caps = provider.capabilities()
    expect(caps.supportsImage).toBe(false)
    expect(caps.supportsDocument).toBe(false)
    expect((caps as { supportsMcp: boolean }).supportsMcp).toBe(false)
  })
})

describe('Client.builder().provider("ollama") done-criteria smoke', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('round-trips the NATIVE route end to end (no apiKey, /api/chat)', async () => {
    const { Client } = await import('../src/index.js')
    let url: string | undefined
    vi.stubGlobal(
      'fetch',
      vi.fn(async (u: string) => {
        url = u
        return new Response(
          JSON.stringify({ model: 'llama3.2', message: { content: 'pong' }, done: true }),
          { status: 200 },
        )
      }),
    )

    const client = Client.builder()
      .provider('ollama')
      .ollamaBaseUrl('http://localhost:11434')
      .ollamaNative(true)
      .build()

    const res = await client.chat({ messages: [{ role: 'user', content: 'ping' }] })
    expect(url).toBe('http://localhost:11434/api/chat')
    expect(res.content).toBe('pong')
    expect(res.stopReason).toBe('stop')
  })

  it('round-trips the COMPAT route end to end (no apiKey, /v1/chat/completions)', async () => {
    const { Client } = await import('../src/index.js')
    let url: string | undefined
    vi.stubGlobal(
      'fetch',
      vi.fn(async (u: string) => {
        url = u
        return new Response(
          JSON.stringify({
            model: 'llama3.2',
            choices: [{ index: 0, message: { content: 'pong' }, finish_reason: 'stop' }],
            usage: { prompt_tokens: 1, completion_tokens: 1 },
          }),
          { status: 200 },
        )
      }),
    )

    const client = Client.builder()
      .provider('ollama')
      .ollamaBaseUrl('http://localhost:11434')
      .build() // no tuning → compat

    const res = await client.chat({ messages: [{ role: 'user', content: 'ping' }] })
    expect(url).toBe('http://localhost:11434/v1/chat/completions')
    expect(res.content).toBe('pong')
  })

  it('round-trips a native STREAM through the Client (stripThink + terminal done)', async () => {
    const { Client } = await import('../src/index.js')
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(
            '{"message":{"content":"hel"},"done":false}\n{"message":{"content":"lo"},"done":false}\n{"done":true}\n',
            { status: 200 },
          ),
      ),
    )

    const client = Client.builder().provider('ollama').ollamaNative(true).build()
    let text = ''
    let sawDone = false
    for await (const e of client.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      if (e.eventType === 'text' && !e.done) text += e.content
      if (e.done) sawDone = true
    }
    expect(text).toBe('hello')
    expect(sawDone).toBe(true)
  })
})
