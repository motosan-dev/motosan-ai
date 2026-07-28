import { afterEach, describe, expect, it, vi } from 'vitest'
import { IncompleteStreamError, StreamReadTimeoutError, UnsupportedFeatureError } from '../src/error.js'
import { Client, ClientBuilder, type ProviderLike } from '../src/client.js'
import {
  dispatchModelChat,
  dispatchModelStream,
  readTimeoutModelStream,
  textOnly,
  validateModelRequest,
  withFreeformTools,
  withImage,
  withImageAndFreeformTools,
  type ProviderImpl,
} from '../src/provider.js'
import { ChatGptCodexProvider } from '../src/providers/chatgpt_codex.js'
import { DEFAULT_OPENAI_RESPONSES_URL, OpenAIProvider } from '../src/providers/openai.js'
import { collectModelStream } from '../src/stream.js'
import type { ModelChatRequest, ModelChatResponse, ModelStreamDelta } from '../src/types.js'

async function* deltas(items: ModelStreamDelta[]): AsyncGenerator<ModelStreamDelta> {
  for (const item of items) yield item
}

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

describe('collectModelStream', () => {
  it('preserves the completed freeform call and never lowers accumulated deltas', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'freeform_input', callId: 'call_js', delta: 'console.' },
        { type: 'freeform_input', callId: 'call_js', delta: 'log(1);' },
        {
          type: 'tool_call_done',
          call: { kind: 'freeform', id: 'call_js', name: 'exec', input: 'console.log(1);' },
        },
        { type: 'usage', usage: { inputTokens: 2, outputTokens: 3 } },
        { type: 'done', stopReason: 'tool_use' },
      ]),
    )

    expect(response.toolCalls).toEqual([
      { kind: 'freeform', id: 'call_js', name: 'exec', input: 'console.log(1);' },
    ])
    expect(response.stopReason).toBe('tool_use')
    expect(response.usage.outputTokens).toBe(3)
    expect(response.model).toBe('')
  })

  it('prefers thinking_done over accumulated thinking deltas', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'thinking_delta', delta: 'think ' },
        { type: 'thinking_delta', delta: 'hard' },
        { type: 'thinking_done', thinking: 'think hard' },
        { type: 'text', delta: 'answer' },
        { type: 'done', stopReason: 'end_turn' },
      ]),
    )
    expect(response.content).toBe('answer')
    expect(response.thinking).toBe('think hard')
  })

  it('falls back to concatenated thinking deltas when no thinking_done arrives', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'thinking_delta', delta: 'partial ' },
        { type: 'thinking_delta', delta: 'reasoning' },
        { type: 'done', stopReason: 'end_turn' },
      ]),
    )
    expect(response.thinking).toBe('partial reasoning')
  })

  it('drops an empty thinking_done payload entirely', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'thinking_delta', delta: 'ignored' },
        { type: 'thinking_done', thinking: '' },
        { type: 'done', stopReason: 'end_turn' },
      ]),
    )
    expect(response.thinking).toBeUndefined()
  })

  it('REPLACES usage rather than merging it', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'usage', usage: { inputTokens: 100, outputTokens: 100, cacheReadInputTokens: 9 } },
        { type: 'usage', usage: { inputTokens: 1, outputTokens: 2 } },
        { type: 'done', stopReason: 'end_turn' },
      ]),
    )
    expect(response.usage).toEqual({ inputTokens: 1, outputTokens: 2 })
  })

  it('stops at the terminal done and ignores anything after it', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'text', delta: 'kept' },
        { type: 'done', stopReason: 'end_turn' },
        { type: 'text', delta: 'dropped' },
      ]),
    )
    expect(response.content).toBe('kept')
  })

  it('infers tool_use when no done delta carried a stop reason', async () => {
    const response = await collectModelStream(
      deltas([
        {
          type: 'tool_call_done',
          call: { kind: 'function', id: 'call_1', name: 'f', arguments: '{}' },
        },
      ]),
    )
    expect(response.stopReason).toBe('tool_use')
  })

  it('infers end_turn for a bare text stream with no done delta', async () => {
    const response = await collectModelStream(deltas([{ type: 'text', delta: 'hi' }]))
    expect(response.stopReason).toBe('end_turn')
  })

  it('propagates a mid-stream error instead of returning a partial response', async () => {
    async function* failing(): AsyncGenerator<ModelStreamDelta> {
      yield { type: 'text', delta: 'partial' }
      throw new Error('boom')
    }
    await expect(collectModelStream(failing())).rejects.toThrow('boom')
  })
})

const FREEFORM_SPEC_REQUEST: ModelChatRequest = {
  context: [{ kind: 'message', message: { role: 'user', content: 'run js' } }],
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

const FREEFORM_TOOL = {
  name: 'exec',
  description: 'Run JavaScript',
  format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
} as const

describe('freeform capability factories (D5)', () => {
  it('withFreeformTools() is text-only plus freeform', () => {
    expect(withFreeformTools()).toEqual({
      supportsImage: false,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: true,
    })
  })

  it('withImageAndFreeformTools() adds images', () => {
    expect(withImageAndFreeformTools()).toEqual({
      supportsImage: true,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: true,
    })
  })

  it('the pre-existing factories keep freeform false', () => {
    expect(textOnly().supportsFreeformTools).toBe(false)
    expect(withImage().supportsFreeformTools).toBe(false)
  })
})

describe('validateModelRequest', () => {
  it('rejects a freeform tool spec on a non-freeform provider', () => {
    expect(() => validateModelRequest(FREEFORM_SPEC_REQUEST, withImage())).toThrow(
      UnsupportedFeatureError,
    )
    expect(() => validateModelRequest(FREEFORM_SPEC_REQUEST, withImage())).toThrow(
      'provider does not support native freeform tools',
    )
  })

  it('rejects freeform history even without a freeform spec', () => {
    const withCall: ModelChatRequest = {
      context: [
        { kind: 'toolCall', call: { kind: 'freeform', id: 'c', name: 'exec', input: 'x' } },
      ],
    }
    const withOutput: ModelChatRequest = {
      context: [{ kind: 'toolOutput', output: { kind: 'custom', callId: 'c', output: 'x' } }],
    }
    expect(() => validateModelRequest(withCall, withImage())).toThrow(UnsupportedFeatureError)
    expect(() => validateModelRequest(withOutput, withImage())).toThrow(UnsupportedFeatureError)
  })

  it('accepts freeform on a freeform-capable provider', () => {
    expect(() => validateModelRequest(FREEFORM_SPEC_REQUEST, withFreeformTools())).not.toThrow()
  })

  it('accepts function specs and function history everywhere', () => {
    const req: ModelChatRequest = {
      context: [
        { kind: 'toolCall', call: { kind: 'function', id: 'c', name: 'f', arguments: '{}' } },
        { kind: 'toolOutput', output: { kind: 'function', callId: 'c', output: 'ok' } },
      ],
      toolSpecs: [{ kind: 'function', tool: { name: 'f', description: 'F', inputSchema: {} } }],
    }
    expect(() => validateModelRequest(req, textOnly())).not.toThrow()
  })

  it('rejects image blocks in context on a text-only provider', () => {
    const req: ModelChatRequest = {
      context: [
        {
          kind: 'message',
          message: {
            role: 'user',
            content: '',
            contentBlocks: [
              { type: 'image', source: { type: 'url', url: 'https://e.example/a.png' } },
            ],
          },
        },
      ],
    }
    expect(() => validateModelRequest(req, withFreeformTools())).toThrow(
      'provider does not support image input',
    )
    expect(() => validateModelRequest(req, withImageAndFreeformTools())).not.toThrow()
  })

  it('rejects document blocks in context', () => {
    const req: ModelChatRequest = {
      context: [
        {
          kind: 'message',
          message: {
            role: 'user',
            content: '',
            contentBlocks: [
              { type: 'document', source: { type: 'url', url: 'https://e.example/a.pdf' } },
            ],
          },
        },
      ],
    }
    expect(() => validateModelRequest(req, withImageAndFreeformTools())).toThrow(
      'provider does not support document input',
    )
  })
})

describe('native dispatch', () => {
  const bareProvider: ProviderImpl = {
    capabilities: withFreeformTools,
    chat: async () => {
      throw new Error('unused')
    },
    stream: () => {
      throw new Error('unused')
    },
  }

  it('rejects modelChat on a provider that implements no native surface', async () => {
    await expect(dispatchModelChat(bareProvider, { context: [] })).rejects.toThrow(
      'provider does not support native model requests',
    )
  })

  it('rejects modelStream on a provider that implements no native surface', () => {
    expect(() => dispatchModelStream(bareProvider, { context: [] })).toThrow(
      'provider does not support native model streams',
    )
  })

  it('validates BEFORE consulting the native surface', async () => {
    const imageOnly: ProviderImpl = { ...bareProvider, capabilities: withImage }
    await expect(dispatchModelChat(imageOnly, FREEFORM_SPEC_REQUEST)).rejects.toThrow(
      'provider does not support native freeform tools',
    )
  })
})

describe('readTimeoutModelStream', () => {
  it('passes deltas through untouched', async () => {
    const out: ModelStreamDelta[] = []
    for await (const delta of readTimeoutModelStream(
      deltas([{ type: 'text', delta: 'a' }, { type: 'done', stopReason: 'end_turn' }]),
      5,
    )) {
      out.push(delta)
    }
    expect(out).toEqual([{ type: 'text', delta: 'a' }, { type: 'done', stopReason: 'end_turn' }])
  })

  it('throws StreamReadTimeoutError on an idle gap and fabricates no done', async () => {
    async function* stalls(): AsyncGenerator<ModelStreamDelta> {
      yield { type: 'text', delta: 'tick' }
      await new Promise((resolve) => setTimeout(resolve, 500))
      yield { type: 'done', stopReason: 'end_turn' }
    }

    const out: ModelStreamDelta[] = []
    let caught: unknown
    try {
      for await (const delta of readTimeoutModelStream(stalls(), 0.05)) out.push(delta)
    } catch (error) {
      caught = error
    }
    expect(caught).toBeInstanceOf(StreamReadTimeoutError)
    expect(out).toEqual([{ type: 'text', delta: 'tick' }])
  })
})

describe('ChatGptCodexProvider native surface', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('declares freeform yes / image no / document no', () => {
    expect(new ChatGptCodexProvider('tok', 'acct').capabilities()).toEqual({
      supportsImage: false,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: true,
    })
  })

  it('hard-sets the codex body fields, overriding the caller tool choice', () => {
    const body = new ChatGptCodexProvider('tok', 'acct').buildModelResponsesBody({
      context: [{ kind: 'message', message: { role: 'user', content: 'hi' } }],
      toolChoice: { type: 'required' },
    })
    expect(body.store).toBe(false)
    expect(body.stream).toBe(true)
    expect(body.include).toEqual(['reasoning.encrypted_content'])
    expect(body.parallel_tool_calls).toBe(true)
    expect(body.tool_choice).toBe('auto')
    expect(body.instructions).toBe('You are a helpful assistant.')
    expect(body.model).toBe('gpt-5.5')
  })

  it('normalizes a per-request reasoning effort and deletes the raw key', () => {
    const body = new ChatGptCodexProvider('tok', 'acct').buildModelResponsesBody({
      context: [],
      providerOptions: { reasoning_effort: 'high' },
    })
    expect(body.reasoning).toEqual({ effort: 'high', summary: 'auto' })
    expect(body.reasoning_effort).toBeUndefined()
    expect('reasoning_effort' in body).toBe(false)
  })

  it('lets the per-request effort beat the provider default', () => {
    const body = new ChatGptCodexProvider('tok', 'acct')
      .reasoningEffort('low')
      .buildModelResponsesBody({ context: [], providerOptions: { reasoning_effort: 'high' } })
    expect(body.reasoning).toEqual({ effort: 'high', summary: 'auto' })
  })

  it('uses the provider default when the request supplies none', () => {
    const body = new ChatGptCodexProvider('tok', 'acct')
      .reasoningEffort('high')
      .buildModelResponsesBody({ context: [] })
    expect(body.reasoning).toEqual({ effort: 'high', summary: 'auto' })
  })

  it('omits reasoning entirely when neither is set', () => {
    expect(
      new ChatGptCodexProvider('tok', 'acct').buildModelResponsesBody({ context: [] }).reasoning,
    ).toBeUndefined()
  })

  it('streams a freeform call and collects it', async () => {
    const sse =
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"console."}\n\n' +
      'data: {"type":"response.output_item.done","item":{"type":"custom_tool_call","call_id":"call_js","name":"exec","input":"console.log(1);\\n"}}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":2,"output_tokens":3}}}\n\n'
    sseFetch(sse)

    const response = await collectModelStream(
      new ChatGptCodexProvider('tok', 'acct').modelStream({
        context: [{ kind: 'message', message: { role: 'user', content: 'run js' } }],
        toolSpecs: [{ kind: 'freeform', tool: FREEFORM_TOOL }],
      }),
    )
    expect(response.toolCalls).toEqual([
      { kind: 'freeform', id: 'call_js', name: 'exec', input: 'console.log(1);\n' },
    ])
    expect(response.stopReason).toBe('tool_use')
    expect(response.usage.outputTokens).toBe(3)
  })

  it('modelChat collects the native stream and backfills the model id', async () => {
    const sse =
      `data: {"type":"response.output_item.done","item":{"type":"custom_tool_call","call_id":"call_js","name":"exec","input":"text('captured');"}}\n\n` +
      'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":4,"output_tokens":5}}}\n\n'
    sseFetch(sse)

    const response = await new ChatGptCodexProvider('tok', 'acct').modelChat({
      context: [{ kind: 'message', message: { role: 'user', content: 'run js' } }],
      toolSpecs: [{ kind: 'freeform', tool: FREEFORM_TOOL }],
    })
    expect(response.toolCalls).toEqual([
      { kind: 'freeform', id: 'call_js', name: 'exec', input: "text('captured');" },
    ])
    expect(response.model).toBe('gpt-5.5')
  })

  it('maps response.incomplete to max_tokens', async () => {
    sseFetch(
      'data: {"type":"response.output_text.delta","delta":"partial"}\n\n' +
        'data: {"type":"response.incomplete","response":{"status":"incomplete","usage":{"input_tokens":6,"output_tokens":7}}}\n\n',
    )
    const response = await new ChatGptCodexProvider('tok', 'acct').modelChat({
      context: [{ kind: 'message', message: { role: 'user', content: 'short' } }],
    })
    expect(response.content).toBe('partial')
    expect(response.stopReason).toBe('max_tokens')
    expect(response.usage.outputTokens).toBe(7)
  })

  it('sends the custom tool and the symmetric history byte-exact', async () => {
    const raw = '{"this":"looks like json"}\nconsole.log(\'but is JS\');'
    let sent: Record<string, any> = {}
    sseFetch(
      'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":1,"output_tokens":1}}}\n\n',
      (_url, init) => {
        sent = JSON.parse(String(init?.body))
      },
    )

    const response = await new ChatGptCodexProvider('tok', 'acct').modelChat({
      context: [
        { kind: 'message', message: { role: 'user', content: 'run js' } },
        { kind: 'toolCall', call: { kind: 'freeform', id: 'call_js', name: 'exec', input: raw } },
        {
          kind: 'toolOutput',
          output: { kind: 'custom', callId: 'call_js', name: 'exec', output: 'done' },
        },
      ],
      toolSpecs: [{ kind: 'freeform', tool: FREEFORM_TOOL }],
    })

    expect(sent.tools[0].type).toBe('custom')
    expect(sent.input.map((item: Record<string, unknown>) => item.type)).toEqual([
      'message',
      'custom_tool_call',
      'custom_tool_call_output',
    ])
    expect(sent.input[1].input).toBe(raw)
    expect(response.stopReason).toBe('end_turn')
  })

  it('throws IncompleteStreamError with the hyphenated provider token on truncation', async () => {
    sseFetch(
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"console."}\n\n',
    )
    await expect(
      collectModelStream(new ChatGptCodexProvider('tok', 'acct').modelStream({ context: [] })),
    ).rejects.toThrow('incomplete stream: chatgpt-codex ended without a terminal event')
    await expect(
      collectModelStream(new ChatGptCodexProvider('tok', 'acct').modelStream({ context: [] })),
    ).rejects.toBeInstanceOf(IncompleteStreamError)
  })
})

describe('OpenAIProvider native surface', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('keeps withResponsesApi and withResponsesFallback independent', () => {
    const provider = new OpenAIProvider('k', 'gpt-5.5-codex')
    expect(provider.capabilities().supportsFreeformTools).toBe(false)
    provider.withResponsesFallback(true)
    expect(provider.capabilities().supportsFreeformTools).toBe(false)
    provider.withResponsesApi(true)
    expect(provider.capabilities()).toEqual({
      supportsImage: true,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: true,
    })
    provider.withResponsesApi(false)
    expect(provider.capabilities().supportsFreeformTools).toBe(false)
  })

  it('POSTs the Responses endpoint and decodes a freeform call (non-streaming)', async () => {
    const raw = 'const x = {a: 1};\nconsole.log(x.a);\n'
    let sent: Record<string, any> = {}
    const mockFetch = jsonFetch(
      {
        model: 'gpt-5.5-codex',
        status: 'completed',
        output: [{ type: 'custom_tool_call', call_id: 'call_js', name: 'exec', input: raw }],
        usage: { input_tokens: 9, output_tokens: 7 },
      },
      (_url, init) => {
        sent = JSON.parse(String(init?.body))
      },
    )

    const response = await new OpenAIProvider('test-key', 'gpt-5.5-codex')
      .withResponsesApi(true)
      .modelChat({
        context: [{ kind: 'message', message: { role: 'user', content: 'run js' } }],
        toolSpecs: [{ kind: 'freeform', tool: FREEFORM_TOOL }],
      })

    expect(String(mockFetch.mock.calls[0][0])).toBe(DEFAULT_OPENAI_RESPONSES_URL)
    expect(sent.stream).toBeUndefined()
    expect(sent.tools[0]).toEqual({
      type: 'custom',
      name: 'exec',
      description: 'Run JavaScript',
      format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
    })
    expect(response.toolCalls).toEqual([
      { kind: 'freeform', id: 'call_js', name: 'exec', input: raw },
    ])
    expect(response.stopReason).toBe('tool_use')
    expect(response.usage.inputTokens).toBe(9)
  })

  it('encodes image content blocks as input_image data URLs', async () => {
    let sent: Record<string, any> = {}
    jsonFetch(
      {
        model: 'gpt-5.5-codex',
        status: 'completed',
        output: [{ type: 'message', content: [{ type: 'output_text', text: 'ok' }] }],
        usage: { input_tokens: 1, output_tokens: 1 },
      },
      (_url, init) => {
        sent = JSON.parse(String(init?.body))
      },
    )

    const response = await new OpenAIProvider('test-key', 'gpt-5.5-codex')
      .withResponsesApi(true)
      .modelChat({
        context: [
          {
            kind: 'message',
            message: {
              role: 'user',
              content: 'inspect',
              contentBlocks: [
                { type: 'text', text: 'inspect' },
                {
                  type: 'image',
                  source: { type: 'base64', mediaType: 'image/png', data: 'abc123' },
                },
              ],
            },
          },
        ],
      })

    expect(sent.input[0].content).toEqual([
      { type: 'input_text', text: 'inspect' },
      { type: 'input_image', image_url: 'data:image/png;base64,abc123' },
    ])
    expect(response.content).toBe('ok')
  })

  it('rejects native freeform BEFORE any HTTP call when the opt-in is off', async () => {
    const mockFetch = jsonFetch({})
    const provider = new OpenAIProvider('test-key', 'gpt-5.5-codex')
    const request: ModelChatRequest = {
      context: [{ kind: 'message', message: { role: 'user', content: 'run js' } }],
      toolSpecs: [{ kind: 'freeform', tool: FREEFORM_TOOL }],
    }

    await expect(provider.modelChat(request)).rejects.toThrow(
      'provider does not support native freeform tools',
    )
    expect(() => provider.modelStream(request)).toThrow(
      'provider does not support native freeform tools',
    )
    expect(mockFetch).not.toHaveBeenCalled()
  })

  it('reports the endpoint message for a non-freeform request with the opt-in off', async () => {
    const mockFetch = jsonFetch({})
    const provider = new OpenAIProvider('test-key', 'gpt-5.5-codex')
    await expect(provider.modelChat({ context: [] })).rejects.toThrow(
      'OpenAI Chat Completions does not support native model requests; enable OpenAI Responses API',
    )
    expect(() => provider.modelStream({ context: [] })).toThrow(
      'OpenAI Chat Completions does not support native model streams; enable OpenAI Responses API',
    )
    expect(mockFetch).not.toHaveBeenCalled()
  })

  it('streams native custom deltas and collects them', async () => {
    const sse =
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"console."}\n\n' +
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"log(1);\\n"}\n\n' +
      'data: {"type":"response.output_item.done","item":{"type":"custom_tool_call","call_id":"call_js","name":"exec","input":"console.log(1);\\n"}}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":2,"output_tokens":3}}}\n\n'
    sseFetch(sse)

    const response = await collectModelStream(
      new OpenAIProvider('test-key', 'gpt-5.5-codex').withResponsesApi(true).modelStream({
        context: [{ kind: 'message', message: { role: 'user', content: 'run js' } }],
        toolSpecs: [{ kind: 'freeform', tool: FREEFORM_TOOL }],
      }),
    )
    expect(response.toolCalls).toEqual([
      { kind: 'freeform', id: 'call_js', name: 'exec', input: 'console.log(1);\n' },
    ])
    expect(response.usage.outputTokens).toBe(3)
  })

  it('throws IncompleteStreamError with the openai payload on truncation', async () => {
    sseFetch(
      'data: {"type":"response.output_text.delta","delta":"hel"}\n\n' +
        'data: {"type":"response.output_text.delta","delta":"lo"}\n\n',
    )
    await expect(
      collectModelStream(
        new OpenAIProvider('test-key', 'gpt-5.5-codex')
          .withResponsesApi(true)
          .modelStream({ context: [] }),
      ),
    ).rejects.toThrow('incomplete stream: openai ended without a terminal event')
  })
})

describe('Client native surface', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('modelChat dispatches through the built provider', async () => {
    jsonFetch({
      model: 'gpt-5.5-codex',
      status: 'completed',
      output: [{ type: 'message', content: [{ type: 'output_text', text: 'ok' }] }],
      usage: { input_tokens: 1, output_tokens: 1 },
    })

    const client = Client.builder()
      .provider('openai')
      .apiKey('test-key')
      .model('gpt-5.5-codex')
      .openaiResponsesApi(true)
      .build()

    const response = await client.modelChat({
      context: [{ kind: 'message', message: { role: 'user', content: 'hi' } }],
    })
    expect(response.content).toBe('ok')
  })

  it('openaiResponsesApi(true) flips the built provider capabilities', () => {
    const provider = new ClientBuilder()
      .provider('openai')
      .apiKey('k')
      .openaiResponsesApi(true)
      .buildProviderForTest()
    expect(provider.capabilities().supportsFreeformTools).toBe(true)
  })

  it('defaults the responses opt-in to off, independent of the fallback flag', () => {
    const plain = new ClientBuilder().provider('openai').apiKey('k').buildProviderForTest()
    expect(plain.capabilities().supportsFreeformTools).toBe(false)

    const fallbackOnly = new ClientBuilder()
      .provider('openai')
      .apiKey('k')
      .openaiResponsesFallback(true)
      .buildProviderForTest()
    expect(fallbackOnly.capabilities().supportsFreeformTools).toBe(false)
  })

  it('modelStream rejects synchronously before any HTTP when unsupported', () => {
    const mockFetch = jsonFetch({})
    const client = Client.builder().provider('openai').apiKey('test-key').build()
    expect(() =>
      client.modelStream({
        context: [],
        toolSpecs: [{ kind: 'freeform', tool: FREEFORM_TOOL }],
      }),
    ).toThrow('provider does not support native freeform tools')
    expect(mockFetch).not.toHaveBeenCalled()
  })

  it('modelStreamCollect backfills the model from the request', async () => {
    sseFetch(
      'data: {"type":"response.output_text.delta","delta":"hi"}\n\n' +
        'data: {"type":"response.completed","response":{"status":"completed"}}\n\n',
    )
    const client = Client.builder()
      .provider('openai')
      .apiKey('test-key')
      .openaiResponsesApi(true)
      .build()

    const response = await client.modelStreamCollect({
      context: [{ kind: 'message', message: { role: 'user', content: 'hi' } }],
      model: 'gpt-5.5-codex',
    })
    expect(response.content).toBe('hi')
    expect(response.model).toBe('gpt-5.5-codex')
  })

  it('applies the read-idle timeout to native streams', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(
            new ReadableStream<Uint8Array>({
              start(controller) {
                controller.enqueue(
                  new TextEncoder().encode(
                    'data: {"type":"response.output_text.delta","delta":"tick"}\n\n',
                  ),
                )
              },
            }),
            { status: 200, headers: { 'content-type': 'text/event-stream' } },
          ),
      ),
    )

    const client = Client.builder()
      .provider('openai')
      .apiKey('test-key')
      .openaiResponsesApi(true)
      .timeouts({ readIdleMs: 50 })
      .build()

    const seen: ModelStreamDelta[] = []
    let caught: unknown
    try {
      for await (const delta of client.modelStream({ context: [] })) seen.push(delta)
    } catch (error) {
      caught = error
    }
    expect(caught).toBeInstanceOf(StreamReadTimeoutError)
    expect(seen).toEqual([{ type: 'text', delta: 'tick' }])
  })
})

describe('asDispatchProvider forwards the native surface', () => {
  const nativeResponse: ModelChatResponse = {
    content: 'shimmed',
    toolCalls: [],
    model: 'm',
    usage: { inputTokens: 0, outputTokens: 0 },
    stopReason: 'end_turn',
  }

  it('keeps modelChat/modelStream when the provider has no capabilities()', async () => {
    const bare: ProviderLike = {
      chat: async () => {
        throw new Error('unused')
      },
      stream: () => {
        throw new Error('unused')
      },
      modelChat: async () => nativeResponse,
      modelStream: () => deltas([{ type: 'done', stopReason: 'end_turn' }]),
    }

    const client = new Client(bare)
    expect((await client.modelChat({ context: [] })).content).toBe('shimmed')

    const seen: ModelStreamDelta[] = []
    for await (const delta of client.modelStream({ context: [] })) seen.push(delta)
    expect(seen).toEqual([{ type: 'done', stopReason: 'end_turn' }])
  })

  it('still rejects when a shimmed provider omits the native surface', async () => {
    const bare: ProviderLike = {
      chat: async () => {
        throw new Error('unused')
      },
      stream: () => {
        throw new Error('unused')
      },
    }
    const client = new Client(bare)
    await expect(client.modelChat({ context: [] })).rejects.toThrow(
      'provider does not support native model requests',
    )
    expect(() => client.modelStream({ context: [] })).toThrow(
      'provider does not support native model streams',
    )
  })

  it('keeps the native surface on a provider that DOES expose capabilities()', async () => {
    const full: ProviderLike = {
      capabilities: () => withFreeformTools(),
      chat: async () => {
        throw new Error('unused')
      },
      stream: () => {
        throw new Error('unused')
      },
      modelChat: async () => nativeResponse,
    }
    const client = new Client(full)
    expect((await client.modelChat({ context: [] })).content).toBe('shimmed')
  })
})
