import { describe, it, expect } from 'vitest'
import {
  textOnly,
  withImage,
  fullCaps,
  minimaxCaps,
  validateRequest,
  type Provider,
  type ProviderCapabilities,
} from '../src/provider.js'
import { UnsupportedFeatureError } from '../src/error.js'
import type { ChatRequest } from '../src/types.js'

describe('ProviderCapabilities factories', () => {
  it('textOnly() returns {false, false, supportsMcp:false}', () => {
    expect(textOnly()).toEqual({
      supportsImage: false,
      supportsDocument: false,
      supportsMcp: false,
    })
  })

  it('withImage() returns {true, false, supportsMcp:false}', () => {
    expect(withImage()).toEqual({
      supportsImage: true,
      supportsDocument: false,
      supportsMcp: false,
    })
  })

  it('fullCaps() returns {true, true, supportsMcp:true}', () => {
    expect(fullCaps()).toEqual({
      supportsImage: true,
      supportsDocument: true,
      supportsMcp: true,
    })
  })

  it('minimaxCaps() returns text-only but MCP-capable', () => {
    expect(minimaxCaps()).toEqual({
      supportsImage: false,
      supportsDocument: false,
      supportsMcp: true,
    })
  })
})

describe('validateRequest', () => {
  const textOnlyCaps = textOnly()
  const withImageCaps = withImage()
  const fullCaps_ = fullCaps()

  describe('text-only provider', () => {
    it('passes a request with only text blocks', () => {
      const req: ChatRequest = {
        messages: [
          {
            role: 'user',
            content: 'Hello',
            contentBlocks: [{ type: 'text', text: 'Hello' }],
          },
        ],
      }
      expect(() => validateRequest(req, textOnlyCaps)).not.toThrow()
    })

    it('throws UnsupportedFeatureError for image blocks', () => {
      const req: ChatRequest = {
        messages: [
          {
            role: 'user',
            content: '',
            contentBlocks: [
              {
                type: 'image',
                source: { type: 'url', url: 'https://example.com/image.jpg' },
              },
            ],
          },
        ],
      }
      expect(() => validateRequest(req, textOnlyCaps)).toThrow(
        UnsupportedFeatureError
      )
      expect(() => validateRequest(req, textOnlyCaps)).toThrow(
        'provider does not support image input'
      )
    })

    it('throws UnsupportedFeatureError for document blocks', () => {
      const req: ChatRequest = {
        messages: [
          {
            role: 'user',
            content: '',
            contentBlocks: [
              {
                type: 'document',
                source: { type: 'url', url: 'https://example.com/doc.pdf' },
              },
            ],
          },
        ],
      }
      expect(() => validateRequest(req, textOnlyCaps)).toThrow(
        UnsupportedFeatureError
      )
      expect(() => validateRequest(req, textOnlyCaps)).toThrow(
        'provider does not support document input'
      )
    })

    it('throws on first unsupported block in a multi-block message', () => {
      const req: ChatRequest = {
        messages: [
          {
            role: 'user',
            content: '',
            contentBlocks: [
              { type: 'text', text: 'Check this image:' },
              {
                type: 'image',
                source: { type: 'url', url: 'https://example.com/image.jpg' },
              },
            ],
          },
        ],
      }
      expect(() => validateRequest(req, textOnlyCaps)).toThrow(
        UnsupportedFeatureError
      )
    })
  })

  describe('with-image provider', () => {
    it('passes a request with text and image blocks', () => {
      const req: ChatRequest = {
        messages: [
          {
            role: 'user',
            content: '',
            contentBlocks: [
              { type: 'text', text: 'Check this image:' },
              {
                type: 'image',
                source: { type: 'url', url: 'https://example.com/image.jpg' },
              },
            ],
          },
        ],
      }
      expect(() => validateRequest(req, withImageCaps)).not.toThrow()
    })

    it('throws UnsupportedFeatureError for document blocks', () => {
      const req: ChatRequest = {
        messages: [
          {
            role: 'user',
            content: '',
            contentBlocks: [
              {
                type: 'document',
                source: { type: 'url', url: 'https://example.com/doc.pdf' },
              },
            ],
          },
        ],
      }
      expect(() => validateRequest(req, withImageCaps)).toThrow(
        UnsupportedFeatureError
      )
      expect(() => validateRequest(req, withImageCaps)).toThrow(
        'provider does not support document input'
      )
    })
  })

  describe('full-capability provider', () => {
    it('passes a request with text, image, and document blocks', () => {
      const req: ChatRequest = {
        messages: [
          {
            role: 'user',
            content: '',
            contentBlocks: [
              { type: 'text', text: 'Here are some files:' },
              {
                type: 'image',
                source: { type: 'url', url: 'https://example.com/image.jpg' },
              },
              {
                type: 'document',
                source: { type: 'url', url: 'https://example.com/doc.pdf' },
              },
            ],
          },
        ],
      }
      expect(() => validateRequest(req, fullCaps_)).not.toThrow()
    })
  })

  describe('edge cases', () => {
    it('passes a request with no contentBlocks (uses content field only)', () => {
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'Hello' }],
      }
      expect(() => validateRequest(req, textOnlyCaps)).not.toThrow()
    })

    it('passes a request with empty contentBlocks', () => {
      const req: ChatRequest = {
        messages: [
          {
            role: 'user',
            content: 'Hello',
            contentBlocks: [],
          },
        ],
      }
      expect(() => validateRequest(req, textOnlyCaps)).not.toThrow()
    })

    it('validates all messages in a multi-message request', () => {
      const req: ChatRequest = {
        messages: [
          { role: 'user', content: 'First message' },
          {
            role: 'assistant',
            content: 'Response',
            contentBlocks: [
              {
                type: 'image',
                source: { type: 'url', url: 'https://example.com/image.jpg' },
              },
            ],
          },
        ],
      }
      expect(() => validateRequest(req, textOnlyCaps)).toThrow(
        UnsupportedFeatureError
      )
    })
  })
})

describe('Provider capabilities integration', () => {
  it('can be composed into a config object', () => {
    const config: { capabilities: ProviderCapabilities } = {
      capabilities: withImage(),
    }
    expect(config.capabilities.supportsImage).toBe(true)
  })
})

describe('validateRequest MCP gating (M4)', () => {
  const mcpReq: ChatRequest = {
    messages: [{ role: 'user', content: 'hi' }],
    mcpServers: [{ type: 'url', url: 'https://m.example/sse', name: 'm' }],
  }
  const mcpToolReq: ChatRequest = {
    messages: [{ role: 'user', content: 'hi' }],
    mcpToolConfigs: [{ kind: 'all', mcpServerName: 'm' }],
  }

  it('throws UnsupportedFeatureError when mcpServers set and caps lack MCP (openai/withImage)', () => {
    expect(() => validateRequest(mcpReq, withImage())).toThrow(UnsupportedFeatureError)
    expect(() => validateRequest(mcpReq, withImage())).toThrow(
      'provider does not support MCP server config',
    )
  })

  it('throws when mcpToolConfigs set and caps lack MCP (textOnly)', () => {
    expect(() => validateRequest(mcpToolReq, textOnly())).toThrow(UnsupportedFeatureError)
  })

  it('passes MCP config for MCP-capable caps (fullCaps / anthropic)', () => {
    expect(() => validateRequest(mcpReq, fullCaps())).not.toThrow()
    expect(() => validateRequest(mcpToolReq, fullCaps())).not.toThrow()
  })

  it('passes MCP config for minimaxCaps (text-only but MCP-capable)', () => {
    expect(() => validateRequest(mcpReq, minimaxCaps())).not.toThrow()
  })

  it('does NOT throw when no MCP config is present (textOnly)', () => {
    const plain: ChatRequest = { messages: [{ role: 'user', content: 'hi' }] }
    expect(() => validateRequest(plain, textOnly())).not.toThrow()
  })
})

describe('Provider union includes ollama (M5)', () => {
  it('accepts "ollama" as a Provider value', () => {
    // Type-level: this assignment only compiles once 'ollama' is in the union.
    const p: Provider = 'ollama'
    expect(p).toBe('ollama')
  })

  it('still accepts the M1-M4 provider tokens', () => {
    const all: Provider[] = ['anthropic', 'openai', 'minimax', 'ollama']
    expect(all).toContain('ollama')
    expect(all).toHaveLength(4)
  })
})
