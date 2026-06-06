import { describe, it, expect } from 'vitest'
import type { ContentBlock } from '../src/types.js'
import {
  user,
  userWithCache,
  assistant,
  assistantWithToolCalls,
  system,
  tool,
  toolResult,
  userWithImage,
  userWithBlocks,
  userWithPdfBase64,
  userWithPdfUrl,
  userWithPdfBytes,
  withCache,
} from '../src/message.js'

describe('message factories', () => {
  describe('user', () => {
    it('creates a user message with content', () => {
      const msg = user('hello')
      expect(msg.role).toBe('user')
      expect(msg.content).toBe('hello')
      expect('cache' in msg).toBe(false)
      expect('contentBlocks' in msg).toBe(false)
    })
  })

  describe('userWithCache', () => {
    it('creates a user message marked for caching', () => {
      const msg = userWithCache('hello')
      expect(msg.role).toBe('user')
      expect(msg.content).toBe('hello')
      expect(msg.cache).toBe(true)
      expect('contentBlocks' in msg).toBe(false)
    })
  })

  describe('assistant', () => {
    it('creates an assistant message', () => {
      const msg = assistant('response')
      expect(msg.role).toBe('assistant')
      expect(msg.content).toBe('response')
      expect('cache' in msg).toBe(false)
    })
  })

  describe('assistantWithToolCalls', () => {
    it('creates an assistant message with tool calls', () => {
      const toolCalls = [{ id: 'call_1', name: 'get_weather', input: { city: 'NYC' } }]
      const msg = assistantWithToolCalls('calling tool', toolCalls)
      expect(msg.role).toBe('assistant')
      expect(msg.content).toBe('calling tool')
      expect(msg.toolCalls).toEqual(toolCalls)
      expect('cache' in msg).toBe(false)
    })
  })

  describe('system', () => {
    it('creates a system message', () => {
      const msg = system('be helpful')
      expect(msg.role).toBe('system')
      expect(msg.content).toBe('be helpful')
      expect('cache' in msg).toBe(false)
    })
  })

  describe('tool', () => {
    it('creates a tool message (alias of toolResult)', () => {
      const msg = tool('result text', 'call_123')
      expect(msg.role).toBe('tool')
      expect(msg.content).toBe('result text')
      expect(msg.toolCallId).toBe('call_123')
      expect('cache' in msg).toBe(false)
    })
  })

  describe('toolResult', () => {
    it('creates a tool message with tool call id', () => {
      const msg = toolResult('call_456', 'some result')
      expect(msg.role).toBe('tool')
      expect(msg.content).toBe('some result')
      expect(msg.toolCallId).toBe('call_456')
      expect('toolCalls' in msg).toBe(false)
      expect('cache' in msg).toBe(false)
    })
  })

  describe('userWithImage', () => {
    it('creates a user message with text and image blocks', () => {
      const base64Data =
        'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=='
      const msg = userWithImage('look at this', base64Data, 'image/png')

      expect(msg.role).toBe('user')
      expect(msg.content).toBe('look at this')
      expect(msg.contentBlocks).toHaveLength(2)
      expect(msg.contentBlocks![0]).toEqual({ type: 'text', text: 'look at this' })
      expect(msg.contentBlocks![1]).toEqual({
        type: 'image',
        source: { type: 'base64', mediaType: 'image/png', data: base64Data },
      })
      expect('cache' in msg).toBe(false)
    })
  })

  describe('userWithBlocks', () => {
    it('extracts first text block as content and stores all blocks', () => {
      const blocks: ContentBlock[] = [
        { type: 'text', text: 'first text' },
        { type: 'text', text: 'second text' },
        { type: 'image', source: { type: 'url', url: 'https://example.com/img.png' } },
      ]
      const msg = userWithBlocks(blocks)

      expect(msg.role).toBe('user')
      expect(msg.content).toBe('first text')
      expect(msg.contentBlocks).toEqual(blocks)
      expect('cache' in msg).toBe(false)
    })

    it('handles empty blocks array', () => {
      const msg = userWithBlocks([])
      expect(msg.role).toBe('user')
      expect(msg.content).toBe('')
      expect(msg.contentBlocks).toEqual([])
    })

    it('extracts text from first text block when not first block', () => {
      const blocks: ContentBlock[] = [
        { type: 'image', source: { type: 'url', url: 'https://example.com/img.png' } },
        { type: 'text', text: 'found text' },
      ]
      const msg = userWithBlocks(blocks)

      expect(msg.content).toBe('found text')
      expect(msg.contentBlocks).toEqual(blocks)
    })
  })

  describe('userWithPdfBase64', () => {
    it('creates a user message with text and PDF document blocks', () => {
      const pdfBase64 = 'JVBERi0xLjQK'
      const msg = userWithPdfBase64('here is a pdf', pdfBase64)

      expect(msg.role).toBe('user')
      expect(msg.content).toBe('here is a pdf')
      expect(msg.contentBlocks).toHaveLength(2)
      expect(msg.contentBlocks![0]).toEqual({ type: 'text', text: 'here is a pdf' })
      expect(msg.contentBlocks![1]).toEqual({
        type: 'document',
        source: { type: 'base64', mediaType: 'application/pdf', data: pdfBase64 },
      })
      expect('cache' in msg).toBe(false)
    })
  })

  describe('userWithPdfUrl', () => {
    it('creates a user message with text and PDF document from URL', () => {
      const url = 'https://example.com/document.pdf'
      const msg = userWithPdfUrl('check this pdf', url)

      expect(msg.role).toBe('user')
      expect(msg.content).toBe('check this pdf')
      expect(msg.contentBlocks).toHaveLength(2)
      expect(msg.contentBlocks![0]).toEqual({ type: 'text', text: 'check this pdf' })
      expect(msg.contentBlocks![1]).toEqual({
        type: 'document',
        source: { type: 'url', url },
      })
      expect('cache' in msg).toBe(false)
    })
  })

  describe('userWithPdfBytes', () => {
    it('base64-encodes bytes and creates PDF document message', () => {
      const bytes = new Uint8Array([0x25, 0x50, 0x44, 0x46]) // "%PDF"
      const msg = userWithPdfBytes('pdf from bytes', bytes)

      expect(msg.role).toBe('user')
      expect(msg.content).toBe('pdf from bytes')
      expect(msg.contentBlocks).toHaveLength(2)
      expect(msg.contentBlocks![0]).toEqual({ type: 'text', text: 'pdf from bytes' })
      expect(msg.contentBlocks![1].type).toBe('document')
      if (msg.contentBlocks![1].type === 'document') {
        const docBlock = msg.contentBlocks![1]
        expect(docBlock.source.type).toBe('base64')
        expect(docBlock.source.mediaType).toBe('application/pdf')
        if (docBlock.source.type === 'base64') {
          expect(docBlock.source.data).toBe('JVBERg==') // base64 of "%PDF"
        }
      }
      expect('cache' in msg).toBe(false)
    })
  })

  describe('withCache', () => {
    it('returns a copy of the message with cache flag set to true', () => {
      const original = user('hello')
      const cached = withCache(original)

      expect(cached.role).toBe('user')
      expect(cached.content).toBe('hello')
      expect(cached.cache).toBe(true)
      expect('cache' in original).toBe(false)
    })

    it('preserves contentBlocks when setting cache', () => {
      const blocks: ContentBlock[] = [
        { type: 'text', text: 'text' },
        { type: 'image', source: { type: 'url', url: 'https://example.com/img.png' } },
      ]
      const original = userWithBlocks(blocks)
      const cached = withCache(original)

      expect(cached.cache).toBe(true)
      expect(cached.contentBlocks).toEqual(blocks)
    })

    it('preserves toolCalls when setting cache', () => {
      const toolCalls = [{ id: 'tc1', name: 'func', input: {} }]
      const original = assistantWithToolCalls('resp', toolCalls)
      const cached = withCache(original)

      expect(cached.cache).toBe(true)
      expect(cached.toolCalls).toEqual(toolCalls)
    })

    it('preserves toolCallId when setting cache', () => {
      const original = toolResult('call_1', 'result')
      const cached = withCache(original)

      expect(cached.cache).toBe(true)
      expect(cached.toolCallId).toBe('call_1')
      expect(cached.role).toBe('tool')
    })
  })
})
