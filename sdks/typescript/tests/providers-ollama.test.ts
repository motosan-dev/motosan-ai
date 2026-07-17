import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { OllamaProvider } from '../src/providers/ollama.js'
import { DEFAULT_OLLAMA_MODEL } from '../src/models.js'
import { RetryPolicy } from '../src/retry.js'
import { IncompleteStreamError } from '../src/error.js'
import type { ChatRequest, StreamEvent } from '../src/types.js'

const BASE = 'http://localhost:11434'

describe('OllamaProvider chat (native /api/chat)', () => {
  let captured: { url: string; headers: Record<string, string>; body: any } | null = null

  beforeEach(() => {
    captured = null
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  function stubOk(payload: unknown) {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, options?: RequestInit) => {
        captured = {
          url,
          headers: (options?.headers as Record<string, string>) ?? {},
          body: options?.body ? JSON.parse(String(options.body)) : null,
        }
        return new Response(JSON.stringify(payload), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        })
      }),
    )
  }

  it('posts to {base}/api/chat with no auth header and flat messages', async () => {
    stubOk({
      model: 'llama3.2',
      message: { role: 'assistant', content: 'Hello!' },
      done: true,
      prompt_eval_count: 9,
      eval_count: 3,
    })
    const provider = new OllamaProvider('llama3.2', BASE)
    const req: ChatRequest = {
      system: '  you are helpful  ',
      messages: [{ role: 'user', content: 'hi' }],
    }
    const res = await provider.chat(req)

    expect(captured?.url).toBe('http://localhost:11434/api/chat')
    // no auth header on the native path
    expect(captured?.headers['authorization']).toBeUndefined()
    expect(captured?.headers['x-api-key']).toBeUndefined()
    expect(captured?.body.model).toBe('llama3.2')
    expect(captured?.body.stream).toBe(false)
    // system trimmed + first; flat {role,content}
    expect(captured?.body.messages).toEqual([
      { role: 'system', content: 'you are helpful' },
      { role: 'user', content: 'hi' },
    ])
    expect(res.content).toBe('Hello!')
    expect(res.model).toBe('llama3.2')
    expect(res.stopReason).toBe('stop')
    expect(res.usage.inputTokens).toBe(9)
    expect(res.usage.outputTokens).toBe(3)
    expect(res.thinking).toBeUndefined()
  })

  it('trims trailing slashes off the base URL', async () => {
    stubOk({ model: 'llama3.2', message: { content: 'x' }, done: true })
    const provider = new OllamaProvider('llama3.2', 'http://localhost:11434///')
    await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(captured?.url).toBe('http://localhost:11434/api/chat')
  })

  it('serializes assistant tool_calls with arguments as a parsed OBJECT (not a JSON string)', async () => {
    stubOk({ model: 'llama3.2', message: { content: 'ok' }, done: true })
    const provider = new OllamaProvider('llama3.2', BASE)
    const req: ChatRequest = {
      messages: [
        { role: 'user', content: 'weather?' },
        {
          role: 'assistant',
          content: '',
          toolCalls: [{ id: 'x', name: 'get_weather', input: { city: 'NYC' } }],
        },
        { role: 'tool', content: '{"temp":70}', toolCallId: 'x' },
      ],
    }
    await provider.chat(req)
    const msgs = captured?.body.messages
    const assistant = msgs.find((m: any) => m.role === 'assistant')
    // arguments is the parsed object directly — contrast OpenAI's JSON string
    expect(assistant.tool_calls).toEqual([
      { function: { name: 'get_weather', arguments: { city: 'NYC' } } },
    ])
    // assistant tool_calls have NO top-level id/type on the native path
    expect(assistant.tool_calls[0].id).toBeUndefined()
    expect(assistant.tool_calls[0].type).toBeUndefined()
    // tool message has NO tool_call_id on the native path
    const tool = msgs.find((m: any) => m.role === 'tool')
    expect(tool).toEqual({ role: 'tool', content: '{"temp":70}' })
  })

  it('puts num_ctx inside options and keep_alive at root; tools only when non-empty', async () => {
    stubOk({ model: 'llama3.2', message: { content: 'ok' }, done: true })
    const provider = new OllamaProvider('llama3.2', BASE)
      .withNumCtx(4096)
      .withKeepAlive('5m')
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      temperature: 0.5,
      stopSequences: ['STOP'],
      tools: [{ name: 't', description: 'd', inputSchema: { type: 'object', properties: {} } }],
    }
    await provider.chat(req)
    expect(captured?.body.keep_alive).toBe('5m')
    expect(captured?.body.options).toEqual({
      temperature: 0.5,
      num_ctx: 4096,
      stop: ['STOP'],
    })
    expect(captured?.body.tools).toEqual([
      {
        type: 'function',
        function: { name: 't', description: 'd', parameters: { type: 'object', properties: {} } },
      },
    ])
  })

  it('omits the options object entirely when empty', async () => {
    stubOk({ model: 'llama3.2', message: { content: 'ok' }, done: true })
    const provider = new OllamaProvider('llama3.2', BASE)
    await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(captured?.body.options).toBeUndefined()
    expect(captured?.body.keep_alive).toBeUndefined()
    expect(captured?.body.tools).toBeUndefined()
  })

  it('folds thinking into <think> wrapper when content is also present', async () => {
    stubOk({
      model: 'llama3.2',
      message: { content: 'answer', thinking: 'reasoning' },
      done: true,
    })
    const provider = new OllamaProvider('llama3.2', BASE)
    const res = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(res.content).toBe('<think>reasoning</think>\n\nanswer')
    // native chat() never populates ChatResponse.thinking; omit the optional key.
    expect(res.thinking).toBeUndefined()
    expect('thinking' in res).toBe(false)
  })

  it('uses thinking AS content when content is empty', async () => {
    stubOk({ model: 'llama3.2', message: { content: '', thinking: 'only-reasoning' }, done: true })
    const provider = new OllamaProvider('llama3.2', BASE)
    const res = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(res.content).toBe('only-reasoning')
  })

  it('extracts tool calls (string + object args) and reports tool_use', async () => {
    stubOk({
      model: 'llama3.2',
      message: {
        content: '',
        tool_calls: [
          { function: { name: 'a', arguments: '{"x":1}' } },
          { function: { name: 'b', arguments: { y: 2 } } },
        ],
      },
      done: true,
    })
    const provider = new OllamaProvider('llama3.2', BASE)
    const res = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(res.stopReason).toBe('tool_use')
    expect(res.toolCalls).toEqual([
      { id: 'call_0', name: 'a', input: { x: 1 } },
      { id: 'call_1', name: 'b', input: { y: 2 } },
    ])
  })

  it('keeps a raw string when tool-call string args fail to parse', async () => {
    stubOk({
      model: 'llama3.2',
      message: { content: '', tool_calls: [{ id: 'real', function: { name: 'a', arguments: 'not json' } }] },
      done: true,
    })
    const provider = new OllamaProvider('llama3.2', BASE)
    const res = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
    // Rust keeps Value::String(s) on parse failure; preserve the raw string.
    expect(res.toolCalls[0]).toEqual({ id: 'real', name: 'a', input: 'not json' })
  })

  it('falls back to DEFAULT_OLLAMA_MODEL when payload.model is missing', async () => {
    stubOk({ message: { content: 'ok' }, done: true })
    const provider = new OllamaProvider('llama3.2', BASE)
    const res = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(res.model).toBe(DEFAULT_OLLAMA_MODEL)
  })

  it('reports stop=other when done is falsy and no tool calls', async () => {
    stubOk({ model: 'llama3.2', message: { content: 'partial' }, done: false })
    const provider = new OllamaProvider('llama3.2', BASE)
    const res = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(res.stopReason).toBe('other')
  })

  it('prefers systemBlocks over system string (joined + trimmed)', async () => {
    stubOk({ model: 'llama3.2', message: { content: 'ok' }, done: true })
    const provider = new OllamaProvider('llama3.2', BASE)
    await provider.chat({
      system: 'ignored',
      systemBlocks: [{ text: ' a ' }, { text: 'b ' }],
      messages: [{ role: 'user', content: 'hi' }],
    })
    expect(captured?.body.messages[0]).toEqual({ role: 'system', content: 'a \nb' })
  })

  it('merges providerOptions into the root last', async () => {
    stubOk({ model: 'llama3.2', message: { content: 'ok' }, done: true })
    const provider = new OllamaProvider('llama3.2', BASE)
    await provider.chat({
      messages: [{ role: 'user', content: 'hi' }],
      providerOptions: { format: 'json', seed: 7 },
    })
    expect(captured?.body.format).toBe('json')
    expect(captured?.body.seed).toBe(7)
  })
})

describe('OllamaProvider think coercion (mirrors ollama.rs:564-635)', () => {
  let captured: { body: any } | null = null
  afterEach(() => vi.unstubAllGlobals())

  function bodyForThink(think: string | undefined): Promise<any> {
    captured = null
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, options?: RequestInit) => {
        captured = { body: options?.body ? JSON.parse(String(options.body)) : null }
        return new Response(JSON.stringify({ model: 'llama3.2', message: { content: 'x' }, done: true }), {
          status: 200,
        })
      }),
    )
    const provider = new OllamaProvider('llama3.2', BASE).withThink(think)
    return provider
      .chat({ messages: [{ role: 'user', content: 'hi' }] })
      .then(() => captured!.body)
  }

  it('coerces truthy synonyms to boolean true', async () => {
    for (const input of ['true', 'yes', 'on', '1', 'YES', 'True', '  yes  ']) {
      const body = await bodyForThink(input)
      expect(body.think).toBe(true)
    }
  })

  it('coerces falsy synonyms to boolean false', async () => {
    for (const input of ['false', 'no', 'off', '0', 'NO', 'False']) {
      const body = await bodyForThink(input)
      expect(body.think).toBe(false)
    }
  })

  it('passes other values through as the trimmed verbatim string', async () => {
    const body = await bodyForThink('  low  ')
    expect(body.think).toBe('low')
    const body2 = await bodyForThink('high')
    expect(body2.think).toBe('high')
  })

  it('omits think for empty / whitespace-only / unset', async () => {
    for (const input of ['', ' ', '   ', '\t', '\n', '\t  \n']) {
      const body = await bodyForThink(input)
      expect(body.think).toBeUndefined()
    }
    const unset = await bodyForThink(undefined)
    expect(unset.think).toBeUndefined()
  })
})

describe('OllamaProvider stream (native NDJSON adapter)', () => {
  afterEach(() => vi.unstubAllGlobals())

  function stubNdjson(lines: string[]) {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, options?: RequestInit) => {
        // assert stream:true is set on the streaming body
        const body = options?.body ? JSON.parse(String(options.body)) : null
        expect(body.stream).toBe(true)
        return new Response(lines.join(''), {
          status: 200,
          headers: { 'content-type': 'application/x-ndjson' },
        })
      }),
    )
  }

  it('emits text events then terminates on done:true (plain doneEvent, no stop_reason)', async () => {
    stubNdjson([
      '{"message":{"content":"Hel"},"done":false}\n',
      '{"message":{"content":"lo"},"done":false}\n',
      '{"done":true}\n',
    ])
    const provider = new OllamaProvider('llama3.2', BASE)
    const events: StreamEvent[] = []
    for await (const e of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      events.push(e)
    }
    expect(events.map((e) => ({ t: e.eventType, c: e.content, done: e.done }))).toEqual([
      { t: 'text', c: 'Hel', done: false },
      { t: 'text', c: 'lo', done: false },
      { t: 'text', c: '', done: true },
    ])
    // plain doneEvent carries NO stopReason
    expect(events[events.length - 1].stopReason).toBeUndefined()
  })

  it('surfaces streamed thinking as a plain text event', async () => {
    stubNdjson([
      '{"message":{"content":"","thinking":"pondering"},"done":false}\n',
      '{"message":{"content":"answer"},"done":false}\n',
      '{"done":true}\n',
    ])
    const provider = new OllamaProvider('llama3.2', BASE)
    const texts: string[] = []
    for await (const e of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      if (e.eventType === 'text' && !e.done) texts.push(e.content)
    }
    expect(texts).toEqual(['pondering', 'answer'])
  })

  it('emits the 3-event tool-call pattern (start, argsWithId, endWithId) with generated call_N ids', async () => {
    stubNdjson([
      '{"message":{"content":"","tool_calls":[{"function":{"name":"get_weather","arguments":{"city":"NYC"}}}]},"done":false}\n',
      '{"done":true}\n',
    ])
    const provider = new OllamaProvider('llama3.2', BASE)
    const events: StreamEvent[] = []
    for await (const e of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      events.push(e)
    }
    const tool = events.filter((e) => e.eventType.startsWith('tool_call'))
    expect(tool).toHaveLength(3)
    expect(tool[0]).toMatchObject({
      eventType: 'tool_call_start',
      toolCallId: 'call_0',
      toolCallName: 'get_weather',
    })
    expect(tool[1]).toMatchObject({
      eventType: 'tool_call_args',
      toolCallId: 'call_0',
      toolCallArgsDelta: '{"city":"NYC"}',
    })
    expect(tool[2]).toMatchObject({ eventType: 'tool_call_end', toolCallId: 'call_0' })
    expect(events[events.length - 1].done).toBe(true)
  })

  it('throws IncompleteStreamError on EOF without done:true (M3/E2)', async () => {
    stubNdjson([
      '{"message":{"content":"a"},"done":false}\n',
      '{"message":{"content":"b"},"done":false}\n',
    ])
    const provider = new OllamaProvider('llama3.2', BASE)
    const events: StreamEvent[] = []
    let error: unknown
    try {
      for await (const e of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        events.push(e)
      }
    } catch (e) {
      error = e
    }
    expect(error).toBeInstanceOf(IncompleteStreamError)
    expect((error as Error).message).toBe('incomplete stream: ollama ended without a terminal event')
    expect(events.map((e) => e.content)).toEqual(['a', 'b'])
  })

  it('propagates an NDJSON body error after yielding partial data (M3: swallow removed)', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        let reads = 0
        const body = new ReadableStream<Uint8Array>({
          pull(controller) {
            reads += 1
            if (reads === 1) {
              controller.enqueue(
                new TextEncoder().encode('{"message":{"content":"partial"},"done":false}\n'),
              )
              return
            }
            controller.error(new Error('socket closed'))
          },
        })
        return new Response(body, { status: 200 })
      }),
    )
    const provider = new OllamaProvider('llama3.2', BASE)
    const events: StreamEvent[] = []
    await expect(
      (async () => {
        for await (const e of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
          events.push(e)
        }
      })(),
    ).rejects.toThrow('socket closed')
    expect(events).toEqual([{ content: 'partial', done: false, eventType: 'text' }])
  })
})

describe('OllamaProvider capabilities + retry', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('is text-only and reports supportsMcp:false', () => {
    const caps = new OllamaProvider('llama3.2', BASE).capabilities()
    expect(caps.supportsImage).toBe(false)
    expect(caps.supportsDocument).toBe(false)
    // M4 adds supportsMcp to every factory; textOnly() must carry it.
    expect((caps as { supportsMcp: boolean }).supportsMcp).toBe(false)
  })

  it('retries a retryable 503 on chat() then succeeds', async () => {
    let calls = 0
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        calls += 1
        if (calls === 1) {
          return new Response(JSON.stringify({ error: { message: 'overloaded' } }), { status: 503 })
        }
        return new Response(JSON.stringify({ model: 'llama3.2', message: { content: 'ok' }, done: true }), {
          status: 200,
        })
      }),
    )
    const provider = new OllamaProvider('llama3.2', BASE).withRetryPolicy(
      new RetryPolicy({ maxRetries: 2, baseDelayMs: 0, maxDelayMs: 0, jitter: false }),
    )
    const res = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(calls).toBe(2)
    expect(res.content).toBe('ok')
  })

  it('does NOT retry a non-retryable 400 on chat()', async () => {
    let calls = 0
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        calls += 1
        return new Response(JSON.stringify({ error: { message: 'bad' } }), { status: 400 })
      }),
    )
    const provider = new OllamaProvider('llama3.2', BASE).withRetryPolicy(
      new RetryPolicy({ maxRetries: 3, baseDelayMs: 0, maxDelayMs: 0, jitter: false }),
    )
    await expect(provider.chat({ messages: [{ role: 'user', content: 'hi' }] })).rejects.toThrow('bad')
    expect(calls).toBe(1)
  })

  it('retries the INITIAL stream fetch on 503 then streams', async () => {
    let calls = 0
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        calls += 1
        if (calls === 1) return new Response('upstream down', { status: 503 })
        return new Response('{"message":{"content":"hi"},"done":false}\n{"done":true}\n', { status: 200 })
      }),
    )
    const provider = new OllamaProvider('llama3.2', BASE).withRetryPolicy(
      new RetryPolicy({ maxRetries: 2, baseDelayMs: 0, maxDelayMs: 0, jitter: false }),
    )
    const events: StreamEvent[] = []
    for await (const e of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      events.push(e)
    }
    expect(calls).toBe(2)
    expect(events[0].content).toBe('hi')
    expect(events[events.length - 1].done).toBe(true)
  })
})

// Env-gated live test (no mock). Requires a local Ollama with llama3.2 pulled.
const liveBase = process.env.OLLAMA_BASE_URL
describe.skipIf(!liveBase)('OllamaProvider live (env-gated)', () => {
  it('streams a real native /api/chat response', async () => {
    const provider = new OllamaProvider('llama3.2', liveBase as string)
    let text = ''
    let sawDone = false
    for await (const e of provider.stream({
      messages: [{ role: 'user', content: 'Say "ok" and nothing else.' }],
    })) {
      if (e.eventType === 'text' && !e.done) text += e.content
      if (e.done) sawDone = true
    }
    expect(sawDone).toBe(true)
    expect(text.length).toBeGreaterThan(0)
  }, 30000)
})
