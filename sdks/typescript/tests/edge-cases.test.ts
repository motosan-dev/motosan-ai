import { describe, it, expect, vi, afterEach } from 'vitest'
import { AnthropicProvider } from '../src/providers/anthropic.js'
import { OpenAIProvider } from '../src/providers/openai.js'
import { Client } from '../src/client.js'
import { collectStream } from '../src/stream.js'
import { validateRequest, fullCaps } from '../src/provider.js'
import { serializeAnthropicRequest } from '../src/serialize/anthropic.js'
import { serializeOpenAiRequest } from '../src/serialize/openai.js'
import { serializeGeminiRequest } from '../src/serialize/gemini.js'
import { MotosanError, ProviderError } from '../src/error.js'
import { DEFAULT_ANTHROPIC_MODEL, DEFAULT_OPENAI_MODEL, DEFAULT_GEMINI_MODEL } from '../src/models.js'
import type { StreamEvent } from '../src/types.js'

// Edge-case coverage layered on top of the per-provider suites. These tests
// assert EXISTING SDK behavior (M7 ships no wire changes); they never alter src.

// Build a one-shot ReadableStream from an SSE/NDJSON transcript string. The
// provider stream adapters read `response.body` as a ReadableStream<Uint8Array>.
function sseStream(text: string): ReadableStream<Uint8Array> {
  return new ReadableStream({
    start(controller) {
      controller.enqueue(new TextEncoder().encode(text))
      controller.close()
    },
  })
}

// Mock fetch to return an SSE stream body (status 200).
function stubSseFetch(transcript: string): void {
  vi.stubGlobal(
    'fetch',
    vi.fn(
      async () =>
        new Response(sseStream(transcript), {
          status: 200,
          headers: { 'content-type': 'text/event-stream' },
        }),
    ),
  )
}

// Mock fetch to return an arbitrary status + body (for the status-code matrix).
function stubStatusFetch(status: number, body: string | null): void {
  vi.stubGlobal('fetch', vi.fn(async () => new Response(body, { status })))
}

describe('edge cases', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  describe('empty messages', () => {
    it('serializeAnthropicRequest produces a well-formed body with empty messages', () => {
      const body = serializeAnthropicRequest({ messages: [] }, DEFAULT_ANTHROPIC_MODEL)
      expect(body.model).toBe(DEFAULT_ANTHROPIC_MODEL)
      expect(Array.isArray(body.messages)).toBe(true)
      expect(body.messages).toEqual([])
    })

    it('serializeOpenAiRequest produces a well-formed body with empty messages', () => {
      const body = serializeOpenAiRequest({ messages: [] }, DEFAULT_OPENAI_MODEL)
      expect(body.model).toBe(DEFAULT_OPENAI_MODEL)
      expect(Array.isArray(body.messages)).toBe(true)
      // No system / user messages were supplied, so the array is empty.
      expect(body.messages).toEqual([])
    })

    it('serializeGeminiRequest produces a well-formed body with empty messages', () => {
      const body = serializeGeminiRequest({ messages: [] }, DEFAULT_GEMINI_MODEL)
      expect(Array.isArray(body.contents)).toBe(true)
      expect(body.contents).toEqual([])
    })

    it('validateRequest does not throw for empty messages under full caps', () => {
      expect(() => validateRequest({ messages: [] }, fullCaps())).not.toThrow()
    })
  })

  describe('malformed / null SSE JSON mid-stream (adapter layer)', () => {
    it('skips malformed and null data lines, still yields text, ends with done (Anthropic adapter)', async () => {
      // Interleave a valid event, a malformed-JSON data line, a `data: null`
      // line, more valid text, then a terminal message_stop. The raw parser
      // drops the malformed line; the adapter skips the null-data event
      // (`if (!data) continue`). Neither should throw.
      const transcript =
        'event: message_start\n' +
        'data: {"type":"message_start","message":{"usage":{"input_tokens":3,"output_tokens":0}}}\n\n' +
        'event: content_block_start\n' +
        'data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\n' +
        'event: content_block_delta\n' +
        'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}\n\n' +
        'event: content_block_delta\n' +
        'data: {not json\n\n' +
        'event: content_block_delta\n' +
        'data: null\n\n' +
        'event: content_block_delta\n' +
        'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world"}}\n\n' +
        'event: content_block_stop\n' +
        'data: {"type":"content_block_stop","index":0}\n\n' +
        'event: message_stop\n' +
        'data: {"type":"message_stop"}\n\n'
      stubSseFetch(transcript)

      const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
      const events: StreamEvent[] = []
      await expect(
        (async () => {
          for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
            events.push(evt)
          }
        })(),
      ).resolves.toBeUndefined()

      const text = events
        .filter((e) => e.eventType === 'text' && !e.done)
        .map((e) => e.content)
        .join('')
      expect(text).toBe('Hello world')
      const last = events[events.length - 1]
      expect(last.done).toBe(true)
    })
  })

  describe('mid-stream reset / partial success', () => {
    // Anthropic & OpenAI fabricate a defensive `done` on EOF without a terminal
    // event (anthropic.ts:386-391, openai.ts:460-474). Gemini deliberately does
    // NOT (gemini.ts:262) and is already covered in providers-gemini.test.ts —
    // so it is excluded here per the plan's Step-1 "add only the missing" rule.

    it('Anthropic: stream that ends without message_stop terminates silently with a partial response', async () => {
      const transcript =
        'event: message_start\n' +
        'data: {"type":"message_start","message":{"usage":{"input_tokens":5,"output_tokens":0}}}\n\n' +
        'event: content_block_start\n' +
        'data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\n' +
        'event: content_block_delta\n' +
        'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}\n\n'
      // No content_block_stop, no message_delta, no message_stop — dropped mid-flight.
      stubSseFetch(transcript)

      const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
      const events: StreamEvent[] = []
      for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        events.push(evt)
      }
      expect(events.filter((e) => e.eventType === 'text' && !e.done).map((e) => e.content)).toEqual([
        'partial',
      ])
      const last = events[events.length - 1]
      expect(last.done).toBe(true)

      // collectStream tolerates the drop and fabricates a terminal stopReason.
      stubSseFetch(transcript)
      const resp = await collectStream(
        provider.stream({ messages: [{ role: 'user', content: 'hi' }] }),
      )
      expect(resp.content).toBe('partial')
      expect(Array.isArray(resp.toolCalls)).toBe(true)
      expect(resp.toolCalls.length).toBe(0)
      expect(resp.stopReason).toBe('end_turn') // fabricated: no tool calls
    })

    it('OpenAI: stream that ends without [DONE]/finish_reason terminates silently with a partial response', async () => {
      const transcript =
        'data: {"choices":[{"index":0,"delta":{"content":"partial"}}]}\n\n'
      // No further chunks, no finish_reason, no [DONE] — dropped mid-flight.
      stubSseFetch(transcript)

      const provider = new OpenAIProvider('sk-test', 'gpt-4o')
      const events: StreamEvent[] = []
      for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        events.push(evt)
      }
      expect(events.filter((e) => e.eventType === 'text' && !e.done).map((e) => e.content)).toEqual([
        'partial',
      ])
      const last = events[events.length - 1]
      expect(last.done).toBe(true)

      stubSseFetch(transcript)
      const resp = await collectStream(
        provider.stream({ messages: [{ role: 'user', content: 'hi' }] }),
      )
      expect(resp.content).toBe('partial')
      expect(Array.isArray(resp.toolCalls)).toBe(true)
      expect(resp.toolCalls.length).toBe(0)
      expect(resp.stopReason).toBe('end_turn')
    })
  })

  describe('unexpected / non-error status codes', () => {
    // mapHttpError (error.ts:38-57) maps 401/429/400 specifically and EVERYTHING
    // ELSE (incl. 301/418) to ProviderError; postJson throws for any !response.ok.
    // 204 / empty-200 are response.ok=true, so postJson reaches response.json()
    // which throws on an empty body (a JSON parse SyntaxError, NOT a MotosanError).

    it('204 (no body): chat rejects on the empty-body JSON parse', async () => {
      stubStatusFetch(204, null)
      const client = new Client({ provider: 'anthropic', apiKey: 'sk-ant-api03-x' })
      await expect(client.chat({ messages: [{ role: 'user', content: 'hi' }] })).rejects.toThrow()
    })

    it('empty-body 200: chat rejects on the empty-body JSON parse', async () => {
      stubStatusFetch(200, '')
      const client = new Client({ provider: 'anthropic', apiKey: 'sk-ant-api03-x' })
      await expect(client.chat({ messages: [{ role: 'user', content: 'hi' }] })).rejects.toThrow()
    })

    it('301 (not ok): chat throws a mapped ProviderError', async () => {
      stubStatusFetch(301, 'moved')
      const client = new Client({ provider: 'anthropic', apiKey: 'sk-ant-api03-x' })
      const err = await client
        .chat({ messages: [{ role: 'user', content: 'hi' }] })
        .then(() => null)
        .catch((e) => e)
      expect(err).toBeInstanceOf(ProviderError)
      expect(err).toBeInstanceOf(MotosanError)
      expect((err as ProviderError).status).toBe(301)
    })

    it('418 (unmapped 4xx): chat throws a mapped ProviderError', async () => {
      stubStatusFetch(418, 'teapot')
      const client = new Client({ provider: 'anthropic', apiKey: 'sk-ant-api03-x' })
      const err = await client
        .chat({ messages: [{ role: 'user', content: 'hi' }] })
        .then(() => null)
        .catch((e) => e)
      expect(err).toBeInstanceOf(ProviderError)
      expect(err).toBeInstanceOf(MotosanError)
      expect((err as ProviderError).status).toBe(418)
    })
  })
})
