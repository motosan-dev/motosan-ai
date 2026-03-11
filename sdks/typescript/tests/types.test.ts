import { describe, expect, it } from 'vitest'
import { MessageFactory, StopReason, type ToolCall } from '../src/types.js'

describe('types constructors', () => {
  it('creates message helpers', () => {
    const tc: ToolCall = { id: 'call_1', name: 'get_weather', input: { city: 'Taipei' } }
    const user = MessageFactory.user('hi')
    const assistant = MessageFactory.assistant('hello')
    const system = MessageFactory.system('rules')
    const tool = MessageFactory.toolResult('call_1', '25C')
    const assistantTc = MessageFactory.assistantWithToolCalls('checking', [tc])

    expect(user.role).toBe('user')
    expect(assistant.role).toBe('assistant')
    expect(system.role).toBe('system')
    expect(tool.role).toBe('tool')
    expect(tool.toolCallId).toBe('call_1')
    expect(assistantTc.toolCalls?.[0].name).toBe('get_weather')
  })

  it('has stop reason values', () => {
    const reasons: StopReason[] = ['end_turn', 'tool_use', 'max_tokens', 'stop_sequence', 'stop', 'other']
    expect(reasons).toContain('tool_use')
  })
})
