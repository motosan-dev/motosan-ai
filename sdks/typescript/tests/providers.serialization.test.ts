import { describe, expect, it } from 'vitest'
import { AnthropicProvider } from '../src/providers/anthropic.js'
import { OpenAIProvider } from '../src/providers/openai.js'

describe('provider serialization', () => {
  it('anthropic serializes assistant tool_use blocks', () => {
    const serialized = AnthropicProvider.serializeMessages([
      { role: 'user', content: 'weather?' },
      {
        role: 'assistant',
        content: 'Let me check',
        toolCalls: [{ id: 'toolu_1', name: 'get_weather', input: { city: 'Taipei' } }]
      },
      { role: 'tool', toolCallId: 'toolu_1', content: '25C' }
    ])

    const assistant = serialized.messages.find((m) => m.role === 'assistant')
    expect(Array.isArray(assistant.content)).toBe(true)
    expect(assistant.content[1].type).toBe('tool_use')
  })

  it('openai serializes assistant tool_calls', () => {
    const serialized = OpenAIProvider.serializeMessages([
      {
        role: 'assistant',
        content: 'Let me check',
        toolCalls: [{ id: 'call_1', name: 'get_weather', input: { city: 'Taipei' } }]
      },
      { role: 'tool', toolCallId: 'call_1', content: '25C' }
    ])

    const assistant = serialized[0]
    expect(Array.isArray(assistant.tool_calls)).toBe(true)
    expect(assistant.tool_calls[0].function.arguments).toContain('Taipei')
    expect(serialized[1].tool_call_id).toBe('call_1')
  })
})
