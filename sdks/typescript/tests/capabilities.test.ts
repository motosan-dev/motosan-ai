import { describe, it, expect } from 'vitest'
import {
  textOnly,
  withImage,
  fullCaps,
  validateRequest,
  type ProviderCapabilities,
} from '../src/provider.js'
import { UnsupportedFeatureError } from '../src/error.js'
import type { ChatRequest } from '../src/types.js'

describe('ProviderCapabilities factories', () => {
  it('textOnly() returns {false, false}', () => {
    const caps = textOnly()
    expect(caps.supportsImage).toBe(false)
    expect(caps.supportsDocument).toBe(false)
  })

  it('withImage() returns {true, false}', () => {
    const caps = withImage()
    expect(caps.supportsImage).toBe(true)
    expect(caps.supportsDocument).toBe(false)
  })

  it('fullCaps() returns {true, true}', () => {
    const caps = fullCaps()
    expect(caps.supportsImage).toBe(true)
    expect(caps.supportsDocument).toBe(true)
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
