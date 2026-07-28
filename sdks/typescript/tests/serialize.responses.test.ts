import { describe, expect, it } from 'vitest'
import {
  encodeContextItem,
  encodeInput,
  encodeToolCall,
  encodeToolOutput,
  encodeTools,
  encodeToolSpec,
} from '../src/serialize/responses.js'
import type { FreeformTool, ModelToolSpec } from '../src/types.js'

const GRAMMAR_TOOL: FreeformTool = {
  name: 'exec',
  description: 'Run JavaScript',
  format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
}

describe('encodeToolSpec', () => {
  it('encodes a freeform tool with the mandatory exact format object', () => {
    expect(encodeToolSpec({ kind: 'freeform', tool: GRAMMAR_TOOL })).toEqual({
      type: 'custom',
      name: 'exec',
      description: 'Run JavaScript',
      format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
    })
  })

  it('encodes a function tool with inputSchema under the wire key `parameters`', () => {
    const spec: ModelToolSpec = {
      kind: 'function',
      tool: {
        name: 'get_weather',
        description: 'Fetch the weather',
        inputSchema: { type: 'object', properties: { city: { type: 'string' } } },
      },
    }
    expect(encodeToolSpec(spec)).toEqual({
      type: 'function',
      name: 'get_weather',
      description: 'Fetch the weather',
      parameters: { type: 'object', properties: { city: { type: 'string' } } },
    })
  })

  it("defaults TypeScript's optional description/inputSchema to '' and {}", () => {
    expect(encodeToolSpec({ kind: 'function', tool: { name: 'noop' } })).toEqual({
      type: 'function',
      name: 'noop',
      description: '',
      parameters: {},
    })
  })

  it('encodeTools maps every spec in order', () => {
    expect(
      encodeTools([
        { kind: 'function', tool: { name: 'a', description: 'A', inputSchema: {} } },
        { kind: 'freeform', tool: GRAMMAR_TOOL },
      ]).map((t) => t.type),
    ).toEqual(['function', 'custom'])
  })
})

describe('encodeToolCall', () => {
  it('encodes a freeform call as custom_tool_call with id under call_id', () => {
    expect(
      encodeToolCall({ kind: 'freeform', id: 'call_js', name: 'exec', input: 'console.log(1);' }),
    ).toEqual({
      type: 'custom_tool_call',
      call_id: 'call_js',
      name: 'exec',
      input: 'console.log(1);',
    })
  })

  it('encodes a function call as function_call with a string arguments field', () => {
    expect(
      encodeToolCall({
        kind: 'function',
        id: 'call_1',
        name: 'get_weather',
        arguments: '{"city":"Paris"}',
      }),
    ).toEqual({
      type: 'function_call',
      call_id: 'call_1',
      name: 'get_weather',
      arguments: '{"city":"Paris"}',
    })
  })

  it('preserves freeform input byte-for-byte and never parses it as JSON', () => {
    const raw = '{"this":"looks like json"}\nconsole.log(\'but is JS\');'
    const encoded = encodeToolCall({ kind: 'freeform', id: 'call_js', name: 'exec', input: raw })
    expect(encoded.input).toBe(raw)
    expect(typeof encoded.input).toBe('string')
  })
})

describe('encodeToolOutput', () => {
  it('encodes a function output', () => {
    expect(encodeToolOutput({ kind: 'function', callId: 'call_1', output: 'sunny, 21C' })).toEqual({
      type: 'function_call_output',
      call_id: 'call_1',
      output: 'sunny, 21C',
    })
  })

  it('encodes a custom output and drops `name` (mirrors Rust encode_tool_output)', () => {
    expect(
      encodeToolOutput({ kind: 'custom', callId: 'call_js', name: 'exec', output: 'stdout: 42' }),
    ).toEqual({
      type: 'custom_tool_call_output',
      call_id: 'call_js',
      output: 'stdout: 42',
    })
  })

  it('encodes a content-item payload with snake_case wire keys', () => {
    expect(
      encodeToolOutput({
        kind: 'function',
        callId: 'call_1',
        output: [
          { type: 'input_text', text: 'hi' },
          { type: 'input_image', imageUrl: 'https://e.example/a.png', detail: 'high' },
          { type: 'encrypted_content', encryptedContent: 'zzz' },
        ],
      }),
    ).toEqual({
      type: 'function_call_output',
      call_id: 'call_1',
      output: [
        { type: 'input_text', text: 'hi' },
        { type: 'input_image', image_url: 'https://e.example/a.png', detail: 'high' },
        { type: 'encrypted_content', encrypted_content: 'zzz' },
      ],
    })
  })
})

describe('encodeContextItem / encodeInput', () => {
  it('drops system messages from input', () => {
    expect(
      encodeContextItem({ kind: 'message', message: { role: 'system', content: 'be terse' } }),
    ).toEqual([])
  })

  it('encodes a user message as a message item with input_text', () => {
    expect(
      encodeContextItem({ kind: 'message', message: { role: 'user', content: 'run js' } }),
    ).toEqual([
      { type: 'message', role: 'user', content: [{ type: 'input_text', text: 'run js' }] },
    ])
  })

  it('encodes user image blocks as input_image data URLs', () => {
    expect(
      encodeContextItem({
        kind: 'message',
        message: {
          role: 'user',
          content: 'inspect',
          contentBlocks: [
            { type: 'text', text: 'inspect' },
            { type: 'image', source: { type: 'base64', mediaType: 'image/png', data: 'abc123' } },
          ],
        },
      }),
    ).toEqual([
      {
        type: 'message',
        role: 'user',
        content: [
          { type: 'input_text', text: 'inspect' },
          { type: 'input_image', image_url: 'data:image/png;base64,abc123' },
        ],
      },
    ])
  })

  it('splits an assistant message with tool calls into text + function_call items', () => {
    expect(
      encodeContextItem({
        kind: 'message',
        message: {
          role: 'assistant',
          content: 'let me look',
          toolCalls: [{ id: 'call_1', name: 'get_weather', input: { city: 'Paris' } }],
        },
      }),
    ).toEqual([
      {
        type: 'message',
        role: 'assistant',
        content: [{ type: 'output_text', text: 'let me look' }],
      },
      {
        type: 'function_call',
        call_id: 'call_1',
        name: 'get_weather',
        arguments: '{"city":"Paris"}',
      },
    ])
  })

  it('encodes a tool-role message as a function_call_output', () => {
    expect(
      encodeContextItem({
        kind: 'message',
        message: { role: 'tool', content: 'sunny', toolCallId: 'call_1' },
      }),
    ).toEqual([{ type: 'function_call_output', call_id: 'call_1', output: 'sunny' }])
  })

  it('preserves mixed message / toolCall / toolOutput order', () => {
    const items = encodeInput([
      { kind: 'message', message: { role: 'user', content: 'run it' } },
      {
        kind: 'toolCall',
        call: { kind: 'freeform', id: 'call_js', name: 'exec', input: 'console.log(1);' },
      },
      {
        kind: 'toolOutput',
        output: { kind: 'custom', callId: 'call_js', name: 'exec', output: '1\n' },
      },
    ])
    expect(items.map((item) => item.type)).toEqual([
      'message',
      'custom_tool_call',
      'custom_tool_call_output',
    ])
  })
})
