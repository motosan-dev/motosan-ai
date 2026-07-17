import { afterEach, describe, expect, it, vi } from 'vitest'
import { Client } from '../src/client.js'
import { IncompleteStreamError, StreamError, StreamReadTimeoutError } from '../src/error.js'
import type { StreamEvent } from '../src/types.js'

// M3 milestone-Done conformance gates (specs/types.md stream termination).
// Cross-SDK mirrors:
// - sdks/rust/tests/m3_stream_conformance.rs
// - sdks/python/tests/test_m3_stream_conformance.py

function sseStream(text: string): ReadableStream<Uint8Array> {
  return new ReadableStream({
    start(controller) {
      controller.enqueue(new TextEncoder().encode(text))
      controller.close()
    },
  })
}

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

async function drainStream(stream: AsyncIterable<StreamEvent>) {
  const events: StreamEvent[] = []
  let error: unknown
  try {
    for await (const event of stream) {
      events.push(event)
    }
  } catch (caught) {
    error = caught
  }
  return { events, error }
}

describe('M3 stream termination conformance', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('kill-the-connection mid-stream rejects with IncompleteStreamError', async () => {
    const transcript =
      'event: message_start\n' +
      'data: {"type":"message_start","message":{"usage":{"input_tokens":5,"output_tokens":0}}}\n\n' +
      'event: content_block_start\n' +
      'data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial-partial"}}\n\n'
    stubSseFetch(transcript)

    const client = Client.builder().provider('anthropic').apiKey('sk-ant-api03-x').build()
    const { events, error } = await drainStream(
      client.stream({ messages: [{ role: 'user', content: 'hi' }] }),
    )

    expect(error).toBeInstanceOf(IncompleteStreamError)
    expect(error).toBeInstanceOf(StreamError)
    expect((error as Error).message).toBe(
      'incomplete stream: anthropic ended without a terminal event',
    )
    const text = events
      .filter((event) => event.eventType === 'text' && !event.done)
      .map((event) => event.content)
      .join('')
    expect(text.startsWith('partial')).toBe(true)
    expect(events.some((event) => event.done)).toBe(false)
  })

  it('hung stream read-idle expiry rejects with StreamReadTimeoutError', async () => {
    const firstChunk =
      'event: content_block_start\n' +
      'data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"tick-tick-tick"}}\n\n'
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(
            new ReadableStream<Uint8Array>({
              start(controller) {
                controller.enqueue(new TextEncoder().encode(firstChunk))
              },
            }),
            { status: 200, headers: { 'content-type': 'text/event-stream' } },
          ),
      ),
    )

    const client = Client.builder()
      .provider('anthropic')
      .apiKey('sk-ant-api03-x')
      .timeouts({ readIdleMs: 50 })
      .build()
    const { events, error } = await drainStream(
      client.stream({ messages: [{ role: 'user', content: 'hi' }] }),
    )

    expect(error).toBeInstanceOf(StreamReadTimeoutError)
    const text = events
      .filter((event) => event.eventType === 'text' && !event.done)
      .map((event) => event.content)
      .join('')
    expect(text.startsWith('tick')).toBe(true)
    expect(events.some((event) => event.done)).toBe(false)
  })
})
