import { afterEach, describe, expect, it, vi } from 'vitest'
import { Client } from '../src/client.js'
import { IncompleteStreamError, StreamError, UnsupportedFeatureError } from '../src/error.js'
import { ChatGptCodexProvider } from '../src/providers/chatgpt_codex.js'
import { OpenAIProvider } from '../src/providers/openai.js'
import { buildModelRequestBody } from '../src/serialize/responses.js'
import { collectModelStream } from '../src/stream.js'
import type { FreeformTool, ModelChatRequest, ModelStreamDelta } from '../src/types.js'

// Freeform parity conformance gates (specs/types.md § Native Model API).
// Cross-SDK mirrors:
// - sdks/rust/tests/freeform_conformance.rs
// - sdks/python/tests/test_freeform_conformance.py
//
// Expected values are taken from the Rust suite that already pins this
// behaviour (tests/core_types.rs, tests/openai_provider.rs,
// tests/chatgpt_codex.rs, tests/native_collect_stream.rs). Do not invent new
// fixtures where one exists.

const EXEC_TOOL: FreeformTool = {
  name: 'exec',
  description: 'Run JavaScript',
  format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
}

/** The Rust fixture for "looks like JSON but is JavaScript". */
const JS_THAT_LOOKS_LIKE_JSON = '{"this":"looks like json"}\nconsole.log(\'but is JS\');'

function sseFetch(sse: string, onRequest?: (url: string, init?: RequestInit) => void) {
  const mockFetch = vi.fn(async (url: string, init?: RequestInit) => {
    onRequest?.(url, init)
    return new Response(
      new ReadableStream<Uint8Array>({
        start(controller) {
          controller.enqueue(new TextEncoder().encode(sse))
          controller.close()
        },
      }),
      { status: 200, headers: { 'content-type': 'text/event-stream' } },
    )
  })
  vi.stubGlobal('fetch', mockFetch)
  return mockFetch
}

function jsonFetch(body: unknown, onRequest?: (url: string, init?: RequestInit) => void) {
  const mockFetch = vi.fn(async (url: string, init?: RequestInit) => {
    onRequest?.(url, init)
    return new Response(JSON.stringify(body), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    })
  })
  vi.stubGlobal('fetch', mockFetch)
  return mockFetch
}

async function drain(stream: AsyncIterable<ModelStreamDelta>) {
  const deltas: ModelStreamDelta[] = []
  let error: unknown
  try {
    for await (const delta of stream) deltas.push(delta)
  } catch (caught) {
    error = caught
  }
  return { deltas, error }
}

describe('Freeform conformance - tool definitions', () => {
  it('a freeform tool serializes with a mandatory, exact format object', () => {
    const body = buildModelRequestBody(
      { context: [], toolSpecs: [{ kind: 'freeform', tool: EXEC_TOOL }] },
      'm',
      false,
    )
    expect(body.tools).toEqual([
      {
        type: 'custom',
        name: 'exec',
        description: 'Run JavaScript',
        format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
      },
    ])
  })

  it('a function tool serializes inputSchema under the wire key `parameters`', () => {
    const body = buildModelRequestBody(
      {
        context: [],
        toolSpecs: [
          {
            kind: 'function',
            tool: {
              name: 'get_weather',
              description: 'Fetch the weather',
              inputSchema: { type: 'object' },
            },
          },
        ],
      },
      'm',
      false,
    )
    expect(body.tools).toEqual([
      {
        type: 'function',
        name: 'get_weather',
        description: 'Fetch the weather',
        parameters: { type: 'object' },
      },
    ])
  })
})

describe('Freeform conformance - ordered history replay', () => {
  it('preserves message / tool-call / tool-output order and byte-exact input', () => {
    const request: ModelChatRequest = {
      context: [
        { kind: 'message', message: { role: 'user', content: 'run js' } },
        {
          kind: 'toolCall',
          call: { kind: 'freeform', id: 'call_js', name: 'exec', input: JS_THAT_LOOKS_LIKE_JSON },
        },
        {
          kind: 'toolOutput',
          output: { kind: 'custom', callId: 'call_js', name: 'exec', output: 'done' },
        },
      ],
      toolSpecs: [{ kind: 'freeform', tool: EXEC_TOOL }],
    }
    const input = buildModelRequestBody(request, 'gpt-5.5-codex', false).input as Record<
      string,
      unknown
    >[]

    expect(input.map((item) => item.type)).toEqual([
      'message',
      'custom_tool_call',
      'custom_tool_call_output',
    ])
    // Byte-for-byte: never parsed as JSON, never lowered into `arguments`.
    expect(input[1].input).toBe(JS_THAT_LOOKS_LIKE_JSON)
    expect(input[1].arguments).toBeUndefined()
    // Identity travels under `call_id`, not `id`.
    expect(input[1].call_id).toBe('call_js')
    expect(input[1].id).toBeUndefined()
    expect(input[2].call_id).toBe('call_js')
  })

  it('hoists system messages into instructions and removes them from input', () => {
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

  it('maps maxTokens to max_output_tokens and merges providerOptions LAST', () => {
    const body = buildModelRequestBody(
      { context: [], maxTokens: 512, temperature: 0.1, providerOptions: { temperature: 0.9 } },
      'm',
      false,
    )
    expect(body.max_output_tokens).toBe(512)
    expect(body.max_tokens).toBeUndefined()
    expect(body.temperature).toBe(0.9)
  })
})

describe('Freeform conformance - pre-network rejection', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('OpenAI without the Responses opt-in rejects freeform before any HTTP call', async () => {
    const mockFetch = jsonFetch({})
    const provider = new OpenAIProvider('test-key', 'gpt-5.5-codex')
    const request: ModelChatRequest = {
      context: [{ kind: 'message', message: { role: 'user', content: 'run js' } }],
      toolSpecs: [{ kind: 'freeform', tool: EXEC_TOOL }],
    }

    await expect(provider.modelChat(request)).rejects.toBeInstanceOf(UnsupportedFeatureError)
    await expect(provider.modelChat(request)).rejects.toThrow(
      'provider does not support native freeform tools',
    )
    expect(() => provider.modelStream(request)).toThrow(UnsupportedFeatureError)
    expect(mockFetch).not.toHaveBeenCalled()
  })

  it('a Client over a non-freeform provider rejects freeform history before any HTTP call', () => {
    const mockFetch = jsonFetch({})
    const client = Client.builder().provider('openai').apiKey('test-key').build()
    expect(() =>
      client.modelStream({
        context: [
          { kind: 'toolOutput', output: { kind: 'custom', callId: 'call_js', output: 'x' } },
        ],
      }),
    ).toThrow('provider does not support native freeform tools')
    expect(mockFetch).not.toHaveBeenCalled()
  })
})

describe('Freeform conformance - stream termination', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('emits exactly one terminal done per successfully completed stream', async () => {
    sseFetch(
      'data: {"type":"response.output_text.delta","delta":"hi"}\n\n' +
        'data: {"type":"response.completed","response":{"status":"completed"}}\n\n' +
        'data: {"type":"response.output_text.delta","delta":"trailing"}\n\n',
    )
    const { deltas, error } = await drain(
      new OpenAIProvider('test-key', 'm').withResponsesApi(true).modelStream({ context: [] }),
    )
    expect(error).toBeUndefined()
    expect(deltas.filter((d) => d.type === 'done')).toHaveLength(1)
  })

  it('openai EOF without a terminal yields IncompleteStreamError with the exact payload', async () => {
    sseFetch(
      'data: {"type":"response.output_text.delta","delta":"hel"}\n\n' +
        'data: {"type":"response.output_text.delta","delta":"lo"}\n\n',
    )
    const { deltas, error } = await drain(
      new OpenAIProvider('test-key', 'm').withResponsesApi(true).modelStream({ context: [] }),
    )
    expect(deltas.some((d) => d.type === 'done')).toBe(false)
    expect(error).toBeInstanceOf(IncompleteStreamError)
    expect(error).toBeInstanceOf(StreamError)
    expect((error as Error).message).toBe(
      'incomplete stream: openai ended without a terminal event',
    )
  })

  it('chatgpt-codex EOF without a terminal yields the hyphenated payload', async () => {
    sseFetch(
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"console."}\n\n',
    )
    const { error } = await drain(new ChatGptCodexProvider('tok', 'acct').modelStream({ context: [] }))
    expect(error).toBeInstanceOf(IncompleteStreamError)
    expect((error as Error).message).toBe(
      'incomplete stream: chatgpt-codex ended without a terminal event',
    )
  })

  it('collectModelStream propagates the incomplete error rather than guessing a stop reason', async () => {
    sseFetch('data: {"type":"response.output_text.delta","delta":"partial"}\n\n')
    await expect(
      collectModelStream(
        new OpenAIProvider('test-key', 'm').withResponsesApi(true).modelStream({ context: [] }),
      ),
    ).rejects.toBeInstanceOf(IncompleteStreamError)
  })

  it('response.incomplete is a terminal that maps to max_tokens', async () => {
    sseFetch(
      'data: {"type":"response.output_text.delta","delta":"partial"}\n\n' +
        'data: {"type":"response.incomplete","response":{"status":"incomplete","usage":{"input_tokens":6,"output_tokens":7}}}\n\n',
    )
    const response = await new ChatGptCodexProvider('tok', 'acct').modelChat({ context: [] })
    expect(response.content).toBe('partial')
    expect(response.stopReason).toBe('max_tokens')
    expect(response.usage.outputTokens).toBe(7)
  })
})

describe('Freeform conformance - collector rules', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('ToolCallDone is authoritative over accumulated freeform deltas', async () => {
    // The deltas spell "console." + "log(1);" but the done frame is the truth.
    sseFetch(
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"console."}\n\n' +
        'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"log(1);"}\n\n' +
        'data: {"type":"response.output_item.done","item":{"type":"custom_tool_call","call_id":"call_js","name":"exec","input":"AUTHORITATIVE"}}\n\n' +
        'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":2,"output_tokens":3}}}\n\n',
    )
    const response = await collectModelStream(
      new OpenAIProvider('test-key', 'm').withResponsesApi(true).modelStream({ context: [] }),
    )
    expect(response.toolCalls).toEqual([
      { kind: 'freeform', id: 'call_js', name: 'exec', input: 'AUTHORITATIVE' },
    ])
    expect(response.stopReason).toBe('tool_use')
    expect(response.usage.outputTokens).toBe(3)
  })

  it('freeform input survives the whole stream byte-for-byte', async () => {
    const encoded = JSON.stringify(JS_THAT_LOOKS_LIKE_JSON)
    sseFetch(
      `data: {"type":"response.output_item.done","item":{"type":"custom_tool_call","call_id":"call_js","name":"exec","input":${encoded}}}\n\n` +
        'data: {"type":"response.completed","response":{"status":"completed"}}\n\n',
    )
    const response = await collectModelStream(
      new OpenAIProvider('test-key', 'm').withResponsesApi(true).modelStream({ context: [] }),
    )
    expect(response.toolCalls[0]).toEqual({
      kind: 'freeform',
      id: 'call_js',
      name: 'exec',
      input: JS_THAT_LOOKS_LIKE_JSON,
    })
  })

  it('usage REPLACES rather than merges', async () => {
    const response = await collectModelStream(
      (async function* (): AsyncGenerator<ModelStreamDelta> {
        yield { type: 'usage', usage: { inputTokens: 100, outputTokens: 100 } }
        yield { type: 'usage', usage: { inputTokens: 1, outputTokens: 2 } }
        yield { type: 'done', stopReason: 'end_turn' }
      })(),
    )
    expect(response.usage).toEqual({ inputTokens: 1, outputTokens: 2 })
  })

  it('ThinkingDone wins over accumulated thinking deltas', async () => {
    sseFetch(
      'data: {"type":"response.reasoning_text.delta","delta":"think "}\n\n' +
        'data: {"type":"response.reasoning_text.delta","delta":"hard"}\n\n' +
        'data: {"type":"response.reasoning_text.done","text":"AUTHORITATIVE"}\n\n' +
        'data: {"type":"response.output_text.delta","delta":"answer"}\n\n' +
        'data: {"type":"response.completed","response":{"status":"completed"}}\n\n',
    )
    const response = await collectModelStream(
      new OpenAIProvider('test-key', 'm').withResponsesApi(true).modelStream({ context: [] }),
    )
    expect(response.thinking).toBe('AUTHORITATIVE')
    expect(response.content).toBe('answer')
  })

  it('pending deltas drain before a stored stream error surfaces', async () => {
    sseFetch(
      'data: {"type":"response.output_text.delta","delta":"before"}\n\n' +
        'data: {"type":"error","message":"upstream exploded"}\n\n',
    )
    const { deltas, error } = await drain(
      new OpenAIProvider('test-key', 'm').withResponsesApi(true).modelStream({ context: [] }),
    )
    expect(deltas).toEqual([{ type: 'text', delta: 'before' }])
    expect(error).toBeInstanceOf(StreamError)
    expect((error as Error).message).toBe('upstream exploded')
  })
})

describe('Freeform conformance - Codex body normalization', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('reasoning_effort never reaches the wire and the per-request value wins', async () => {
    let sent: Record<string, any> = {}
    sseFetch('data: {"type":"response.completed","response":{"status":"completed"}}\n\n', (_u, init) => {
      sent = JSON.parse(String(init?.body))
    })

    await new ChatGptCodexProvider('tok', 'acct').reasoningEffort('low').modelChat({
      context: [{ kind: 'message', message: { role: 'user', content: 'hi' } }],
      providerOptions: { reasoning_effort: 'high' },
    })

    expect(sent.reasoning).toEqual({ effort: 'high', summary: 'auto' })
    expect('reasoning_effort' in sent).toBe(false)
  })

  it('codex hard-sets store/include/parallel_tool_calls and tool_choice auto', async () => {
    let sent: Record<string, any> = {}
    sseFetch('data: {"type":"response.completed","response":{"status":"completed"}}\n\n', (_u, init) => {
      sent = JSON.parse(String(init?.body))
    })

    await new ChatGptCodexProvider('tok', 'acct').modelChat({
      context: [{ kind: 'message', message: { role: 'user', content: 'hi' } }],
      toolChoice: { type: 'required' },
    })

    expect(sent.store).toBe(false)
    expect(sent.include).toEqual(['reasoning.encrypted_content'])
    expect(sent.parallel_tool_calls).toBe(true)
    expect(sent.tool_choice).toBe('auto')
  })
})
