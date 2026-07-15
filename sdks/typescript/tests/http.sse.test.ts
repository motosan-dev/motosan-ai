import { describe, it, expect } from 'vitest'
import { parseSse } from '../src/http/sse.js'

describe('parseSse', () => {
  it('parses basic SSE events with event and data fields', async () => {
    const input = 'event: message\ndata: {"type":"text","content":"hello"}\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].event).toBe('message')
    expect(events[0].data).toEqual({ type: 'text', content: 'hello' })
  })

  it('handles multiple events in stream', async () => {
    const input =
      'event: start\ndata: {"id":1}\n\nevent: delta\ndata: {"text":"hi"}\n\nevent: done\ndata: [DONE]\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(3)
    expect(events[0].event).toBe('start')
    expect(events[0].data).toEqual({ id: 1 })
    expect(events[1].event).toBe('delta')
    expect(events[1].data).toEqual({ text: 'hi' })
    expect(events[2].event).toBe('done')
    expect(events[2].data).toBe('[DONE]')
  })

  it('buffers across chunk boundaries (split mid-line)', async () => {
    const full = 'event: message\ndata: {"content":"split"}\n\n'
    const chunk1 = full.substring(0, 15) // "event: message\nda"
    const chunk2 = full.substring(15) // 'ta: {"content":"split"}\n\n'

    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(chunk1))
        controller.enqueue(new TextEncoder().encode(chunk2))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].event).toBe('message')
    expect(events[0].data).toEqual({ content: 'split' })
  })

  it('buffers across chunk boundaries (split mid-JSON)', async () => {
    const full = 'event: msg\ndata: {"x":123,"y":456}\n\n'
    const chunk1 = full.substring(0, 25) // "event: msg\ndata: {"x":123"
    const chunk2 = full.substring(25) // ',"y":456}\n\n'

    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(chunk1))
        controller.enqueue(new TextEncoder().encode(chunk2))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].event).toBe('msg')
    expect(events[0].data).toEqual({ x: 123, y: 456 })
  })

  it('skips malformed JSON data silently', async () => {
    const input =
      'event: good\ndata: {"valid":true}\n\nevent: bad\ndata: {not json}\n\nevent: good2\ndata: {"valid":2}\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(2)
    expect(events[0].event).toBe('good')
    expect(events[1].event).toBe('good2')
  })

  it('recognizes [DONE] string but does not terminate parsing', async () => {
    const input =
      'event: delta\ndata: {"text":"chunk1"}\n\nevent: done\ndata: [DONE]\n\nevent: final\ndata: {"text":"chunk2"}\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(3)
    expect(events[1].data).toBe('[DONE]')
    expect(events[2].event).toBe('final')
  })

  it('handles events without event field (data only)', async () => {
    const input = 'data: {"text":"no event"}\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].event).toBeUndefined()
    expect(events[0].data).toEqual({ text: 'no event' })
  })

  it('ignores empty lines and non-field lines', async () => {
    const input = 'event: msg\n\ndata: {"ok":true}\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].event).toBe('msg')
    expect(events[0].data).toEqual({ ok: true })
  })

  it('handles stream ending without final double newline', async () => {
    const input = 'event: last\ndata: {"final":true}'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].event).toBe('last')
    expect(events[0].data).toEqual({ final: true })
  })

  it('cancels the underlying stream when the consumer exits early', async () => {
    let cancelled = false
    const input = 'event: first\ndata: {"n":1}\n\nevent: second\ndata: {"n":2}\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        // Never close: simulates a long-lived HTTP body held open by the server.
      },
      cancel() {
        cancelled = true
      }
    })

    for await (const event of parseSse(stream)) {
      expect(event.data).toEqual({ n: 1 })
      break // early exit after the first event
    }

    expect(cancelled).toBe(true)
  })

  it('parses CRLF line endings and CRLF CRLF event separators (WHATWG spec)', async () => {
    // Identical payload to the LF multi-event test, delivered with \r\n.
    const input =
      'event: start\r\ndata: {"id":1}\r\n\r\nevent: delta\r\ndata: {"text":"hi"}\r\n\r\nevent: done\r\ndata: [DONE]\r\n\r\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(3)
    expect(events[0].event).toBe('start')
    expect(events[0].data).toEqual({ id: 1 })
    expect(events[1].event).toBe('delta')
    expect(events[1].data).toEqual({ text: 'hi' })
    expect(events[2].event).toBe('done')
    expect(events[2].data).toBe('[DONE]')
  })

  it('parses bare CR line endings and CR CR event separators', async () => {
    const input = 'event: message\rdata: {"n":1}\r\r'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].event).toBe('message')
    expect(events[0].data).toEqual({ n: 1 })
  })

  it('parses a CRLF pair split across chunk boundaries', async () => {
    const full = 'event: a\r\ndata: {"n":1}\r\n\r\nevent: b\r\ndata: {"n":2}\r\n\r\n'
    const splitAt = full.indexOf('\r\n\r\n') + 1 // chunk1 ends with a lone \r
    const chunk1 = full.substring(0, splitAt)
    const chunk2 = full.substring(splitAt) // starts with \n\r\n

    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(chunk1))
        controller.enqueue(new TextEncoder().encode(chunk2))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(2)
    expect(events[0].event).toBe('a')
    expect(events[0].data).toEqual({ n: 1 })
    expect(events[1].event).toBe('b')
    expect(events[1].data).toEqual({ n: 2 })
  })

  it('preserves data bytes verbatim after removing at most one leading space', async () => {
    const input = 'data: {"text":"  two leading spaces kept"}\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].data).toEqual({ text: '  two leading spaces kept' })
  })

  it('parses a data field with no space after the colon', async () => {
    const input = 'data:{"n":7}\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].data).toEqual({ n: 7 })
  })
})
