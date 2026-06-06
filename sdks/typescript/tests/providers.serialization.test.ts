import { afterEach, describe, expect, it, vi } from 'vitest'
import { AnthropicProvider } from '../src/providers/anthropic.js'
import { OpenAIProvider } from '../src/providers/openai.js'

describe('provider serialization', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('anthropic serializes assistant tool_use blocks', async () => {
    let capturedBody: any
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, options?: RequestInit) => {
        capturedBody = JSON.parse(String(options?.body ?? '{}'))
        return new Response(
          JSON.stringify({
            id: 'msg_1',
            type: 'message',
            role: 'assistant',
            content: [{ type: 'text', text: 'ok' }],
            model: 'claude-3-5-sonnet-20241022',
            stop_reason: 'end_turn',
            usage: { input_tokens: 1, output_tokens: 1 },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        )
      }),
    )

    const provider = new AnthropicProvider('test-key')
    await provider.chat({
      messages: [
        { role: 'user', content: 'weather?' },
        {
          role: 'assistant',
          content: 'Let me check',
          toolCalls: [{ id: 'toolu_1', name: 'get_weather', input: { city: 'Taipei' } }]
        },
        { role: 'tool', toolCallId: 'toolu_1', content: '25C' }
      ],
    })

    const assistant = capturedBody.messages.find((m: any) => m.role === 'assistant')
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
