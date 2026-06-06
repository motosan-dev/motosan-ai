import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { OpenAIProvider } from '../src/providers/openai.js'
import type { ChatRequest, StreamEvent } from '../src/types.js'

describe('OpenAIProvider chat', () => {
  let capturedRequest: { url: string; headers: Record<string, string>; body: any } | null = null

  beforeEach(() => {
    capturedRequest = null
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('sends correct URL + Bearer auth header and parses a text response', async () => {
    const mockFetch = vi.fn(async (url: string, options?: RequestInit) => {
      capturedRequest = {
        url,
        headers: (options?.headers as Record<string, string>) ?? {},
        body: options?.body ? JSON.parse(String(options.body)) : null,
      }
      return new Response(
        JSON.stringify({
          id: 'chatcmpl_1',
          object: 'chat.completion',
          created: 1234567890,
          model: 'gpt-4o',
          choices: [
            {
              index: 0,
              message: {
                role: 'assistant',
                content: 'Hello, world!',
              },
              finish_reason: 'stop',
            },
          ],
          usage: {
            prompt_tokens: 10,
            completion_tokens: 5,
          },
        }),
        { status: 200, headers: { 'content-type': 'application/json' } }
      )
    })
    vi.stubGlobal('fetch', mockFetch)

    const provider = new OpenAIProvider('sk-test', 'gpt-4o', 'https://api.openai.com/v1')
    const request: ChatRequest = {
      messages: [{ role: 'user', content: 'Hello' }],
    }

    const response = await provider.chat(request)

    expect(capturedRequest?.url).toBe('https://api.openai.com/v1/chat/completions')
    expect(capturedRequest?.headers['authorization']).toBe('Bearer sk-test')
    expect(capturedRequest?.headers['content-type']).toBe('application/json')
    expect(capturedRequest?.body.model).toBe('gpt-4o')
    expect(Array.isArray(capturedRequest?.body.messages)).toBe(true)
    expect(response.content).toBe('Hello, world!')
    expect(response.model).toBe('gpt-4o')
    expect(response.toolCalls).toEqual([])
    expect(response.usage.inputTokens).toBe(10)
    expect(response.usage.outputTokens).toBe(5)
    expect(response.stopReason).toBe('stop')
  })

  it('parses tool_calls with function.arguments as stringified JSON', async () => {
    const mockFetch = vi.fn(async () => {
      return new Response(
        JSON.stringify({
          id: 'chatcmpl_2',
          object: 'chat.completion',
          created: 1234567890,
          model: 'gpt-4o',
          choices: [
            {
              index: 0,
              message: {
                role: 'assistant',
                content: 'Calling a tool',
                tool_calls: [
                  {
                    id: 'call_abc123',
                    type: 'function',
                    function: {
                      name: 'calculate',
                      arguments: '{"x": 2, "y": 3}',
                    },
                  },
                ],
              },
              finish_reason: 'tool_calls',
            },
          ],
          usage: {
            prompt_tokens: 15,
            completion_tokens: 8,
          },
        }),
        { status: 200, headers: { 'content-type': 'application/json' } }
      )
    })
    vi.stubGlobal('fetch', mockFetch)

    const provider = new OpenAIProvider('sk-test', 'gpt-4o')
    const response = await provider.chat({
      messages: [{ role: 'user', content: 'Call calculate' }],
    })

    expect(response.toolCalls).toHaveLength(1)
    expect(response.toolCalls[0]).toEqual({
      id: 'call_abc123',
      name: 'calculate',
      input: { x: 2, y: 3 },
    })
    expect(response.stopReason).toBe('tool_use')
  })

  it('maps finish_reason: length to max_tokens', async () => {
    const mockFetch = vi.fn(async () => {
      return new Response(
        JSON.stringify({
          id: 'chatcmpl_3',
          model: 'gpt-4o',
          choices: [
            {
              message: { content: 'truncated' },
              finish_reason: 'length',
            },
          ],
          usage: { prompt_tokens: 5, completion_tokens: 5 },
        }),
        { status: 200 }
      )
    })
    vi.stubGlobal('fetch', mockFetch)

    const provider = new OpenAIProvider('sk-test')
    const response = await provider.chat({
      messages: [{ role: 'user', content: 'test' }],
    })

    expect(response.stopReason).toBe('max_tokens')
  })

  it('uses baseUrl constructor parameter for custom endpoints', async () => {
    const mockFetch = vi.fn(async (url: string) => {
      capturedRequest = { url, headers: {}, body: null }
      return new Response(
        JSON.stringify({
          id: 'chatcmpl_4',
          model: 'gpt-4o',
          choices: [{ message: { content: 'ok' }, finish_reason: 'stop' }],
          usage: { prompt_tokens: 1, completion_tokens: 1 },
        }),
        { status: 200 }
      )
    })
    vi.stubGlobal('fetch', mockFetch)

    const provider = new OpenAIProvider('sk-test', 'gpt-4o', 'https://api.custom.com/v1/')
    await provider.chat({ messages: [{ role: 'user', content: 'test' }] })

    expect(capturedRequest?.url).toBe('https://api.custom.com/v1/chat/completions')
  })
})

describe('OpenAIProvider stream', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('emits text events and terminal done event', async () => {
    const mockFetch = vi.fn(async () => {
      const sseData = [
        'data: {"choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}\n\n',
        'data: {"choices":[{"index":0,"delta":{"content":" world"},"finish_reason":null}]}\n\n',
        'data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}\n\n',
        'data: [DONE]\n\n',
      ].join('')

      return new Response(sseData, {
        status: 200,
        headers: { 'content-type': 'text/event-stream' },
      })
    })
    vi.stubGlobal('fetch', mockFetch)

    const provider = new OpenAIProvider('sk-test')
    const events: StreamEvent[] = []

    for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'Hi' }] })) {
      events.push(evt)
    }

    expect(events.length).toBeGreaterThanOrEqual(2)
    expect(events[0].eventType).toBe('text')
    expect(events[0].content).toBe('Hello')
    expect(events[1].eventType).toBe('text')
    expect(events[1].content).toBe(' world')
    expect(events[events.length - 1].done).toBe(true)
    expect(events[events.length - 1].stopReason).toBe('stop')
  })

  it('handles indexed tool_calls in deltas with sequential flush', async () => {
    const mockFetch = vi.fn(async () => {
      const sseData = [
        // First tool call (index 0) starts with id + name
        'data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"get_weather"}}]},"finish_reason":null}]}\n\n',
        // Arguments fragment for index 0
        'data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\\\"city"}}]},"finish_reason":null}]}\n\n',
        // More arguments for index 0
        'data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\\\":\\\"NYC\\\"}"}}]},"finish_reason":null}]}\n\n',
        // Finish with tool_calls reason
        'data: {"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}\n\n',
        'data: [DONE]\n\n',
      ].join('')

      return new Response(sseData, {
        status: 200,
        headers: { 'content-type': 'text/event-stream' },
      })
    })
    vi.stubGlobal('fetch', mockFetch)

    const provider = new OpenAIProvider('sk-test')
    const events: StreamEvent[] = []

    for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'Weather?' }] })) {
      events.push(evt)
    }

    // Should emit: tool_call_start, tool_call_args (x2), tool_call_end, done with stop_reason=tool_use
    const toolEvents = events.filter((e) => e.eventType.startsWith('tool_call'))
    expect(toolEvents.length).toBeGreaterThanOrEqual(3) // start, args (x2), end
    expect(toolEvents[0].eventType).toBe('tool_call_start')
    expect(toolEvents[0].toolCallId).toBe('call_1')
    expect(toolEvents[0].toolCallName).toBe('get_weather')

    const toolArgEvents = toolEvents.filter((e) => e.eventType === 'tool_call_args')
    expect(toolArgEvents.length).toBe(2)
    expect(toolArgEvents[0].toolCallArgsDelta).toBe('{"city')
    expect(toolArgEvents[1].toolCallArgsDelta).toBe('":"NYC"}')

    const endEvent = toolEvents[toolEvents.length - 1]
    expect(endEvent.eventType).toBe('tool_call_end')
    expect(endEvent.toolCallId).toBe('call_1')

    const doneEvent = events[events.length - 1]
    expect(doneEvent.done).toBe(true)
    expect(doneEvent.stopReason).toBe('tool_use')
  })

  it('switches between two tool indices: closes the prior tool before opening the next', async () => {
    const mockFetch = vi.fn(async () => {
      const sseData = [
        // index 0 opens (id + name)
        'data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"get_weather"}}]},"finish_reason":null}]}\n\n',
        // index 0 args
        'data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\\"city\\":\\"NYC\\"}"}}]},"finish_reason":null}]}\n\n',
        // index 1 opens -> must close index 0 (call_1) FIRST, then open call_2
        'data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"call_2","function":{"name":"get_time"}}]},"finish_reason":null}]}\n\n',
        // index 1 args
        'data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"function":{"arguments":"{\\"tz\\":\\"EST\\"}"}}]},"finish_reason":null}]}\n\n',
        // finish closes the last-open tool (call_2)
        'data: {"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}\n\n',
        'data: [DONE]\n\n',
      ].join('')

      return new Response(sseData, {
        status: 200,
        headers: { 'content-type': 'text/event-stream' },
      })
    })
    vi.stubGlobal('fetch', mockFetch)

    const provider = new OpenAIProvider('sk-test')
    const events: StreamEvent[] = []

    for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'multi' }] })) {
      events.push(evt)
    }

    const starts = events.filter((e) => e.eventType === 'tool_call_start')
    const ends = events.filter((e) => e.eventType === 'tool_call_end')
    // Two distinct tools, each opened and closed EXACTLY once.
    expect(starts.map((e) => e.toolCallId)).toEqual(['call_1', 'call_2'])
    expect(ends.map((e) => e.toolCallId)).toEqual(['call_1', 'call_2'])
    // The single-open invariant collectStream relies on: call_1 closes BEFORE call_2 opens.
    expect(events.indexOf(ends[0])).toBeLessThan(events.indexOf(starts[1]))
    // Terminal done with tool_use.
    expect(events[events.length - 1].done).toBe(true)
    expect(events[events.length - 1].stopReason).toBe('tool_use')
  })

  it('closes open tool and emits done when [DONE] arrives', async () => {
    const mockFetch = vi.fn(async () => {
      const sseData = [
        'data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_x","function":{"name":"func_x"}}]},"finish_reason":null}]}\n\n',
        'data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{}"}}]},"finish_reason":null}]}\n\n',
        'data: [DONE]\n\n',
      ].join('')

      return new Response(sseData, {
        status: 200,
        headers: { 'content-type': 'text/event-stream' },
      })
    })
    vi.stubGlobal('fetch', mockFetch)

    const provider = new OpenAIProvider('sk-test')
    const events: StreamEvent[] = []

    for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'test' }] })) {
      events.push(evt)
    }

    // Must have tool_call_start, args, and end before done
    const eventTypes = events.map((e) => e.eventType)
    const toolStartIdx = eventTypes.indexOf('tool_call_start')
    const toolEndIdx = eventTypes.indexOf('tool_call_end')

    expect(toolStartIdx).toBeGreaterThanOrEqual(0)
    expect(toolEndIdx).toBeGreaterThan(toolStartIdx)
    expect(events[events.length - 1].done).toBe(true)
  })

  it('emits usage event if present in final chunk', async () => {
    const mockFetch = vi.fn(async () => {
      const sseData = [
        'data: {"choices":[{"index":0,"delta":{"content":"test"},"finish_reason":null}]}\n\n',
        'data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":3}}\n\n',
        'data: [DONE]\n\n',
      ].join('')

      return new Response(sseData, {
        status: 200,
        headers: { 'content-type': 'text/event-stream' },
      })
    })
    vi.stubGlobal('fetch', mockFetch)

    const provider = new OpenAIProvider('sk-test')
    const events: StreamEvent[] = []

    for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'test' }] })) {
      events.push(evt)
    }

    const usageEvent = events.find((e) => e.eventType === 'usage')
    expect(usageEvent).toBeDefined()
    expect(usageEvent?.usage?.inputTokens).toBe(5)
    expect(usageEvent?.usage?.outputTokens).toBe(3)
  })
})

describe('OpenAIProvider (live integration)', () => {
  it.skipIf(!process.env.OPENAI_API_KEY)(
    'performs a real chat.completions request',
    async () => {
      const provider = new OpenAIProvider(process.env.OPENAI_API_KEY!, 'gpt-4o')
      const response = await provider.chat({
        messages: [{ role: 'user', content: 'Say "live test ok"' }],
        maxTokens: 10,
      })

      expect(response.content).toBeTruthy()
      expect(response.model).toBe('gpt-4o')
      expect(response.usage.inputTokens).toBeGreaterThan(0)
      expect(response.usage.outputTokens).toBeGreaterThan(0)
    }
  )

  it.skipIf(!process.env.OPENAI_API_KEY)(
    'streams a real response',
    async () => {
      const provider = new OpenAIProvider(process.env.OPENAI_API_KEY!, 'gpt-4o')
      let textAccum = ''
      let gotStream = false

      for await (const evt of provider.stream({
        messages: [{ role: 'user', content: 'Say "streaming ok" in one short word' }],
        maxTokens: 5,
      })) {
        if (evt.eventType === 'text' && evt.content) {
          textAccum += evt.content
          gotStream = true
        }
      }

      expect(gotStream).toBe(true)
      expect(textAccum.length).toBeGreaterThan(0)
    }
  )
})
