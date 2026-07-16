import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ClientBuilder } from '../src/client.js'
import {
  DEFAULT_OPENAI_RESPONSES_URL,
  OpenAIProvider,
} from '../src/providers/openai.js'
import { ProviderError } from '../src/error.js'
import { RetryPolicy } from '../src/retry.js'
import type { ChatRequest } from '../src/types.js'

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  })
}

describe('OpenAI Responses-API fallback', () => {
  let mockFetch: ReturnType<typeof vi.fn>
  let provider: OpenAIProvider

  beforeEach(() => {
    mockFetch = vi.fn()
    vi.stubGlobal('fetch', mockFetch)
    provider = new OpenAIProvider('test-key', 'gpt-4o')
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('exports the default Responses endpoint', () => {
    expect(DEFAULT_OPENAI_RESPONSES_URL).toBe('https://api.openai.com/v1/responses')
  })

  it('404 with fallback disabled by default throws ProviderError', async () => {
    mockFetch.mockResolvedValue(jsonResponse(404, { error: { message: 'Not found' } }))

    const request: ChatRequest = { messages: [{ role: 'user', content: 'hello' }] }

    try {
      await provider.chat(request)
      throw new Error('expected provider.chat to throw')
    } catch (error) {
      expect(error).toBeInstanceOf(ProviderError)
      expect((error as Error).message).toContain('Not found')
    }
  })

  it('404 with fallback enabled falls back to the Responses API', async () => {
    provider.withResponsesFallback(true)
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, { error: { message: 'Not found' } }))
      .mockResolvedValueOnce(
        jsonResponse(200, {
          output_text: 'Hello, world!',
          model: 'gpt-4o',
          usage: { input_tokens: 10, output_tokens: 5 },
        }),
      )

    const response = await provider.chat({ messages: [{ role: 'user', content: 'hello' }] })

    expect(response.content).toBe('Hello, world!')
    expect(response.stopReason).toBe('stop')
    expect(response.toolCalls).toEqual([])
    expect(response.usage).toEqual({ inputTokens: 10, outputTokens: 5 })
    expect(mockFetch).toHaveBeenCalledTimes(2)
    expect(String(mockFetch.mock.calls[1][0])).toBe(DEFAULT_OPENAI_RESPONSES_URL)
  })

  it('extracts output_text with surrounding whitespace trimmed', async () => {
    provider.withResponsesFallback(true)
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, {}))
      .mockResolvedValueOnce(
        jsonResponse(200, {
          output_text: '  trimmed content  ',
          usage: { input_tokens: 1, output_tokens: 1 },
        }),
      )

    const response = await provider.chat({ messages: [{ role: 'user', content: 'test' }] })

    expect(response.content).toBe('trimmed content')
  })

  it('falls back to output[0].content[*].text when output_text is blank', async () => {
    provider.withResponsesFallback(true)
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, {}))
      .mockResolvedValueOnce(
        jsonResponse(200, {
          output_text: '   ',
          output: [{ content: [{ type: 'text', text: 'fallback text' }] }],
          usage: { input_tokens: 1, output_tokens: 1 },
        }),
      )

    const response = await provider.chat({ messages: [{ role: 'user', content: 'test' }] })

    expect(response.content).toBe('fallback text')
  })

  it('falls back to nested output[0].content[*].content[*].text', async () => {
    provider.withResponsesFallback(true)
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, {}))
      .mockResolvedValueOnce(
        jsonResponse(200, {
          output: [{ content: [{ content: [{ type: 'text', text: 'nested text' }] }] }],
          usage: { input_tokens: 1, output_tokens: 1 },
        }),
      )

    const response = await provider.chat({ messages: [{ role: 'user', content: 'test' }] })

    expect(response.content).toBe('nested text')
  })

  it('derives the Responses URL from a custom base URL', async () => {
    provider = new OpenAIProvider('test-key', 'gpt-4o', 'https://custom.openai.test/v1/')
    provider.withResponsesFallback(true)
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, {}))
      .mockResolvedValueOnce(jsonResponse(200, { output_text: 'ok', usage: {} }))

    await provider.chat({ messages: [{ role: 'user', content: 'test' }] })

    expect(String(mockFetch.mock.calls[0][0])).toBe(
      'https://custom.openai.test/v1/chat/completions',
    )
    expect(String(mockFetch.mock.calls[1][0])).toBe(
      'https://custom.openai.test/v1/responses',
    )
  })

  it('builds Responses instructions from systemBlocks before system', async () => {
    provider.withResponsesFallback(true)
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, {}))
      .mockResolvedValueOnce(jsonResponse(200, { output_text: 'ok', usage: {} }))

    await provider.chat({
      messages: [{ role: 'user', content: 'test' }],
      system: 'Ignored system string',
      systemBlocks: [{ text: 'Block 1' }, { text: '  Block 2  ' }],
    })

    const body = JSON.parse(String(mockFetch.mock.calls[1][1].body))
    expect(body.instructions).toBe('Block 1\n\nBlock 2')
  })

  it('adds system-role messages to Responses instructions', async () => {
    provider.withResponsesFallback(true)
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, {}))
      .mockResolvedValueOnce(jsonResponse(200, { output_text: 'ok', usage: {} }))

    await provider.chat({
      messages: [
        { role: 'system', content: 'Per-message system' },
        { role: 'user', content: 'test' },
      ],
    })

    const body = JSON.parse(String(mockFetch.mock.calls[1][1].body))
    expect(body.instructions).toBe('Per-message system')
  })

  it('maps supported message roles into the Responses input array', async () => {
    provider.withResponsesFallback(true)
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, {}))
      .mockResolvedValueOnce(jsonResponse(200, { output_text: 'ok', usage: {} }))

    await provider.chat({
      messages: [
        { role: 'user', content: 'user message' },
        {
          role: 'assistant',
          content: 'calling tool',
          toolCalls: [{ id: 'call-123', name: 'foo', input: { arg: 'value' } }],
        },
        { role: 'tool', content: 'tool result', toolCallId: 'call-123' },
      ],
    })

    const body = JSON.parse(String(mockFetch.mock.calls[1][1].body))
    expect(body.input).toContainEqual({ role: 'user', content: 'user message' })
    expect(body.input).toContainEqual({
      role: 'assistant',
      content: 'calling tool',
      tool_calls: [
        {
          id: 'call-123',
          type: 'function',
          function: { name: 'foo', arguments: '{"arg":"value"}' },
        },
      ],
    })
    expect(body.input).toContainEqual({
      role: 'tool',
      tool_call_id: 'call-123',
      content: 'tool result',
    })
  })

  it('merges providerOptions into the Responses request body', async () => {
    provider.withResponsesFallback(true)
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, {}))
      .mockResolvedValueOnce(jsonResponse(200, { output_text: 'ok', usage: {} }))

    await provider.chat({
      messages: [{ role: 'user', content: 'test' }],
      providerOptions: { store: false, previous_response_id: 'resp-123' },
    })

    const body = JSON.parse(String(mockFetch.mock.calls[1][1].body))
    expect(body.store).toBe(false)
    expect(body.previous_response_id).toBe('resp-123')
  })

  it('uses prompt_tokens and completion_tokens as usage fallback', async () => {
    provider.withResponsesFallback(true)
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, {}))
      .mockResolvedValueOnce(
        jsonResponse(200, {
          output_text: 'ok',
          usage: { prompt_tokens: 10, completion_tokens: 5 },
        }),
      )

    const response = await provider.chat({ messages: [{ role: 'user', content: 'test' }] })

    expect(response.usage.inputTokens).toBe(10)
    expect(response.usage.outputTokens).toBe(5)
  })

  it('uses a custom Responses URL and trims trailing slashes', async () => {
    provider.withResponsesFallback(true).withResponsesUrl('https://custom.openai.test/responses///')
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, {}))
      .mockResolvedValueOnce(jsonResponse(200, { output_text: 'ok', usage: {} }))

    await provider.chat({ messages: [{ role: 'user', content: 'test' }] })

    expect(String(mockFetch.mock.calls[1][0])).toBe('https://custom.openai.test/responses')
  })

  it('retries the Responses fallback call on a retryable 5xx then succeeds (shared path)', async () => {
    provider
      .withRetryPolicy(new RetryPolicy({ maxRetries: 3, baseDelayMs: 1, jitter: false }))
      .withResponsesFallback(true)
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, {}))
      .mockResolvedValueOnce(jsonResponse(500, { error: { message: 'Responses unavailable' } }))
      .mockResolvedValueOnce(
        jsonResponse(200, {
          output_text: 'recovered',
          usage: { input_tokens: 1, output_tokens: 1 },
        }),
      )

    const response = await provider.chat({ messages: [{ role: 'user', content: 'test' }] })

    expect(response.content).toBe('recovered')
    // 3 total fetches: the triggering 404 on chat/completions, then the
    // Responses call's 500 -> 200 retry through the shared withRetry engine
    // (the two Responses-endpoint fetches are the retried pair).
    expect(mockFetch).toHaveBeenCalledTimes(3)
    expect(String(mockFetch.mock.calls[1][0])).toBe(DEFAULT_OPENAI_RESPONSES_URL)
    expect(String(mockFetch.mock.calls[2][0])).toBe(DEFAULT_OPENAI_RESPONSES_URL)
  })

  it('does not retry a non-retryable 404 in the Responses fallback path', async () => {
    provider
      .withRetryPolicy(new RetryPolicy({ maxRetries: 3, baseDelayMs: 1, jitter: false }))
      .withResponsesFallback(true)
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, {}))
      .mockResolvedValueOnce(jsonResponse(404, { error: { message: 'Responses not found' } }))

    await expect(provider.chat({ messages: [{ role: 'user', content: 'test' }] })).rejects.toThrow(
      'Responses not found',
    )
    // 2 total fetches: chat/completions 404 triggers the fallback, and the
    // Responses 404 is non-retryable so withRetry rethrows without a retry.
    expect(mockFetch).toHaveBeenCalledTimes(2)
  })

  it('ClientBuilder wires openaiResponsesFallback and openaiResponsesUrl', async () => {
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, {}))
      .mockResolvedValueOnce(jsonResponse(200, { output_text: 'ok', usage: {} }))
    const client = new ClientBuilder()
      .provider('openai')
      .apiKey('test-key')
      .openaiResponsesFallback(true)
      .openaiResponsesUrl('https://builder.openai.test/responses/')
      .build()

    const response = await client.chat({ messages: [{ role: 'user', content: 'test' }] })

    expect(response.content).toBe('ok')
    expect(String(mockFetch.mock.calls[1][0])).toBe('https://builder.openai.test/responses')
  })
})
