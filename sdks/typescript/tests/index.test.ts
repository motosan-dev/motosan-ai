import { describe, expect, it } from 'vitest'
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
    })
    expect(mod.withImage()).toEqual({
      supportsImage: true,
      supportsDocument: false,
      supportsMcp: false,
    })
    expect(mod.fullCaps()).toEqual({
      supportsImage: true,
      supportsDocument: true,
      supportsMcp: true,
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
