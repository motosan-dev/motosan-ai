import { describe, expect, it } from 'vitest'
import {
  buildModelRequestBody,
  encodeContextItem,
  encodeInput,
  encodeToolCall,
  encodeToolOutput,
  encodeTools,
  encodeToolSpec,
} from '../src/serialize/responses.js'
import type { FreeformTool, ModelChatRequest, ModelToolSpec } from '../src/types.js'

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

describe('buildModelRequestBody', () => {
  it('uses the request model, falling back to the provider default', () => {
    expect(
      buildModelRequestBody({ context: [], model: 'gpt-5.5-codex' }, 'gpt-5.5', false).model,
    ).toBe('gpt-5.5-codex')
    expect(buildModelRequestBody({ context: [] }, 'gpt-5.5', false).model).toBe('gpt-5.5')
  })

  it('sets stream only when asked', () => {
    expect(buildModelRequestBody({ context: [] }, 'm', false).stream).toBeUndefined()
    expect(buildModelRequestBody({ context: [] }, 'm', true).stream).toBe(true)
  })

  it('hoists a system message into instructions AND removes it from input', () => {
    const body = buildModelRequestBody(
      {
        context: [
          { kind: 'message', message: { role: 'system', content: 'be terse' } },
          { kind: 'message', message: { role: 'user', content: 'hi' } },
        ],
      },
      'm',
      false,
    )
    expect(body.instructions).toBe('be terse')
    expect(body.input).toHaveLength(1)
    expect((body.input as Record<string, unknown>[])[0].role).toBe('user')
  })

  it('prefers systemBlocks over system and joins with a blank line', () => {
    const body = buildModelRequestBody(
      {
        context: [],
        system: 'ignored',
        systemBlocks: [{ text: 'a' }, { text: '  ' }, { text: 'b' }],
      },
      'm',
      false,
    )
    expect(body.instructions).toBe('a\n\nb')
  })

  it('appends hoisted system messages after system/systemBlocks', () => {
    const body = buildModelRequestBody(
      {
        context: [{ kind: 'message', message: { role: 'system', content: 'second' } }],
        system: 'first',
      },
      'm',
      false,
    )
    expect(body.instructions).toBe('first\n\nsecond')
  })

  it('falls back to defaultInstructions only when nothing was supplied', () => {
    expect(
      buildModelRequestBody({ context: [] }, 'm', false, 'You are a helpful assistant.')
        .instructions,
    ).toBe('You are a helpful assistant.')
    expect(
      buildModelRequestBody({ context: [], system: 'given' }, 'm', false, 'default').instructions,
    ).toBe('given')
    expect(buildModelRequestBody({ context: [] }, 'm', false).instructions).toBeUndefined()
  })

  it('maps maxTokens to max_output_tokens and never emits max_tokens', () => {
    const body = buildModelRequestBody({ context: [], maxTokens: 512 }, 'm', false)
    expect(body.max_output_tokens).toBe(512)
    expect(body.max_tokens).toBeUndefined()
  })

  it('emits temperature, tool_choice and stop when set', () => {
    const body = buildModelRequestBody(
      {
        context: [],
        temperature: 0.3,
        toolChoice: { type: 'tool', name: 'exec' },
        stopSequences: ['STOP'],
      },
      'm',
      false,
    )
    expect(body.temperature).toBe(0.3)
    expect(body.tool_choice).toEqual({ type: 'function', name: 'exec' })
    expect(body.stop).toEqual(['STOP'])
  })

  it('maps the string tool choices', () => {
    expect(
      buildModelRequestBody({ context: [], toolChoice: { type: 'auto' } }, 'm', false)
        .tool_choice,
    ).toBe('auto')
    expect(
      buildModelRequestBody({ context: [], toolChoice: { type: 'required' } }, 'm', false)
        .tool_choice,
    ).toBe('required')
    expect(
      buildModelRequestBody({ context: [], toolChoice: { type: 'none' } }, 'm', false)
        .tool_choice,
    ).toBe('none')
  })

  it('omits stop for an empty stopSequences array', () => {
    expect(buildModelRequestBody({ context: [], stopSequences: [] }, 'm', false).stop).toBeUndefined()
  })

  it('omits tools when there are no tool specs', () => {
    expect(buildModelRequestBody({ context: [] }, 'm', false).tools).toBeUndefined()
    expect(buildModelRequestBody({ context: [], toolSpecs: [] }, 'm', false).tools).toBeUndefined()
  })

  it('shallow-merges providerOptions LAST so it overrides encoder output', () => {
    const body = buildModelRequestBody(
      {
        context: [],
        temperature: 0.1,
        providerOptions: { temperature: 0.9, reasoning_effort: 'high', custom: 1 },
      },
      'm',
      false,
    )
    expect(body.temperature).toBe(0.9)
    expect(body.reasoning_effort).toBe('high')
    expect(body.custom).toBe(1)
  })

  it('replays a symmetric freeform history byte-exact and in order', () => {
    const raw = '{"this":"looks like json"}\nconsole.log(\'but is JS\');'
    const req: ModelChatRequest = {
      context: [
        { kind: 'message', message: { role: 'user', content: 'run js' } },
        { kind: 'toolCall', call: { kind: 'freeform', id: 'call_js', name: 'exec', input: raw } },
        {
          kind: 'toolOutput',
          output: { kind: 'custom', callId: 'call_js', name: 'exec', output: 'done' },
        },
      ],
      toolSpecs: [
        {
          kind: 'freeform',
          tool: {
            name: 'exec',
            description: 'Run JavaScript',
            format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
          },
        },
      ],
    }
    const body = buildModelRequestBody(req, 'gpt-5.5-codex', false)
    const input = body.input as Record<string, unknown>[]
    expect(input.map((item) => item.type)).toEqual([
      'message',
      'custom_tool_call',
      'custom_tool_call_output',
    ])
    expect(input[1].input).toBe(raw)
    expect(input[1].call_id).toBe('call_js')
    expect((body.tools as Record<string, unknown>[])[0].type).toBe('custom')
  })
})
