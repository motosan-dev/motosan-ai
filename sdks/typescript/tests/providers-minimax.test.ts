import { afterEach, describe, expect, it, vi } from 'vitest'
import { MinimaxProvider } from '../src/providers/minimax.js'
import type { ChatRequest } from '../src/types.js'

describe('minimax provider serialization', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('serializes messages via serializeOpenAiRequest and produces correct body', async () => {
    let capturedBody: any
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, options?: RequestInit) => {
        capturedBody = JSON.parse(String(options?.body ?? '{}'))
        return new Response(
          JSON.stringify({
            id: 'cmpl_123',
            model: 'MiniMax-Text-01',
            choices: [
              {
                message: { role: 'assistant', content: 'ok' },
                finish_reason: 'stop'
              }
            ],
            usage: { prompt_tokens: 10, completion_tokens: 5 }
          }),
          { status: 200, headers: { 'content-type': 'application/json' } }
        )
      })
    )

    const provider = new MinimaxProvider('test-key')
    const request: ChatRequest = {
      messages: [
        { role: 'user', content: 'What is 2+2?' },
        {
          role: 'assistant',
          content: '2+2=4',
          toolCalls: [{ id: 'call_1', name: 'calculator', input: { expr: '2+2' } }]
        },
        { role: 'tool', toolCallId: 'call_1', content: '4' }
      ],
      system: 'You are a helpful assistant.',
      temperature: 0.7,
      maxTokens: 256,
      tools: [
        {
          name: 'calculator',
          description: 'Evaluate math',
          inputSchema: { type: 'object', properties: { expr: { type: 'string' } } }
        }
      ]
    }

    await provider.chat(request)

    // Verify the captured body has OpenAI wire format
    expect(capturedBody.model).toBe('MiniMax-Text-01')

    // System message should be FIRST message in the array (OpenAI format)
    expect(capturedBody.messages[0]).toEqual({
      role: 'system',
      content: 'You are a helpful assistant.'
    })

    // User message comes next
    expect(capturedBody.messages[1]).toEqual({
      role: 'user',
      content: 'What is 2+2?'
    })

    // Assistant message with tool_calls
    const assistantMsg = capturedBody.messages[2]
    expect(assistantMsg.role).toBe('assistant')
    expect(assistantMsg.content).toBe('2+2=4')
    expect(Array.isArray(assistantMsg.tool_calls)).toBe(true)
    expect(assistantMsg.tool_calls[0]).toEqual({
      id: 'call_1',
      type: 'function',
      function: {
        name: 'calculator',
        arguments: JSON.stringify({ expr: '2+2' })
      }
    })

    // Tool result message
    expect(capturedBody.messages[3]).toEqual({
      role: 'tool',
      tool_call_id: 'call_1',
      content: '4'
    })

    // Tools array (flat OpenAI format)
    expect(Array.isArray(capturedBody.tools)).toBe(true)
    expect(capturedBody.tools[0]).toEqual({
      type: 'function',
      function: {
        name: 'calculator',
        description: 'Evaluate math',
        parameters: { type: 'object', properties: { expr: { type: 'string' } } }
      }
    })

    // Temperature and maxTokens
    expect(capturedBody.temperature).toBe(0.7)
    expect(capturedBody.max_tokens).toBe(256)
  })

  it('respects custom endpoint and Bearer auth', async () => {
    let capturedUrl: string = ''
    let capturedHeaders: HeadersInit = {}
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, options?: RequestInit) => {
        capturedUrl = url
        capturedHeaders = options?.headers ?? {}
        return new Response(
          JSON.stringify({
            id: 'cmpl_456',
            model: 'test-model',
            choices: [{ message: { role: 'assistant', content: 'test' }, finish_reason: 'stop' }],
            usage: { prompt_tokens: 1, completion_tokens: 1 }
          }),
          { status: 200, headers: { 'content-type': 'application/json' } }
        )
      })
    )

    const customEndpoint = 'https://custom.minimax.example/v1/api'
    const provider = new MinimaxProvider('custom-key', 'custom-model', customEndpoint)

    await provider.chat({
      messages: [{ role: 'user', content: 'test' }]
    })

    expect(capturedUrl).toBe(customEndpoint)
    expect((capturedHeaders as Record<string, string>).authorization).toBe('Bearer custom-key')
  })

  it('stream respects stream:true flag in body', async () => {
    let capturedBody: any
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, options?: RequestInit) => {
        capturedBody = JSON.parse(String(options?.body ?? '{}'))
        return new Response('', { status: 200, headers: { 'content-type': 'text/event-stream' } })
      })
    )

    const provider = new MinimaxProvider('test-key')
    const request: ChatRequest = {
      messages: [{ role: 'user', content: 'test' }]
    }

    // Consume the async generator without caring about the events
    try {
      for await (const _ of provider.stream(request)) {
        break
      }
    } catch {
      // Ignore stream parsing errors; we only care about the body
    }

    expect(capturedBody.stream).toBe(true)
  })
})
