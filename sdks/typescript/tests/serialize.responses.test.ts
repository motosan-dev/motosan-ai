import { describe, expect, it } from 'vitest'
import {
  buildModelRequestBody,
  decodeOutputText,
  decodeToolCall,
  decodeToolOutput,
  decodeUsage,
  encodeContextItem,
  encodeInput,
  encodeToolCall,
  encodeToolOutput,
  encodeTools,
  encodeToolSpec,
  modelChatResponseFromOutput,
  stopReasonFromStatus,
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

describe('decodeToolCall', () => {
  it('decodes a custom_tool_call and keeps input byte-exact', () => {
    const raw = 'const x = {a: 1};\nconsole.log(`raw ${x.a}`);\n'
    expect(
      decodeToolCall({ type: 'custom_tool_call', call_id: 'call_js', name: 'exec', input: raw }),
    ).toEqual({ kind: 'freeform', id: 'call_js', name: 'exec', input: raw })
  })

  it('decodes a function_call', () => {
    expect(
      decodeToolCall({
        type: 'function_call',
        call_id: 'call_1',
        name: 'get_weather',
        arguments: '{"city":"Paris"}',
      }),
    ).toEqual({
      kind: 'function',
      id: 'call_1',
      name: 'get_weather',
      arguments: '{"city":"Paris"}',
    })
  })

  it('accepts `id` when `call_id` is absent', () => {
    expect(decodeToolCall({ type: 'function_call', id: 'call_2', name: 'f' })).toEqual({
      kind: 'function',
      id: 'call_2',
      name: 'f',
      arguments: '',
    })
  })

  it('returns undefined for non-call items', () => {
    expect(decodeToolCall({ type: 'message', role: 'assistant' })).toBeUndefined()
    expect(decodeToolCall({ type: 'function_call' })).toBeUndefined()
    expect(decodeToolCall('nope')).toBeUndefined()
  })

  it('round-trips a freeform call through encode then decode', () => {
    const raw = 'const x = {a: 1};\nconsole.log(`raw ${x.a}`);\n'
    const call = { kind: 'freeform', id: 'call_js', name: 'exec', input: raw } as const
    expect(decodeToolCall(JSON.parse(JSON.stringify(encodeToolCall(call))))).toEqual(call)
  })
})

describe('decodeToolOutput', () => {
  it('decodes a custom output including its optional name', () => {
    expect(
      decodeToolOutput({
        type: 'custom_tool_call_output',
        call_id: 'call_js',
        name: 'exec',
        output: 'stdout: 42',
      }),
    ).toEqual({ kind: 'custom', callId: 'call_js', name: 'exec', output: 'stdout: 42' })
  })

  it('omits name when the wire has none', () => {
    expect(
      decodeToolOutput({ type: 'custom_tool_call_output', call_id: 'call_js', output: 'x' }),
    ).toEqual({ kind: 'custom', callId: 'call_js', output: 'x' })
  })

  it('decodes a function output with content items', () => {
    expect(
      decodeToolOutput({
        type: 'function_call_output',
        call_id: 'call_1',
        output: [{ type: 'input_text', text: 'hi' }],
      }),
    ).toEqual({ kind: 'function', callId: 'call_1', output: [{ type: 'input_text', text: 'hi' }] })
  })

  it('returns undefined without a payload or a known type', () => {
    expect(decodeToolOutput({ type: 'function_call_output', call_id: 'c' })).toBeUndefined()
    expect(decodeToolOutput({ type: 'other', call_id: 'c', output: 'x' })).toBeUndefined()
  })
})

describe('decodeOutputText', () => {
  it('concatenates output_text parts of a message item', () => {
    expect(
      decodeOutputText({
        type: 'message',
        content: [
          { type: 'output_text', text: 'Hi ' },
          { type: 'refusal', text: 'IGNORED' },
          { type: 'output_text', text: 'there' },
        ],
      }),
    ).toBe('Hi there')
  })

  it('returns undefined for non-message items and empty text', () => {
    expect(decodeOutputText({ type: 'reasoning' })).toBeUndefined()
    expect(decodeOutputText({ type: 'message', content: [] })).toBeUndefined()
  })
})

describe('decodeUsage', () => {
  it('reads Responses keys', () => {
    expect(decodeUsage({ input_tokens: 9, output_tokens: 7 })).toEqual({
      inputTokens: 9,
      outputTokens: 7,
    })
  })

  it('falls back to chat-completions key names', () => {
    expect(decodeUsage({ prompt_tokens: 4, completion_tokens: 5 })).toEqual({
      inputTokens: 4,
      outputTokens: 5,
    })
  })

  it('maps cached_tokens only when positive', () => {
    expect(
      decodeUsage({ input_tokens: 1, output_tokens: 1, input_tokens_details: { cached_tokens: 3 } }),
    ).toEqual({ inputTokens: 1, outputTokens: 1, cacheReadInputTokens: 3 })
    expect(
      decodeUsage({ input_tokens: 1, output_tokens: 1, input_tokens_details: { cached_tokens: 0 } }),
    ).toEqual({ inputTokens: 1, outputTokens: 1 })
  })

  it('returns zeros for missing usage', () => {
    expect(decodeUsage(undefined)).toEqual({ inputTokens: 0, outputTokens: 0 })
  })
})

describe('stopReasonFromStatus', () => {
  it('prefers tool_use whenever there are tool calls', () => {
    expect(stopReasonFromStatus('incomplete', true)).toBe('tool_use')
  })

  it('maps statuses', () => {
    expect(stopReasonFromStatus('completed', false)).toBe('end_turn')
    expect(stopReasonFromStatus(undefined, false)).toBe('end_turn')
    expect(stopReasonFromStatus('incomplete', false)).toBe('max_tokens')
    expect(stopReasonFromStatus('failed', false)).toBe('other')
    expect(stopReasonFromStatus('weird', false)).toBe('other')
  })
})

describe('modelChatResponseFromOutput', () => {
  it('decodes a freeform tool call with tool_use and usage', () => {
    const raw = 'const x = {a: 1};\nconsole.log(x.a);\n'
    const response = modelChatResponseFromOutput(
      {
        model: 'gpt-5.5-codex',
        status: 'completed',
        output: [{ type: 'custom_tool_call', call_id: 'call_js', name: 'exec', input: raw }],
        usage: { input_tokens: 9, output_tokens: 7 },
      },
      'fallback-model',
    )
    expect(response.toolCalls).toEqual([
      { kind: 'freeform', id: 'call_js', name: 'exec', input: raw },
    ])
    expect(response.stopReason).toBe('tool_use')
    expect(response.usage.inputTokens).toBe(9)
    expect(response.model).toBe('gpt-5.5-codex')
    expect(response.content).toBe('')
  })

  it('concatenates output_text items into content', () => {
    const response = modelChatResponseFromOutput(
      {
        status: 'completed',
        output: [
          { type: 'message', role: 'assistant', content: [{ type: 'output_text', text: 'ok' }] },
        ],
        usage: { input_tokens: 1, output_tokens: 1 },
      },
      'gpt-5.5-codex',
    )
    expect(response.content).toBe('ok')
    expect(response.model).toBe('gpt-5.5-codex')
    expect(response.stopReason).toBe('end_turn')
  })

  it('keeps reasoning summary text in thinking, separate from content', () => {
    const response = modelChatResponseFromOutput(
      {
        status: 'completed',
        output: [
          { type: 'reasoning', summary: [{ text: 'private reasoning' }] },
          { type: 'message', content: [{ type: 'output_text', text: 'answer' }] },
        ],
      },
      'm',
    )
    expect(response.content).toBe('answer')
    expect(response.thinking).toBe('private reasoning')
  })

  it('omits thinking entirely when there is no reasoning summary', () => {
    const response = modelChatResponseFromOutput({ status: 'completed' }, 'm')
    expect('thinking' in response).toBe(false)
  })
})
