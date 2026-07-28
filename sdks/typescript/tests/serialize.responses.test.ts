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
  modelStreamAdapter,
  stopReasonFromStatus,
} from '../src/serialize/responses.js'
import { IncompleteStreamError, StreamError } from '../src/error.js'
import type {
  FreeformTool,
  ModelChatRequest,
  ModelStreamDelta,
  ModelToolSpec,
} from '../src/types.js'

const GRAMMAR_TOOL: FreeformTool = {
  name: 'exec',
  description: 'Run JavaScript',
  format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
}

function sseBody(text: string): ReadableStream<Uint8Array> {
  return new ReadableStream({
    start(controller) {
      controller.enqueue(new TextEncoder().encode(text))
      controller.close()
    },
  })
}

async function drainDeltas(sse: string, provider = 'openai'): Promise<ModelStreamDelta[]> {
  const deltas: ModelStreamDelta[] = []
  for await (const delta of modelStreamAdapter(sseBody(sse), provider)) deltas.push(delta)
  return deltas
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

describe('modelStreamAdapter', () => {
  it('decodes custom tool input deltas plus an authoritative tool_call_done', async () => {
    const sse =
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"console."}\n\n' +
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"log(1);\\n"}\n\n' +
      'data: {"type":"response.output_item.done","item":{"type":"custom_tool_call","call_id":"call_js","name":"exec","input":"console.log(1);\\n"}}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":2,"output_tokens":3}}}\n\n'

    expect(await drainDeltas(sse)).toEqual([
      { type: 'freeform_input', callId: 'call_js', delta: 'console.' },
      { type: 'freeform_input', callId: 'call_js', delta: 'log(1);\n' },
      {
        type: 'tool_call_done',
        call: { kind: 'freeform', id: 'call_js', name: 'exec', input: 'console.log(1);\n' },
      },
      { type: 'usage', usage: { inputTokens: 2, outputTokens: 3 } },
      { type: 'done', stopReason: 'tool_use' },
    ])
  })

  it('emits text deltas and skips empty ones', async () => {
    const sse =
      'data: {"type":"response.output_text.delta","delta":"hel"}\n\n' +
      'data: {"type":"response.output_text.delta","delta":""}\n\n' +
      'data: {"type":"response.output_text.delta","delta":"lo"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'

    expect(await drainDeltas(sse)).toEqual([
      { type: 'text', delta: 'hel' },
      { type: 'text', delta: 'lo' },
      { type: 'done', stopReason: 'end_turn' },
    ])
  })

  it('emits thinking deltas and a thinking_done from both reasoning families', async () => {
    const sse =
      'data: {"type":"response.reasoning_text.delta","delta":"think "}\n\n' +
      'data: {"type":"response.reasoning_summary_text.delta","delta":"hard"}\n\n' +
      'data: {"type":"response.reasoning_summary_text.done","text":"think hard"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'

    expect(await drainDeltas(sse)).toEqual([
      { type: 'thinking_delta', delta: 'think ' },
      { type: 'thinking_delta', delta: 'hard' },
      { type: 'thinking_done', thinking: 'think hard' },
      { type: 'done', stopReason: 'end_turn' },
    ])
  })

  it('resolves call_id through the item map when frames are keyed by item_id', async () => {
    const sse =
      'data: {"type":"response.output_item.added","item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"f"}}\n\n' +
      'data: {"type":"response.function_call_arguments.delta","item_id":"fc_1","delta":"{\\"a\\":1}"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'

    expect(await drainDeltas(sse)).toEqual([
      { type: 'function_arguments', callId: 'call_1', delta: '{"a":1}' },
      { type: 'done', stopReason: 'tool_use' },
    ])
  })

  it('falls back to the raw item_id when the map has no entry', async () => {
    const sse =
      'data: {"type":"response.function_call_arguments.delta","item_id":"fc_orphan","delta":"x"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'

    expect(await drainDeltas(sse)).toEqual([
      { type: 'function_arguments', callId: 'fc_orphan', delta: 'x' },
      { type: 'done', stopReason: 'end_turn' },
    ])
  })

  it('maps response.incomplete to a max_tokens done', async () => {
    const sse =
      'data: {"type":"response.output_text.delta","delta":"partial"}\n\n' +
      'data: {"type":"response.incomplete","response":{"status":"incomplete","usage":{"input_tokens":6,"output_tokens":7}}}\n\n'

    expect(await drainDeltas(sse)).toEqual([
      { type: 'text', delta: 'partial' },
      { type: 'usage', usage: { inputTokens: 6, outputTokens: 7 } },
      { type: 'done', stopReason: 'max_tokens' },
    ])
  })

  it('omits the usage delta when the terminal frame carries no usage', async () => {
    const sse = 'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
    expect(await drainDeltas(sse)).toEqual([{ type: 'done', stopReason: 'end_turn' }])
  })

  it('ignores empty, [DONE], malformed and unknown frames', async () => {
    const sse =
      'data: \n\n' +
      'data: [DONE]\n\n' +
      'data: {not json}\n\n' +
      'data: {"type":"response.unknown"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
    expect(await drainDeltas(sse)).toEqual([{ type: 'done', stopReason: 'end_turn' }])
  })

  it('drains pending deltas before surfacing a stream error frame', async () => {
    const sse =
      'data: {"type":"response.output_text.delta","delta":"before"}\n\n' +
      'data: {"type":"error","message":"upstream exploded"}\n\n'

    const deltas: ModelStreamDelta[] = []
    let caught: unknown
    try {
      for await (const delta of modelStreamAdapter(sseBody(sse), 'openai')) deltas.push(delta)
    } catch (error) {
      caught = error
    }
    expect(deltas).toEqual([{ type: 'text', delta: 'before' }])
    expect(caught).toBeInstanceOf(StreamError)
    expect((caught as Error).message).toBe('upstream exploded')
  })

  it('uses the nested response.error.message, then error.message, then a fallback', async () => {
    const nested =
      'data: {"type":"response.failed","response":{"error":{"message":"nested boom"}}}\n\n'
    await expect(drainDeltas(nested)).rejects.toThrow('nested boom')

    const top = 'data: {"type":"error","error":{"message":"top boom"}}\n\n'
    await expect(drainDeltas(top)).rejects.toThrow('top boom')

    const bare = 'data: {"type":"error"}\n\n'
    await expect(drainDeltas(bare)).rejects.toThrow('responses stream error')
  })

  it('throws IncompleteStreamError with the openai payload on EOF without a terminal', async () => {
    const sse =
      'data: {"type":"response.output_text.delta","delta":"hel"}\n\n' +
      'data: {"type":"response.output_text.delta","delta":"lo"}\n\n'

    const deltas: ModelStreamDelta[] = []
    let caught: unknown
    try {
      for await (const delta of modelStreamAdapter(sseBody(sse), 'openai')) deltas.push(delta)
    } catch (error) {
      caught = error
    }
    expect(deltas.some((d) => d.type === 'done')).toBe(false)
    expect(caught).toBeInstanceOf(IncompleteStreamError)
    expect(caught).toBeInstanceOf(StreamError)
    expect((caught as Error).message).toBe(
      'incomplete stream: openai ended without a terminal event',
    )
  })

  it('uses the hyphenated chatgpt-codex provider token on the native path', async () => {
    const sse =
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"console."}\n\n'
    await expect(drainDeltas(sse, 'chatgpt-codex')).rejects.toThrow(
      'incomplete stream: chatgpt-codex ended without a terminal event',
    )
  })

  it('emits exactly one done even when frames follow the terminal', async () => {
    const sse =
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n' +
      'data: {"type":"response.output_text.delta","delta":"trailing"}\n\n'
    const deltas = await drainDeltas(sse)
    expect(deltas.filter((d) => d.type === 'done')).toHaveLength(1)
  })
})
