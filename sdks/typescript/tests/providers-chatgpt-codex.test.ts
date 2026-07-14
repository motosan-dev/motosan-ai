import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  ChatGptCodexProvider,
  DEFAULT_CHATGPT_CODEX_MODEL,
  chatGptCodexErrorMessage,
} from '../src/providers/chatgpt_codex.js'
import { AuthError, NetworkError, ProviderError, RateLimitError, StreamError } from '../src/error.js'
import { RetryPolicy } from '../src/retry.js'
import type { ChatRequest, StreamEvent } from '../src/types.js'

function p(): ChatGptCodexProvider {
  return new ChatGptCodexProvider('tok', 'acct')
}

describe('ChatGptCodexProvider buildResponsesBody', () => {
  it('uses default model gpt-5.5 and default base URL', () => {
    const prov = p()
    const body = prov.buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, prov.modelId())
    expect(DEFAULT_CHATGPT_CODEX_MODEL).toBe('gpt-5.5')
    expect(body.model).toBe('gpt-5.5')
    expect(prov.endpointUrl()).toBe('https://chatgpt.com/backend-api/codex/responses')
  })

  it('per-request model overrides the default', () => {
    const prov = p()
    const req: ChatRequest = { messages: [{ role: 'user', content: 'hi' }], model: 'gpt-x' }
    const body = prov.buildResponsesBody(req, req.model ?? prov.modelId())
    expect(body.model).toBe('gpt-x')
  })

  it('sets the required codex fields and a single input_text user item', () => {
    const body = p().buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, 'gpt-5.5')
    expect(body.store).toBe(false)
    expect(body.stream).toBe(true)
    expect(typeof body.instructions).toBe('string')
    expect(Array.isArray(body.input)).toBe(true)
    expect(body.include).toEqual(['reasoning.encrypted_content'])
    expect(body.tool_choice).toBe('auto')
    expect(body.parallel_tool_calls).toBe(true)
    expect(body.input).toHaveLength(1)
    expect(body.input[0]).toEqual({
      type: 'message',
      role: 'user',
      content: [{ type: 'input_text', text: 'hi' }],
    })
    expect(body.tools).toBeUndefined()
    expect(body.reasoning).toBeUndefined()
    expect(body.temperature).toBeUndefined()
  })

  it('falls back to the default instructions when nothing is supplied', () => {
    const body = p().buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, 'gpt-5.5')
    expect(body.instructions).toBe('You are a helpful assistant.')
  })

  it('routes a system message to instructions, not input', () => {
    const req: ChatRequest = {
      messages: [
        { role: 'system', content: 'be terse' },
        { role: 'user', content: 'hi' },
      ],
    }
    const body = p().buildResponsesBody(req, 'gpt-5.5')
    expect(body.instructions).toBe('be terse')
    expect(body.input).toHaveLength(1)
    expect(body.input[0].role).toBe('user')
  })

  it('uses the system field for instructions', () => {
    const body = p().buildResponsesBody(
      { messages: [{ role: 'user', content: 'hi' }], system: 'sys here' },
      'gpt-5.5',
    )
    expect(body.instructions).toBe('sys here')
  })

  it('prefers systemBlocks over system, joined with \\n\\n', () => {
    const body = p().buildResponsesBody(
      {
        messages: [{ role: 'user', content: 'hi' }],
        system: 'ignored',
        systemBlocks: [{ text: 'a' }, { text: '  ' }, { text: 'b' }],
      },
      'gpt-5.5',
    )
    expect(body.instructions).toBe('a\n\nb')
  })

  it('emits an output_text item for assistant text', () => {
    const body = p().buildResponsesBody(
      { messages: [{ role: 'assistant', content: 'prior answer' }] },
      'gpt-5.5',
    )
    expect(body.input[0]).toEqual({
      type: 'message',
      role: 'assistant',
      content: [{ type: 'output_text', text: 'prior answer' }],
    })
  })

  it('emits function_call + function_call_output for a tool round trip', () => {
    const req: ChatRequest = {
      messages: [
        {
          role: 'assistant',
          content: '',
          toolCalls: [{ id: 'call_1', name: 'get_weather', input: { city: 'Paris' } }],
        },
        { role: 'tool', content: '{"temp":20}', toolCallId: 'call_1' },
      ],
    }
    const body = p().buildResponsesBody(req, 'gpt-5.5')
    expect(body.input[0]).toEqual({
      type: 'function_call',
      call_id: 'call_1',
      name: 'get_weather',
      arguments: '{"city":"Paris"}',
    })
    expect(body.input[1]).toEqual({
      type: 'function_call_output',
      call_id: 'call_1',
      output: '{"temp":20}',
    })
  })

  it('maps tools to the flat Responses shape with strict:null', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      tools: [{ name: 'get_weather', description: 'gets weather', inputSchema: { type: 'object' } }],
    }
    const body = p().buildResponsesBody(req, 'gpt-5.5')
    expect(body.tools).toEqual([
      {
        type: 'function',
        name: 'get_weather',
        description: 'gets weather',
        parameters: { type: 'object' },
        strict: null,
      },
    ])
  })

  it('omits the tools key for an empty tools list', () => {
    const body = p().buildResponsesBody(
      { messages: [{ role: 'user', content: 'hi' }], tools: [] },
      'gpt-5.5',
    )
    expect(body.tools).toBeUndefined()
  })

  it('emits reasoning and temperature when supplied', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      temperature: 0.3,
      providerOptions: { reasoning_effort: 'high' },
    }
    const body = p().buildResponsesBody(req, 'gpt-5.5')
    expect(body.reasoning).toEqual({ effort: 'high', summary: 'auto' })
    expect(body.temperature).toBe(0.3)
  })

  it('omits reasoning when the per-request effort is not a string', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      providerOptions: { reasoning_effort: 5 },
    }
    const body = p().buildResponsesBody(req, 'gpt-5.5')
    expect(body.reasoning).toBeUndefined()
  })

  it('emits the provider-default reasoning effort, overridden by a per-request value', () => {
    const def = p().reasoningEffort('medium')
    expect(def.buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, 'gpt-5.5').reasoning)
      .toEqual({ effort: 'medium', summary: 'auto' })
    // per-request wins
    const body = def.buildResponsesBody(
      { messages: [{ role: 'user', content: 'hi' }], providerOptions: { reasoning_effort: 'high' } },
      'gpt-5.5',
    )
    expect(body.reasoning).toEqual({ effort: 'high', summary: 'auto' })
  })

  it('reasoningEffort(undefined) clears the default and the setter returns this', () => {
    const prov = p()
    expect(prov.reasoningEffort('high')).toBe(prov) // returns this
    prov.reasoningEffort(undefined)
    expect(prov.buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, 'gpt-5.5').reasoning)
      .toBeUndefined()
  })
})

// ---------------------------------------------------------------------------
// T2 — SSE adapter (text / thinking / tool triplet / usage / terminal / error)
// ---------------------------------------------------------------------------

function streamFromTranscript(
  sse: string,
  onRequest?: (url: string, options?: RequestInit) => void,
): void {
  const bytes = new TextEncoder().encode(sse)
  const mockStream = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(bytes)
      controller.close()
    },
  })
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, options?: RequestInit) => {
      onRequest?.(url, options)
      return new Response(mockStream, {
        status: 200,
        headers: { 'content-type': 'text/event-stream' },
      })
    }),
  )
}

const REQ: ChatRequest = { messages: [{ role: 'user', content: 'hi' }] }

async function collect(sse: string): Promise<StreamEvent[]> {
  streamFromTranscript(sse)
  const prov = new ChatGptCodexProvider('tok', 'acct')
  const events: StreamEvent[] = []
  for await (const e of prov.stream(REQ)) events.push(e)
  return events
}

describe('ChatGptCodexProvider SSE adapter', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('ignores empty/[DONE]/malformed/unknown frames and empty text deltas', async () => {
    const sse =
      'data: \n\n' +
      'data: [DONE]\n\n' +
      'data: {not json}\n\n' +
      'data: {"type":"response.unknown"}\n\n' +
      'data: {"type":"response.output_text.delta","delta":""}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
    const events = await collect(sse)
    // only the terminal done
    expect(events).toHaveLength(1)
    expect(events[0].done).toBe(true)
    expect(events[0].stopReason).toBe('end_turn')
  })

  it('concatenates text deltas and ends with end_turn', async () => {
    const sse =
      'data: {"type":"response.output_text.delta","delta":"Hello, "}\n\n' +
      'data: {"type":"response.output_text.delta","delta":"world"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
    const events = await collect(sse)
    // The terminal done is itself an eventType:'text' event with empty content;
    // exclude it with `!e.done` (matches providers-anthropic.test.ts:416).
    expect(events.filter((e) => e.eventType === 'text' && !e.done).map((e) => e.content)).toEqual([
      'Hello, ',
      'world',
    ])
    expect(events[events.length - 1]).toMatchObject({ done: true, stopReason: 'end_turn' })
  })

  it('emits a usage event from response.completed (cacheRead omitted when absent)', async () => {
    const sse =
      'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":12,"output_tokens":7}}}\n\n'
    const events = await collect(sse)
    const usage = events.find((e) => e.eventType === 'usage')
    expect(usage?.usage).toEqual({ inputTokens: 12, outputTokens: 7 })
    expect(usage?.usage?.cacheReadInputTokens).toBeUndefined()
  })

  it('surfaces cached_tokens as-is (not subtracted)', async () => {
    const sse =
      'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":100,"output_tokens":5,"input_tokens_details":{"cached_tokens":30}}}}\n\n'
    const events = await collect(sse)
    const usage = events.find((e) => e.eventType === 'usage')?.usage
    expect(usage?.inputTokens).toBe(100)
    expect(usage?.cacheReadInputTokens).toBe(30)
  })

  it('maps both reasoning delta types to thinking_delta', async () => {
    const sse =
      'data: {"type":"response.reasoning_text.delta","delta":"plan "}\n\n' +
      'data: {"type":"response.reasoning_summary_text.delta","delta":"ahead"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
    const events = await collect(sse)
    expect(events.filter((e) => e.eventType === 'thinking_delta').map((e) => e.content)).toEqual([
      'plan ',
      'ahead',
    ])
  })

  it('runs the function_call lifecycle and ends with tool_use', async () => {
    // Distinct ids as on the real wire: item.id "fc_001" keys the argument
    // deltas; call_id "call_001" keys start/end. Every tool event must carry
    // the call_id.
    const sse =
      'data: {"type":"response.output_item.added","item":{"type":"function_call","id":"fc_001","call_id":"call_001","name":"get_weather"}}\n\n' +
      'data: {"type":"response.function_call_arguments.delta","item_id":"fc_001","delta":"{\\"city\\":"}\n\n' +
      'data: {"type":"response.function_call_arguments.delta","item_id":"fc_001","delta":"\\"Paris\\"}"}\n\n' +
      'data: {"type":"response.output_item.done","item":{"type":"function_call","id":"fc_001","call_id":"call_001"}}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
    const events = await collect(sse)
    const tool = events.filter((e) => e.eventType.startsWith('tool_call'))
    expect(tool[0]).toMatchObject({
      eventType: 'tool_call_start',
      toolCallId: 'call_001',
      toolCallName: 'get_weather',
    })
    expect(tool[1]).toMatchObject({ eventType: 'tool_call_args', toolCallId: 'call_001' })
    expect(tool[2]).toMatchObject({ eventType: 'tool_call_args', toolCallId: 'call_001' })
    expect(tool[3]).toMatchObject({ eventType: 'tool_call_end', toolCallId: 'call_001' })
    const argText = tool
      .filter((e) => e.eventType === 'tool_call_args')
      .map((e) => e.toolCallArgsDelta)
      .join('')
    expect(argText).toBe('{"city":"Paris"}')
    expect(events[events.length - 1]).toMatchObject({ done: true, stopReason: 'tool_use' })
  })

  it('passes an unmapped item_id through on argument deltas (fallback)', async () => {
    const sse =
      'data: {"type":"response.function_call_arguments.delta","item_id":"fc_orphan","delta":"{}"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
    const events = await collect(sse)
    const args = events.filter((e) => e.eventType === 'tool_call_args')
    expect(args).toHaveLength(1)
    expect(args[0]).toMatchObject({ toolCallId: 'fc_orphan', toolCallArgsDelta: '{}' })
  })

  it('maps status:"incomplete" to max_tokens', async () => {
    const sse = 'data: {"type":"response.completed","response":{"status":"incomplete"}}\n\n'
    const events = await collect(sse)
    expect(events[events.length - 1]).toMatchObject({ done: true, stopReason: 'max_tokens' })
  })

  it('rejects with a StreamError on a top-level error frame (partial text still emitted)', async () => {
    streamFromTranscript(
      'data: {"type":"response.output_text.delta","delta":"partial"}\n\n' +
        'data: {"type":"error","message":"rate limited"}\n\n',
    )
    const prov = new ChatGptCodexProvider('tok', 'acct')
    const events: StreamEvent[] = []
    let error: unknown
    try {
      for await (const e of prov.stream(REQ)) events.push(e)
    } catch (e) {
      error = e
    }
    expect(error).toBeInstanceOf(StreamError)
    expect((error as Error).message).toBe('rate limited')
    // the partial text was yielded before the throw; NO terminal done
    expect(events).toEqual([{ content: 'partial', done: false, eventType: 'text' }])
  })

  it('rejects with the helper-formatted message on a response.failed frame', async () => {
    streamFromTranscript(
      'data: {"type":"response.failed","response":{"error":{"message":"boom"}}}\n\n',
    )
    const prov = new ChatGptCodexProvider('tok', 'acct')
    const events: StreamEvent[] = []
    let error: unknown
    try {
      for await (const e of prov.stream(REQ)) events.push(e)
    } catch (e) {
      error = e
    }
    expect(error).toBeInstanceOf(StreamError)
    expect((error as Error).message).toBe('boom')
    expect(events).toHaveLength(0)
  })
})

describe('chatGptCodexErrorMessage', () => {
  it('prefers the top-level message', () => {
    expect(chatGptCodexErrorMessage({ type: 'error', message: 'rate limited' })).toBe('rate limited')
  })
  it('reads the nested response.error.message', () => {
    expect(
      chatGptCodexErrorMessage({ type: 'response.failed', response: { error: { message: 'boom' } } }),
    ).toBe('boom')
  })
  it('reads the error.message branch', () => {
    expect(chatGptCodexErrorMessage({ type: 'error', error: { message: 'nope' } })).toBe('nope')
  })
  it('falls back when no message is present', () => {
    expect(chatGptCodexErrorMessage({ type: 'error' })).toBe('ChatGPT-backend stream error')
  })
})

// ---------------------------------------------------------------------------
// T3 — Provider HTTP behavior (headers/body, chat(), capabilities, errors)
// ---------------------------------------------------------------------------

describe('ChatGptCodexProvider HTTP', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('sends the six codex headers and the responses body', async () => {
    let url = ''
    let headers: Record<string, string> = {}
    let body: any = null
    streamFromTranscript(
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n',
      (u, opts) => {
        url = u
        headers = (opts?.headers as Record<string, string>) ?? {}
        body = JSON.parse(String(opts?.body ?? '{}'))
      },
    )
    const prov = new ChatGptCodexProvider('my-token', 'acct-123')
    for await (const _ of prov.stream(REQ)) {
      /* drain */
    }

    expect(url).toBe('https://chatgpt.com/backend-api/codex/responses')
    expect(headers.authorization).toBe('Bearer my-token')
    expect(headers['chatgpt-account-id']).toBe('acct-123')
    expect(headers.originator).toBe('codex_cli_rs')
    expect(headers['openai-beta']).toBe('responses=experimental')
    expect(headers.accept).toBe('text/event-stream')
    expect(body.store).toBe(false)
    expect(body.stream).toBe(true)
    expect(body.include).toEqual(['reasoning.encrypted_content'])
    expect(body.input[0].type).toBe('message')
    expect(body.input[0].content[0].type).toBe('input_text')
  })

  it('has text-only capabilities', () => {
    expect(new ChatGptCodexProvider('t', 'a').capabilities()).toEqual({
      supportsImage: false,
      supportsDocument: false,
      supportsMcp: false,
    })
  })

  it('chat() collects the stream into a ChatResponse', async () => {
    streamFromTranscript(
      'data: {"type":"response.output_text.delta","delta":"Hi there"}\n\n' +
        'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":4,"output_tokens":2}}}\n\n',
    )
    const resp = await new ChatGptCodexProvider('t', 'a').chat(REQ)
    expect(resp.content).toBe('Hi there')
    expect(resp.usage).toEqual({ inputTokens: 4, outputTokens: 2 })
    expect(resp.model).toBe('gpt-5.5')
    expect(resp.stopReason).toBe('end_turn')
  })

  it('chat() honors the per-request model in the result', async () => {
    streamFromTranscript('data: {"type":"response.completed","response":{"status":"completed"}}\n\n')
    const resp = await new ChatGptCodexProvider('t', 'a').chat({ ...REQ, model: 'gpt-x' })
    expect(resp.model).toBe('gpt-x')
  })

  it('chat() surfaces thinking', async () => {
    streamFromTranscript(
      'data: {"type":"response.reasoning_text.delta","delta":"plan "}\n\n' +
        'data: {"type":"response.reasoning_summary_text.delta","delta":"ahead"}\n\n' +
        'data: {"type":"response.completed","response":{"status":"completed"}}\n\n',
    )
    const resp = await new ChatGptCodexProvider('t', 'a').chat(REQ)
    expect(resp.thinking).toBe('plan ahead')
  })

  it('chat() yields a tool call from the lifecycle', async () => {
    streamFromTranscript(
      'data: {"type":"response.output_item.added","item":{"type":"function_call","id":"fc_001","call_id":"call_001","name":"lookup"}}\n\n' +
        'data: {"type":"response.function_call_arguments.delta","item_id":"fc_001","delta":"{\\"q\\":\\"x\\"}"}\n\n' +
        'data: {"type":"response.output_item.done","item":{"type":"function_call","id":"fc_001","call_id":"call_001"}}\n\n' +
        'data: {"type":"response.completed","response":{"status":"completed"}}\n\n',
    )
    const resp = await new ChatGptCodexProvider('t', 'a').chat(REQ)
    expect(resp.stopReason).toBe('tool_use')
    expect(resp.toolCalls).toHaveLength(1)
    expect(resp.toolCalls[0]).toMatchObject({ id: 'call_001', name: 'lookup', input: { q: 'x' } })
  })

  function stubError(status: number): void {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(JSON.stringify({ error: { message: `err ${status}` } }), {
            status,
            headers: { 'content-type': 'application/json' },
          }),
      ),
    )
  }

  const noRetry = new RetryPolicy({ maxRetries: 0 })

  it('maps 401 to AuthError', async () => {
    stubError(401)
    const prov = new ChatGptCodexProvider('t', 'a').withRetryPolicy(noRetry)
    await expect(
      (async () => {
        for await (const _ of prov.stream(REQ)) {
          /* drain */
        }
      })(),
    ).rejects.toBeInstanceOf(AuthError)
  })

  it('maps 429 to RateLimitError', async () => {
    stubError(429)
    const prov = new ChatGptCodexProvider('t', 'a').withRetryPolicy(noRetry)
    await expect(
      (async () => {
        for await (const _ of prov.stream(REQ)) {
          /* drain */
        }
      })(),
    ).rejects.toBeInstanceOf(RateLimitError)
  })

  it('maps 500 to ProviderError', async () => {
    stubError(500)
    const prov = new ChatGptCodexProvider('t', 'a').withRetryPolicy(noRetry)
    await expect(
      (async () => {
        for await (const _ of prov.stream(REQ)) {
          /* drain */
        }
      })(),
    ).rejects.toBeInstanceOf(ProviderError)
  })

  it('propagates a transport error', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new NetworkError('socket hangup')
      }),
    )
    const prov = new ChatGptCodexProvider('t', 'a').withRetryPolicy(noRetry)
    await expect(
      (async () => {
        for await (const _ of prov.stream(REQ)) {
          /* drain */
        }
      })(),
    ).rejects.toBeInstanceOf(NetworkError)
  })
})
