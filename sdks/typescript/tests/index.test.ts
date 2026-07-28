import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  Client,
  GeminiProvider,
  DEFAULT_GEMINI_MODEL,
  GEMINI_MODELS,
  serializeGeminiRequest,
} from '../src/index.js'
import type { Provider, ProviderCapabilities } from '../src/index.js'

describe('index.ts public exports', () => {
  it('re-exports M3 public symbols from the package entrypoint', async () => {
    const mod = await import('../src/index.js')

    expect(typeof mod.RetryPolicy).toBe('function')

    expect(typeof mod.textOnly).toBe('function')
    expect(typeof mod.withImage).toBe('function')
    expect(typeof mod.fullCaps).toBe('function')
    const caps: ProviderCapabilities = mod.textOnly()
    expect(caps).toEqual({
      supportsImage: false,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: false,
    })
    expect(mod.withImage()).toEqual({
      supportsImage: true,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: false,
    })
    expect(mod.fullCaps()).toEqual({
      supportsImage: true,
      supportsDocument: true,
      supportsMcp: true,
      supportsFreeformTools: false,
    })

    expect(typeof mod.DEFAULT_ANTHROPIC_MODEL).toBe('string')
    expect(typeof mod.DEFAULT_OPENAI_MODEL).toBe('string')
    expect(typeof mod.DEFAULT_MINIMAX_MODEL).toBe('string')
    expect(Array.isArray(mod.ANTHROPIC_MODELS)).toBe(true)
    expect(Array.isArray(mod.OPENAI_MODELS)).toBe(true)
    expect(Array.isArray(mod.MINIMAX_MODELS)).toBe(true)

    expect(typeof mod.ThinkStripper).toBe('function')
    expect(typeof mod.stripThink).toBe('function')
    expect(typeof mod.ClientBuilder).toBe('function')

    const provider: Provider = 'anthropic'
    expect(provider).toBe('anthropic')
  })

  it('keeps existing public symbols available from the package entrypoint', async () => {
    const mod = await import('../src/index.js')

    expect(typeof mod.Client).toBe('function')
    expect(typeof mod.ConfigError).toBe('function')
    expect(typeof mod.MotosanError).toBe('function')
    expect(typeof mod.collectStream).toBe('function')
    expect(typeof mod.validateRequest).toBe('function')
  })

  it('round-trips Client.builder().provider(...).apiKey(...).build()', async () => {
    const { Client } = await import('../src/index.js')
    const builderFactory = Client as unknown as {
      builder(): { provider(p: 'anthropic'): { apiKey(k: string): { build(): unknown } } }
    }

    const client = builderFactory.builder().provider('anthropic').apiKey('test').build()
    expect(client).toBeInstanceOf(Client)
  })
})

describe('M6 Gemini public surface (index re-exports)', () => {
  it('re-exports DEFAULT_GEMINI_MODEL and GEMINI_MODELS from the root', () => {
    expect(DEFAULT_GEMINI_MODEL).toBe('gemini-2.5-flash')
    expect(GEMINI_MODELS).toContain('gemini-2.5-flash')
    expect(GEMINI_MODELS.length).toBe(8)
  })

  it('re-exports the GeminiProvider class from the root', () => {
    const provider = new GeminiProvider('test-key')
    const caps = provider.capabilities()
    expect(caps.supportsImage).toBe(true)
    expect(caps.supportsDocument).toBe(false)
    expect(caps.supportsMcp).toBe(false)
  })

  it('re-exports serializeGeminiRequest from the root', () => {
    const body = serializeGeminiRequest(
      { messages: [{ role: 'user', content: 'Hello' }] },
      DEFAULT_GEMINI_MODEL,
    )
    const contents = (body as any).contents
    expect(contents[0].role).toBe('user')
    expect(contents[0].parts[0].text).toBe('Hello')
    expect((body as any).model).toBeUndefined()
  })
})

describe('ChatGPT-Codex public surface (index re-exports)', () => {
  it('exports ChatGptCodexProvider and its default model', async () => {
    const sdk = await import('../src/index.js')
    expect(typeof sdk.ChatGptCodexProvider).toBe('function')
    expect(sdk.DEFAULT_CHATGPT_CODEX_MODEL).toBe('gpt-5.5')
    expect(sdk.CHATGPT_CODEX_MODELS).toContain('gpt-5.5')
  })
})

describe('M6 Gemini done-criteria smoke (builder round-trip)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    delete process.env.GEMINI_API_KEY
  })

  it('Client.builder().provider("gemini").apiKey(...).build() returns a Client', () => {
    const client = Client.builder().provider('gemini').apiKey('smoke-key').build()
    expect(client).toBeInstanceOf(Client)
  })

  it('a built gemini Client performs a chat through a mocked fetch (end-to-end)', async () => {
    const mockFetch = vi.fn(async (url: string) => {
      expect(url).toContain('/models/gemini-2.5-flash:generateContent')
      return new Response(
        JSON.stringify({
          candidates: [
            { content: { parts: [{ text: 'pong' }], role: 'model' }, finishReason: 'STOP' },
          ],
          usageMetadata: { promptTokenCount: 1, candidatesTokenCount: 1 },
          modelVersion: 'gemini-2.5-flash',
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      )
    })
    vi.stubGlobal('fetch', mockFetch)

    const client = Client.builder().provider('gemini').apiKey('smoke-key').build()
    const resp = await client.chat({ messages: [{ role: 'user', content: 'ping' }] })
    expect(resp.content).toBe('pong')
    expect(resp.stopReason).toBe('end_turn')
    expect(mockFetch).toHaveBeenCalledTimes(1)
  })
})
