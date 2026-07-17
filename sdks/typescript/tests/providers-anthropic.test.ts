import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { IncompleteStreamError, StreamError } from '../src/error.js'
import { AnthropicProvider } from '../src/providers/anthropic.js'
import type { ChatRequest, StreamEvent } from '../src/types.js'

describe('AnthropicProvider chat', () => {
  let capturedRequest: { url: string; headers: Record<string, string>; body: any } | null = null

  beforeEach(() => {
    capturedRequest = null
    const mockFetch = vi.fn(async (url: string, options?: RequestInit) => {
      capturedRequest = {
        url,
        headers: (options?.headers as Record<string, string>) ?? {},
        body: options?.body ? JSON.parse(String(options.body)) : null,
      }
      return new Response(
        JSON.stringify({
          id: 'msg_1',
          type: 'message',
          role: 'assistant',
          content: [{ type: 'text', text: 'Hello, world!' }],
          model: 'claude-3-5-sonnet-20241022',
          stop_reason: 'end_turn',
          stop_sequence: null,
          usage: {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
          },
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      )
    })
    vi.stubGlobal('fetch', mockFetch)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('sends correct URL + headers and parses a text response', async () => {
    const provider = new AnthropicProvider(
      'test-api-key',
      'claude-3-5-sonnet-20241022',
      'https://api.anthropic.com',
    )
    const request: ChatRequest = {
      messages: [{ role: 'user', content: 'Hello' }],
      maxTokens: 100,
    }

    const response = await provider.chat(request)

    expect(capturedRequest?.url).toBe('https://api.anthropic.com/v1/messages')
    expect(capturedRequest?.headers['x-api-key']).toBe('test-api-key')
    expect(capturedRequest?.headers['anthropic-version']).toBe('2023-06-01')
    expect(capturedRequest?.headers['content-type']).toBe('application/json')
    expect(capturedRequest?.body.model).toBe('claude-3-5-sonnet-20241022')
    expect(capturedRequest?.body.max_tokens).toBe(100)
    expect(capturedRequest?.body.stream).toBeUndefined()

    expect(response.content).toBe('Hello, world!')
    expect(response.model).toBe('claude-3-5-sonnet-20241022')
    expect(response.toolCalls).toEqual([])
    expect(response.usage.inputTokens).toBe(10)
    expect(response.usage.outputTokens).toBe(5)
    expect(response.stopReason).toBe('end_turn')
  })

  it('uses bearer auth and OAuth beta headers for setup tokens', async () => {
    const provider = new AnthropicProvider(
      'sk-ant-oat01-test-token',
      'claude-3-5-sonnet-20241022',
      'https://api.anthropic.com',
    )

    await provider.chat({ messages: [{ role: 'user', content: 'Hello' }] })

    expect(capturedRequest?.headers['x-api-key']).toBeUndefined()
    expect(capturedRequest?.headers.authorization).toBe('Bearer sk-ant-oat01-test-token')
    expect(capturedRequest?.headers['anthropic-beta']).toContain('claude-code-20250219')
    expect(capturedRequest?.headers['anthropic-beta']).toContain('oauth-2025-04-20')
    expect(capturedRequest?.headers['anthropic-beta']).toContain(
      'fine-grained-tool-streaming-2025-05-14',
    )
    expect(capturedRequest?.headers['user-agent']).toBe('claude-code/1.0.33')
    expect(capturedRequest?.headers['x-app']).toBe('cli')
    expect(capturedRequest?.body.system).toEqual([
      {
        type: 'text',
        text: "You are Claude Code, Anthropic's official CLI for Claude.",
        cache_control: { type: 'ephemeral' },
      },
    ])
  })

  it('preserves the user system prompt after OAuth Claude Code identity', async () => {
    const provider = new AnthropicProvider(
      'sk-ant-oat01-test-token',
      'claude-3-5-sonnet-20241022',
      'https://api.anthropic.com',
    )

    await provider.chat({
      messages: [{ role: 'user', content: 'Hello' }],
      system: 'Reply tersely.',
    })

    expect(capturedRequest?.body.system).toEqual([
      {
        type: 'text',
        text: "You are Claude Code, Anthropic's official CLI for Claude.",
        cache_control: { type: 'ephemeral' },
      },
      { type: 'text', text: 'Reply tersely.' },
    ])
  })

  it('parses cache tokens from a non-streaming usage block', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(
          JSON.stringify({
            id: 'msg_1',
            type: 'message',
            role: 'assistant',
            content: [{ type: 'text', text: 'cached' }],
            model: 'claude-3-5-sonnet-20241022',
            stop_reason: 'end_turn',
            usage: {
              input_tokens: 10,
              output_tokens: 5,
              cache_creation_input_tokens: 100,
              cache_read_input_tokens: 200,
            },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      ),
    )

    const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
    const response = await provider.chat({ messages: [{ role: 'user', content: 'test' }] })

    expect(response.usage.cacheCreationInputTokens).toBe(100)
    expect(response.usage.cacheReadInputTokens).toBe(200)
  })

  it('parses tool_use blocks into toolCalls', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(
          JSON.stringify({
            id: 'msg_1',
            type: 'message',
            role: 'assistant',
            content: [
              { type: 'text', text: 'Let me check' },
              { type: 'tool_use', id: 'tool_1', name: 'get_weather', input: { city: 'NYC' } },
            ],
            model: 'claude-3-5-sonnet-20241022',
            stop_reason: 'tool_use',
            usage: { input_tokens: 10, output_tokens: 5 },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      ),
    )

    const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
    const response = await provider.chat({ messages: [{ role: 'user', content: 'weather' }] })

    expect(response.content).toBe('Let me check')
    expect(response.toolCalls).toHaveLength(1)
    expect(response.toolCalls[0]).toEqual({
      id: 'tool_1',
      name: 'get_weather',
      input: { city: 'NYC' },
    })
    expect(response.stopReason).toBe('tool_use')
  })

  it('extracts thinking content when present', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(
          JSON.stringify({
            id: 'msg_1',
            type: 'message',
            role: 'assistant',
            content: [
              { type: 'thinking', thinking: 'Let me think about this...' },
              { type: 'text', text: 'The answer is 42' },
            ],
            model: 'claude-3-5-sonnet-20241022',
            stop_reason: 'end_turn',
            usage: { input_tokens: 10, output_tokens: 5 },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      ),
    )

    const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
    const response = await provider.chat({ messages: [{ role: 'user', content: 'test' }] })

    expect(response.thinking).toBe('Let me think about this...')
    expect(response.content).toBe('The answer is 42')
  })

  it('capabilities() reports image + document support', () => {
    const provider = new AnthropicProvider('key')
    expect(provider.capabilities()).toEqual({
      supportsImage: true,
      supportsDocument: true,
      supportsMcp: true,
    })
  })
})

describe('AnthropicProvider stream', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

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

  it('uses bearer auth and OAuth beta headers for setup token streams', async () => {
    let capturedHeaders: Record<string, string> = {}
    let capturedBody: any = null
    streamFromTranscript(
      'event: message_stop\ndata: {"type":"message_stop"}\n\n',
      (_url, options) => {
        capturedHeaders = (options?.headers as Record<string, string>) ?? {}
        capturedBody = JSON.parse(String(options?.body ?? '{}'))
      },
    )

    const provider = new AnthropicProvider(
      'sk-ant-oat01-stream-token',
      'claude-3-5-sonnet-20241022',
    )
    const events: StreamEvent[] = []
    for await (const event of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      events.push(event)
    }

    expect(events.some((event) => event.done)).toBe(true)
    expect(capturedHeaders['x-api-key']).toBeUndefined()
    expect(capturedHeaders.authorization).toBe('Bearer sk-ant-oat01-stream-token')
    expect(capturedHeaders['anthropic-beta']).toContain('oauth-2025-04-20')
    expect(capturedHeaders['user-agent']).toBe('claude-code/1.0.33')
    expect(capturedHeaders['x-app']).toBe('cli')
    expect(capturedBody.system).toEqual([
      {
        type: 'text',
        text: "You are Claude Code, Anthropic's official CLI for Claude.",
        cache_control: { type: 'ephemeral' },
      },
    ])
  })

  it('maps a full thinking + text + tool_use + usage transcript to StreamEvents', async () => {
    const sse =
      'event: message_start\n' +
      'data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"model":"claude-3-5-sonnet-20241022","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":100,"output_tokens":0,"cache_creation_input_tokens":50,"cache_read_input_tokens":0}}}\n\n' +
      'event: content_block_start\n' +
      'data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me "}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"think..."}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig..."}}\n\n' +
      'event: content_block_stop\n' +
      'data: {"type":"content_block_stop","index":0}\n\n' +
      'event: content_block_start\n' +
      'data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"The "}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"answer"}}\n\n' +
      'event: content_block_stop\n' +
      'data: {"type":"content_block_stop","index":1}\n\n' +
      'event: content_block_start\n' +
      'data: {"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"tool_xyz","name":"calculator","input":{}}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{"}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"\\"x\\": 2"}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"}"}}\n\n' +
      'event: content_block_stop\n' +
      'data: {"type":"content_block_stop","index":2}\n\n' +
      'event: message_delta\n' +
      'data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":100,"output_tokens":20}}\n\n' +
      'event: message_stop\n' +
      'data: {"type":"message_stop"}\n\n'

    streamFromTranscript(sse)

    const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
    const events: StreamEvent[] = []
    for await (const event of provider.stream({ messages: [{ role: 'user', content: 'test' }] })) {
      events.push(event)
    }

    // Exact ordered sequence the adapter must emit.
    const labels = events.map((e) => `${e.eventType}${e.done ? ':done' : ''}`)
    expect(labels).toEqual([
      'usage',
      'thinking_delta',
      'thinking_delta',
      'thinking_done',
      'text',
      'text',
      'tool_call_start',
      'tool_call_args',
      'tool_call_args',
      'tool_call_args',
      'tool_call_end',
      'usage',
      'text:done', // doneWithStopReason yields a default-eventType ('text') event with done=true
    ])

    const usageEvents = events.filter((e) => e.eventType === 'usage')
    expect(usageEvents).toHaveLength(2)
    // message_start usage carries cache tokens
    expect(usageEvents[0].usage?.inputTokens).toBe(100)
    expect(usageEvents[0].usage?.cacheCreationInputTokens).toBe(50)
    // message_delta usage carries NO cache tokens
    expect(usageEvents[1].usage?.inputTokens).toBe(100)
    expect(usageEvents[1].usage?.outputTokens).toBe(20)
    expect(usageEvents[1].usage?.cacheCreationInputTokens).toBeUndefined()
    expect(usageEvents[1].usage?.cacheReadInputTokens).toBeUndefined()

    const thinkingDeltas = events.filter((e) => e.eventType === 'thinking_delta')
    expect(thinkingDeltas.map((e) => e.content)).toEqual(['Let me ', 'think...'])

    const thinkingDone = events.filter((e) => e.eventType === 'thinking_done')
    expect(thinkingDone).toHaveLength(1)
    expect(thinkingDone[0].content).toBe('Let me think...')

    const textEvents = events.filter((e) => e.eventType === 'text' && !e.done)
    expect(textEvents.map((e) => e.content)).toEqual(['The ', 'answer'])

    const toolStart = events.filter((e) => e.eventType === 'tool_call_start')
    expect(toolStart).toHaveLength(1)
    expect(toolStart[0].toolCallId).toBe('tool_xyz')
    expect(toolStart[0].toolCallName).toBe('calculator')

    const toolArgs = events.filter((e) => e.eventType === 'tool_call_args')
    expect(toolArgs).toHaveLength(3)
    expect(toolArgs.every((e) => e.toolCallId === 'tool_xyz')).toBe(true)
    expect(toolArgs.map((e) => e.toolCallArgsDelta).join('')).toBe('{"x": 2}')

    const toolEnd = events.filter((e) => e.eventType === 'tool_call_end')
    expect(toolEnd).toHaveLength(1)
    expect(toolEnd[0].toolCallId).toBe('tool_xyz')

    const doneEvents = events.filter((e) => e.done)
    expect(doneEvents).toHaveLength(1)
    expect(doneEvents[0].stopReason).toBe('tool_use')
  })

  it('ignores redacted_thinking and signature_delta, emits only text + done', async () => {
    const sse =
      'event: content_block_start\n' +
      'data: {"type":"content_block_start","index":0,"content_block":{"type":"redacted_thinking","data":"blob"}}\n\n' +
      'event: content_block_stop\n' +
      'data: {"type":"content_block_stop","index":0}\n\n' +
      'event: content_block_start\n' +
      'data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"answer"}}\n\n' +
      'event: content_block_stop\n' +
      'data: {"type":"content_block_stop","index":1}\n\n' +
      'event: message_stop\n' +
      'data: {"type":"message_stop"}\n\n'

    streamFromTranscript(sse)

    const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
    const events: StreamEvent[] = []
    for await (const event of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      events.push(event)
    }

    expect(events.some((e) => e.eventType === 'thinking_delta')).toBe(false)
    expect(events.some((e) => e.eventType === 'thinking_done')).toBe(false)
    expect(events.filter((e) => e.eventType === 'text' && !e.done).map((e) => e.content)).toEqual([
      'answer',
    ])
    const doneEvents = events.filter((e) => e.done)
    expect(doneEvents).toHaveLength(1)
    expect(doneEvents[0].stopReason).toBeUndefined()
  })

  it('does not emit thinking_done for an empty thinking block (start/stop, no deltas)', async () => {
    const sse =
      'event: content_block_start\n' +
      'data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}\n\n' +
      'event: content_block_stop\n' +
      'data: {"type":"content_block_stop","index":0}\n\n' +
      'event: content_block_start\n' +
      'data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"hi"}}\n\n' +
      'event: content_block_stop\n' +
      'data: {"type":"content_block_stop","index":1}\n\n' +
      'event: message_stop\n' +
      'data: {"type":"message_stop"}\n\n'

    streamFromTranscript(sse)

    const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
    const events: StreamEvent[] = []
    for await (const event of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      events.push(event)
    }

    // A thinking block that produced no deltas must NOT emit thinking_done
    // (collectStream maps the absence to thinking: undefined).
    expect(events.some((e) => e.eventType === 'thinking_done')).toBe(false)
    expect(events.some((e) => e.eventType === 'thinking_delta')).toBe(false)
    expect(events.filter((e) => e.eventType === 'text' && !e.done).map((e) => e.content)).toEqual([
      'hi',
    ])
  })

  it('rejects with a StreamError on a mid-stream error frame (deltas already emitted survive)', async () => {
    const sse =
      'event: message_start\n' +
      'data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"model":"claude-3-5-sonnet-20241022","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":3,"output_tokens":0}}}\n\n' +
      'event: content_block_start\n' +
      'data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}\n\n' +
      'event: error\n' +
      'data: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}\n\n'

    streamFromTranscript(sse)

    const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
    const events: StreamEvent[] = []
    let error: unknown
    try {
      for await (const event of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        events.push(event)
      }
    } catch (e) {
      error = e
    }

    expect(error).toBeInstanceOf(StreamError)
    expect((error as Error).message).toContain('overloaded_error')
    expect((error as Error).message).toContain('Overloaded')
    // The text delta emitted before the error frame was still yielded,
    // and NO fabricated done event followed the error.
    expect(
      events.filter((e) => e.eventType === 'text' && !e.done).map((e) => e.content),
    ).toEqual(['partial'])
    expect(events.some((e) => e.done)).toBe(false)
  })
})

describe('AnthropicProvider live', () => {
  it.skipIf(!process.env.ANTHROPIC_API_KEY)('streams from the real API', async () => {
    const provider = new AnthropicProvider(process.env.ANTHROPIC_API_KEY as string)
    const chunks: string[] = []
    let sawDone = false
    for await (const event of provider.stream({
      messages: [{ role: 'user', content: 'reply with OK' }],
      maxTokens: 32,
    })) {
      if (event.done) sawDone = true
      else if (event.eventType === 'text') chunks.push(event.content)
    }
    expect(sawDone).toBe(true)
    expect(chunks.join('').length).toBeGreaterThan(0)
  }, 60_000)
})

describe('AnthropicProvider beta headers (M4)', () => {
  let captured: { url: string; headers: Record<string, string>; body: any } | null = null

  beforeEach(() => {
    captured = null
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, options?: RequestInit) => {
        captured = {
          url,
          headers: (options?.headers as Record<string, string>) ?? {},
          body: options?.body ? JSON.parse(String(options.body)) : null,
        }
        return new Response(
          JSON.stringify({
            id: 'msg_x',
            type: 'message',
            role: 'assistant',
            content: [{ type: 'text', text: 'ok' }],
            model: 'claude-sonnet-4-6',
            stop_reason: 'end_turn',
            usage: { input_tokens: 1, output_tokens: 1 },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        )
      }),
    )
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('adds anthropic-beta: mcp-client-2025-11-20 when mcpServers present', async () => {
    const provider = new AnthropicProvider('k', 'claude-sonnet-4-6')
    await provider.chat({
      messages: [{ role: 'user', content: 'hi' }],
      mcpServers: [{ type: 'url', url: 'https://m.example/sse', name: 'm' }],
    })
    expect(captured?.headers['anthropic-beta']).toBe('mcp-client-2025-11-20')
  })

  it('adds anthropic-beta when only mcpToolConfigs present', async () => {
    const provider = new AnthropicProvider('k', 'claude-sonnet-4-6')
    await provider.chat({
      messages: [{ role: 'user', content: 'hi' }],
      mcpToolConfigs: [{ kind: 'all', mcpServerName: 'm' }],
    })
    expect(captured?.headers['anthropic-beta']).toBe('mcp-client-2025-11-20')
  })

  it('adds interleaved-thinking beta when non-adaptive thinking is present without MCP', async () => {
    const provider = new AnthropicProvider('k', 'claude-sonnet-4-6')
    await provider.chat({
      messages: [{ role: 'user', content: 'hi' }],
      thinking: { budgetTokens: 1024 },
    })
    expect(captured?.headers['anthropic-beta']).toBe('interleaved-thinking-2025-05-14')
  })

  it('comma-joins MCP and interleaved-thinking betas with no spaces', async () => {
    const provider = new AnthropicProvider('k', 'claude-sonnet-4-6')
    await provider.chat({
      messages: [{ role: 'user', content: 'hi' }],
      mcpServers: [{ type: 'url', url: 'https://m.example/sse', name: 'm' }],
      thinking: { budgetTokens: 1024 },
    })
    expect(captured?.headers['anthropic-beta']).toBe(
      'mcp-client-2025-11-20,interleaved-thinking-2025-05-14',
    )
  })

  it('omits interleaved-thinking beta for adaptive thinking without MCP', async () => {
    const provider = new AnthropicProvider('k', 'claude-opus-4-8')
    await provider.chat({
      messages: [{ role: 'user', content: 'hi' }],
      thinking: { budgetTokens: 1024 },
    })
    expect('anthropic-beta' in (captured?.headers ?? {})).toBe(false)
  })

  it('uses exactly the OAuth betas for a plain setup-token request', async () => {
    const provider = new AnthropicProvider('sk-ant-oat01-k', 'claude-sonnet-4-6')
    await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(captured?.headers['anthropic-beta']).toBe(
      'claude-code-20250219,oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14',
    )
  })

  it('merges OAuth, MCP, and interleaved-thinking betas for setup-token requests', async () => {
    const provider = new AnthropicProvider('sk-ant-oat01-k', 'claude-sonnet-4-6')
    await provider.chat({
      messages: [{ role: 'user', content: 'hi' }],
      mcpServers: [{ type: 'url', url: 'https://m.example/sse', name: 'm' }],
      thinking: { budgetTokens: 1024 },
    })
    expect(captured?.headers['anthropic-beta']).toBe(
      'mcp-client-2025-11-20,claude-code-20250219,oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14,interleaved-thinking-2025-05-14',
    )
  })

  it('omits anthropic-beta for a plain request', async () => {
    const provider = new AnthropicProvider('k', 'claude-sonnet-4-6')
    await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect('anthropic-beta' in (captured?.headers ?? {})).toBe(false)
  })

  it('streamImpl also adds the MCP beta header', async () => {
    // Return an empty SSE body so EOF is reached before a terminal event.
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, options?: RequestInit) => {
        captured = {
          url,
          headers: (options?.headers as Record<string, string>) ?? {},
          body: options?.body ? JSON.parse(String(options.body)) : null,
        }
        return new Response('', {
          status: 200,
          headers: { 'content-type': 'text/event-stream' },
        })
      }),
    )
    const provider = new AnthropicProvider('k', 'claude-sonnet-4-6')
    let error: unknown
    try {
      for await (const _ of provider.stream({
        messages: [{ role: 'user', content: 'hi' }],
        mcpServers: [{ type: 'url', url: 'https://m.example/sse', name: 'm' }],
      }))
        void _
    } catch (e) {
      error = e
    }
    expect(error).toBeInstanceOf(IncompleteStreamError)
    expect(captured?.headers['anthropic-beta']).toBe('mcp-client-2025-11-20')
  })
})
