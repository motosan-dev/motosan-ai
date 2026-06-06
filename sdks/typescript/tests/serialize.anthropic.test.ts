import { describe, it, expect } from 'vitest'
import { serializeAnthropicRequest } from '../src/serialize/anthropic.js'
import type { ContentBlock, SystemBlock } from '../src/types.js'

describe('serializeAnthropicRequest', () => {
  describe('basic structure', () => {
    it('serializes model, max_tokens, and messages', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        maxTokens: 2048,
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.model).toBe('claude-opus-4')
      expect(result.max_tokens).toBe(2048)
      expect(Array.isArray(result.messages)).toBe(true)
      expect(result.messages.length).toBe(1)
      expect(result.messages[0].role).toBe('user')
      expect(result.messages[0].content).toBe('hello')
    })

    it('includes optional fields only when present', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        model: 'claude-sonnet',
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect('system' in result).toBe(false)
      expect('tools' in result).toBe(false)
      expect('thinking' in result).toBe(false)
      expect('stop_sequences' in result).toBe(false)
      expect('temperature' in result).toBe(false)
    })
  })

  describe('messages: contentBlocks', () => {
    it('converts user contentBlocks to structured content array', () => {
      const contentBlocks: ContentBlock[] = [
        { type: 'text', text: 'Look at this:' },
        { type: 'image', source: { type: 'base64', mediaType: 'image/png', data: 'abc123' } },
      ]
      const req = {
        messages: [
          { role: 'user' as const, content: 'Look at this:', contentBlocks },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      const msg = result.messages[0]
      expect(Array.isArray(msg.content)).toBe(true)
      expect(msg.content.length).toBe(2)
      expect(msg.content[0]).toEqual({ type: 'text', text: 'Look at this:' })
      expect(msg.content[1].type).toBe('image')
      expect(msg.content[1].source.type).toBe('base64')
      expect(msg.content[1].source.media_type).toBe('image/png')
    })

    it('converts document source to snake_case media_type', () => {
      const contentBlocks: ContentBlock[] = [
        { type: 'text', text: 'PDF attached' },
        { type: 'document', source: { type: 'base64', mediaType: 'application/pdf', data: 'pdf_data_here' } },
      ]
      const req = {
        messages: [
          { role: 'user' as const, content: 'PDF attached', contentBlocks },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      const msg = result.messages[0]
      expect(msg.content[1]).toEqual({
        type: 'document',
        source: {
          type: 'base64',
          media_type: 'application/pdf',
          data: 'pdf_data_here',
        },
      })
    })

    it('applies cache_control to last block when cache=true', () => {
      const contentBlocks: ContentBlock[] = [
        { type: 'text', text: 'First' },
        { type: 'text', text: 'Second' },
      ]
      const req = {
        messages: [
          { role: 'user' as const, content: 'First\nSecond', contentBlocks, cache: true },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      const msg = result.messages[0]
      expect(msg.content[0].cache_control).toBeUndefined()
      expect(msg.content[1].cache_control).toEqual({ type: 'ephemeral' })
    })
  })

  describe('messages: plain text with cache', () => {
    it('wraps cached plain-text user message in content block array', () => {
      const req = {
        messages: [
          { role: 'user' as const, content: 'hello', cache: true },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      const msg = result.messages[0]
      expect(Array.isArray(msg.content)).toBe(true)
      expect(msg.content.length).toBe(1)
      expect(msg.content[0]).toEqual({
        type: 'text',
        text: 'hello',
        cache_control: { type: 'ephemeral' },
      })
    })

    it('keeps plain-text user message as string when cache=false', () => {
      const req = {
        messages: [
          { role: 'user' as const, content: 'hello' },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.messages[0].content).toBe('hello')
    })
  })

  describe('messages: assistant toolCalls', () => {
    it('converts assistant toolCalls to tool_use blocks', () => {
      const req = {
        messages: [
          {
            role: 'assistant' as const,
            content: 'Let me check the weather',
            toolCalls: [
              { id: 'toolu_1', name: 'get_weather', input: { city: 'Tokyo' } },
            ],
          },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      const msg = result.messages[0]
      expect(Array.isArray(msg.content)).toBe(true)
      expect(msg.content[0]).toEqual({ type: 'text', text: 'Let me check the weather' })
      expect(msg.content[1]).toEqual({
        type: 'tool_use',
        id: 'toolu_1',
        name: 'get_weather',
        input: { city: 'Tokyo' },
      })
    })

    it('applies cache_control to last tool_use block when cache=true', () => {
      const req = {
        messages: [
          {
            role: 'assistant' as const,
            content: 'checking',
            toolCalls: [
              { id: 'toolu_1', name: 'get_weather', input: { city: 'Tokyo' } },
              { id: 'toolu_2', name: 'get_time', input: {} },
            ],
            cache: true,
          },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      const msg = result.messages[0]
      expect(msg.content[0].cache_control).toBeUndefined()
      expect(msg.content[1].cache_control).toBeUndefined()
      expect(msg.content[2].cache_control).toEqual({ type: 'ephemeral' })
    })

    it('omits text block if assistant content is empty', () => {
      const req = {
        messages: [
          {
            role: 'assistant' as const,
            content: '',
            toolCalls: [
              { id: 'toolu_1', name: 'get_weather', input: { city: 'Tokyo' } },
            ],
          },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      const msg = result.messages[0]
      expect(Array.isArray(msg.content)).toBe(true)
      expect(msg.content[0].type).toBe('tool_use')
    })
  })

  describe('messages: tool role (tool_result)', () => {
    it('converts tool role message to user message with tool_result block', () => {
      const req = {
        messages: [
          { role: 'tool' as const, content: '25C', toolCallId: 'toolu_1' },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      const msg = result.messages[0]
      expect(msg.role).toBe('user')
      expect(Array.isArray(msg.content)).toBe(true)
      expect(msg.content[0]).toEqual({
        type: 'tool_result',
        tool_use_id: 'toolu_1',
        content: '25C',
      })
    })
  })

  describe('system prompt handling', () => {
    it('serializes plain system string as string when no systemBlocks or systemCache', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        system: 'You are helpful',
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.system).toBe('You are helpful')
    })

    it('wraps system string in array with cache_control when systemCache=true', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        system: 'You are helpful',
        systemCache: true,
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(Array.isArray(result.system)).toBe(true)
      expect(result.system).toEqual([
        { type: 'text', text: 'You are helpful', cache_control: { type: 'ephemeral' } },
      ])
    })

    it('serializes systemBlocks as array with per-block cache_control', () => {
      const systemBlocks: SystemBlock[] = [
        { text: 'You are a helpful assistant' },
        { text: 'Always be polite', cacheControl: true },
      ]
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        systemBlocks,
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(Array.isArray(result.system)).toBe(true)
      expect(result.system).toEqual([
        { type: 'text', text: 'You are a helpful assistant' },
        {
          type: 'text',
          text: 'Always be polite',
          cache_control: { type: 'ephemeral' },
        },
      ])
    })

    it('prioritizes systemBlocks over plain system string', () => {
      const systemBlocks: SystemBlock[] = [
        { text: 'Block system' },
      ]
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        system: 'String system',
        systemBlocks,
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.system).toEqual([{ type: 'text', text: 'Block system' }])
    })

    it('omits system field when not provided', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect('system' in result).toBe(false)
    })
  })

  describe('tools', () => {
    it('flattens tools to {name, description, input_schema}', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        tools: [
          {
            name: 'get_weather',
            description: 'Get current weather',
            inputSchema: {
              type: 'object',
              properties: { city: { type: 'string' } },
            },
          },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.tools).toEqual([
        {
          name: 'get_weather',
          description: 'Get current weather',
          input_schema: {
            type: 'object',
            properties: { city: { type: 'string' } },
          },
        },
      ])
    })

    it('includes cache_control on last tool when there are multiple tools', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        tools: [
          { name: 'tool1', description: 'First', inputSchema: {} },
          { name: 'tool2', description: 'Second', inputSchema: {}, cache: true },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.tools[0].cache_control).toBeUndefined()
      expect(result.tools[1].cache_control).toEqual({ type: 'ephemeral' })
    })

    it('omits tools field when no tools provided', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect('tools' in result).toBe(false)
    })
  })

  describe('optional fields', () => {
    it('includes thinking when present', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        thinking: { budgetTokens: 5000 },
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.thinking).toBeDefined()
    })

    it('includes stop_sequences when present', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        stopSequences: ['STOP', 'END'],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.stop_sequences).toEqual(['STOP', 'END'])
    })

    it('includes temperature when present', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        temperature: 0.8,
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.temperature).toBe(0.8)
    })
  })

  describe('providerOptions', () => {
    it('merges providerOptions into root of result', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        providerOptions: {
          custom_field: 'value',
          another: 123,
        },
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.custom_field).toBe('value')
      expect(result.another).toBe(123)
    })
  })

  describe('default maxTokens', () => {
    it('uses 8192 as default max_tokens when not provided', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.max_tokens).toBe(8192)
    })
  })
})
