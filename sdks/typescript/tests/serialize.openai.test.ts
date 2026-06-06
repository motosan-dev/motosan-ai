import { describe, it, expect } from 'vitest'
import { serializeOpenAiRequest } from '../src/serialize/openai.js'
import type { ContentBlock, SystemBlock } from '../src/types.js'

describe('serializeOpenAiRequest', () => {
  describe('basic structure', () => {
    it('emits model + messages; max_tokens only when set', () => {
      const r = serializeOpenAiRequest(
        { messages: [{ role: 'user', content: 'hi' }] },
        'gpt-4o',
      )
      expect(r.model).toBe('gpt-4o')
      expect(Array.isArray(r.messages)).toBe(true)
      expect('max_tokens' in r).toBe(false) // no SDK-side default, unlike Anthropic
    })

    it('emits max_tokens, temperature, stop when present', () => {
      const r = serializeOpenAiRequest(
        {
          messages: [{ role: 'user', content: 'hi' }],
          maxTokens: 256,
          temperature: 0.7,
          stopSequences: ['STOP'],
        },
        'gpt-4o',
      )
      expect(r.max_tokens).toBe(256)
      expect(r.temperature).toBe(0.7)
      expect(r.stop).toEqual(['STOP']) // 'stop', not 'stop_sequences'
    })
  })

  describe('system prompt as a message (NOT top-level)', () => {
    it('prepends system string as a role:system message', () => {
      const r = serializeOpenAiRequest(
        { messages: [{ role: 'user', content: 'hi' }], system: 'be terse' },
        'gpt-4o',
      )
      expect('system' in r).toBe(false) // divergence from Anthropic
      const msgs = r.messages as any[]
      expect(msgs[0]).toEqual({ role: 'system', content: 'be terse' })
      expect(msgs[1]).toEqual({ role: 'user', content: 'hi' })
    })

    it('joins systemBlocks with newlines into one system message (no per-block cache)', () => {
      const systemBlocks: SystemBlock[] = [
        { text: 'line one' },
        { text: 'line two', cacheControl: true },
      ]
      const r = serializeOpenAiRequest(
        { messages: [{ role: 'user', content: 'hi' }], systemBlocks },
        'gpt-4o',
      )
      const msgs = r.messages as any[]
      expect(msgs[0]).toEqual({ role: 'system', content: 'line one\nline two' })
      expect(JSON.stringify(msgs[0])).not.toContain('cache_control')
    })

    it('prioritizes systemBlocks over system string', () => {
      const r = serializeOpenAiRequest(
        {
          messages: [{ role: 'user', content: 'hi' }],
          system: 'ignored',
          systemBlocks: [{ text: 'used' }],
        },
        'gpt-4o',
      )
      expect((r.messages as any[])[0].content).toBe('used')
    })
  })

  describe('tools (flat function schema)', () => {
    it('wraps tools as {type:function, function:{name,description,parameters}}', () => {
      const r = serializeOpenAiRequest(
        {
          messages: [{ role: 'user', content: 'hi' }],
          tools: [
            { name: 'get_weather', description: 'w', inputSchema: { type: 'object', properties: { city: { type: 'string' } } } },
          ],
        },
        'gpt-4o',
      )
      expect(r.tools).toEqual([
        {
          type: 'function',
          function: {
            name: 'get_weather',
            description: 'w',
            parameters: { type: 'object', properties: { city: { type: 'string' } } },
          },
        },
      ])
      // Divergence: NOT Anthropic's {name, description, input_schema}.
      expect(JSON.stringify(r.tools)).not.toContain('input_schema')
    })

    it('defaults missing description/parameters', () => {
      const r = serializeOpenAiRequest(
        { messages: [{ role: 'user', content: 'hi' }], tools: [{ name: 'noop' }] },
        'gpt-4o',
      )
      const fn = (r.tools as any[])[0].function
      expect(fn.description).toBe('')
      expect(fn.parameters).toEqual({ type: 'object', properties: {} })
    })

    it('omits tools when none provided', () => {
      const r = serializeOpenAiRequest({ messages: [{ role: 'user', content: 'hi' }] }, 'gpt-4o')
      expect('tools' in r).toBe(false)
    })
  })

  describe('tool_choice (OpenAI string/function forms)', () => {
    const base = {
      messages: [{ role: 'user' as const, content: 'hi' }],
      tools: [{ name: 'get_weather', description: 'w', inputSchema: { type: 'object' } }],
    }
    it('auto -> "auto" (string, not object)', () => {
      const r = serializeOpenAiRequest({ ...base, toolChoice: { type: 'auto' } }, 'gpt-4o')
      expect(r.tool_choice).toBe('auto')
    })
    it('required -> "required" (NOT Anthropic any)', () => {
      const r = serializeOpenAiRequest({ ...base, toolChoice: { type: 'required' } }, 'gpt-4o')
      expect(r.tool_choice).toBe('required')
    })
    it('none -> "none" string; tools UNTOUCHED (unlike Anthropic which removes them)', () => {
      const r = serializeOpenAiRequest({ ...base, toolChoice: { type: 'none' } }, 'gpt-4o')
      expect(r.tool_choice).toBe('none')
      expect(r.tools).toBeDefined()
    })
    it('tool -> {type:function, function:{name}}', () => {
      const r = serializeOpenAiRequest(
        { ...base, toolChoice: { type: 'tool', name: 'get_weather' } },
        'gpt-4o',
      )
      expect(r.tool_choice).toEqual({ type: 'function', function: { name: 'get_weather' } })
    })
  })

  describe('assistant tool_calls (stringified arguments)', () => {
    it('serializes tool_calls with function.arguments as a JSON STRING', () => {
      const r = serializeOpenAiRequest(
        {
          messages: [
            {
              role: 'assistant',
              content: 'checking',
              toolCalls: [{ id: 'call_1', name: 'get_weather', input: { city: 'Taipei' } }],
            },
          ],
        },
        'gpt-4o',
      )
      const a = (r.messages as any[])[0]
      expect(a.tool_calls[0]).toEqual({
        id: 'call_1',
        type: 'function',
        function: { name: 'get_weather', arguments: '{"city":"Taipei"}' },
      })
      expect(typeof a.tool_calls[0].function.arguments).toBe('string') // NOT an object
    })
  })

  describe('tool role -> role:tool message', () => {
    it('maps tool message to {role:tool, tool_call_id, content}', () => {
      const r = serializeOpenAiRequest(
        { messages: [{ role: 'tool', content: '25C', toolCallId: 'call_1' }] },
        'gpt-4o',
      )
      expect((r.messages as any[])[0]).toEqual({
        role: 'tool',
        tool_call_id: 'call_1',
        content: '25C',
      })
    })

    it('drops a tool message lacking tool_call_id', () => {
      const r = serializeOpenAiRequest(
        { messages: [{ role: 'tool', content: 'orphan' }] },
        'gpt-4o',
      )
      expect((r.messages as any[]).length).toBe(0)
    })
  })

  describe('user content blocks -> image_url', () => {
    it('serializes base64 image as data URL image_url', () => {
      const contentBlocks: ContentBlock[] = [
        { type: 'text', text: 'look' },
        { type: 'image', source: { type: 'base64', mediaType: 'image/png', data: 'abc' } },
      ]
      const r = serializeOpenAiRequest(
        { messages: [{ role: 'user', content: 'look', contentBlocks }] },
        'gpt-4o',
      )
      const content = (r.messages as any[])[0].content
      expect(content[0]).toEqual({ type: 'text', text: 'look' })
      expect(content[1]).toEqual({
        type: 'image_url',
        image_url: { url: 'data:image/png;base64,abc' },
      })
    })

    it('serializes url image as image_url passthrough', () => {
      const contentBlocks: ContentBlock[] = [
        { type: 'image', source: { type: 'url', url: 'https://x/y.png' } },
      ]
      const r = serializeOpenAiRequest(
        { messages: [{ role: 'user', content: '', contentBlocks }] },
        'gpt-4o',
      )
      expect((r.messages as any[])[0].content[0]).toEqual({
        type: 'image_url',
        image_url: { url: 'https://x/y.png' },
      })
    })

    it('throws on a document content block (OpenAI-unsupported)', () => {
      const contentBlocks: ContentBlock[] = [
        { type: 'document', source: { type: 'base64', mediaType: 'application/pdf', data: 'd' } },
      ]
      expect(() =>
        serializeOpenAiRequest(
          { messages: [{ role: 'user', content: '', contentBlocks }] },
          'gpt-4o',
        ),
      ).toThrow()
    })
  })

  describe('providerOptions', () => {
    it('merges providerOptions into the request root', () => {
      const r = serializeOpenAiRequest(
        {
          messages: [{ role: 'user', content: 'hi' }],
          providerOptions: { stream_options: { include_usage: true } },
        },
        'gpt-4o',
      )
      expect(r.stream_options).toEqual({ include_usage: true })
    })
  })
})
