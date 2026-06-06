import { describe, expect, it } from 'vitest'
import { UnsupportedFeatureError } from '../src/error.js'
import { validateRequest, withImage, fullCaps } from '../src/provider.js'
import { serializeAnthropicRequest } from '../src/serialize/anthropic.js'
import type { ChatRequest } from '../src/types.js'

describe('M4 done-criteria smoke', () => {
  const req: ChatRequest = {
    messages: [{ role: 'user', content: 'use the tools' }],
    mcpServers: [{ type: 'url', url: 'https://m.example/sse', name: 'm' }],
    mcpToolConfigs: [{ kind: 'all', mcpServerName: 'm' }],
    thinking: { budgetTokens: 2048 },
  }

  it('serializes MCP + thinking for anthropic (non-adaptive model)', () => {
    const body = serializeAnthropicRequest(req, 'claude-sonnet-4-6')
    // MCP servers on the body.
    expect(body.mcp_servers).toEqual([
      { type: 'url', url: 'https://m.example/sse', name: 'm' },
    ])
    // mcp_toolset folded into the tools array.
    expect(body.tools).toEqual([{ type: 'mcp_toolset', mcp_server_name: 'm' }])
    // Non-adaptive thinking → enabled shape + forced temperature.
    expect(body.thinking).toEqual({
      type: 'enabled',
      budget_tokens: 2048,
      display: 'summarized',
    })
    expect(body.temperature).toBe(1.0)
  })

  it('serializes MCP + thinking for anthropic (adaptive model)', () => {
    const body = serializeAnthropicRequest(req, 'claude-opus-4-8')
    expect(body.thinking).toEqual({ type: 'adaptive', display: 'summarized' })
    expect(body.output_config).toEqual({ effort: 'high' })
    expect('temperature' in body).toBe(false)
  })

  it('passes validateRequest for an MCP-capable provider (anthropic/fullCaps)', () => {
    expect(() => validateRequest(req, fullCaps())).not.toThrow()
  })

  it('rejects MCP for a non-MCP provider (openai/withImage) before any HTTP', () => {
    expect(() => validateRequest(req, withImage())).toThrow(UnsupportedFeatureError)
    expect(() => validateRequest(req, withImage())).toThrow(
      'provider does not support MCP server config',
    )
  })

  it('exposes the MCP types and caps helpers from the package entrypoint', async () => {
    const mod = await import('../src/index.js')
    expect(typeof mod.validateRequest).toBe('function')
    expect(typeof mod.minimaxCaps).toBe('function')
    // McpServerConfig is a type (erased at runtime) — assert usage compiles by
    // constructing a value typed as it via the imported module's type surface.
    const cfg: import('../src/index.js').McpServerConfig = {
      type: 'url',
      url: 'https://x/sse',
      name: 'x',
    }
    expect(cfg.name).toBe('x')
  })
})
