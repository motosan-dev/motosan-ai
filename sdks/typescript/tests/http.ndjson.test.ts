import { describe, it, expect } from 'vitest'
import { parseNdjson } from '../src/http/ndjson.js'

describe('parseNdjson', () => {
  it('parses newline-delimited JSON objects', async () => {
    const input = '{"id":1,"text":"hello"}\n{"id":2,"text":"world"}\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(2)
    expect(objects[0]).toEqual({ id: 1, text: 'hello' })
    expect(objects[1]).toEqual({ id: 2, text: 'world' })
  })

  it('buffers across chunk boundaries (split mid-line)', async () => {
    const full = '{"x":100,"y":200}\n'
    const chunk1 = full.substring(0, 10) // '{"x":100,'
    const chunk2 = full.substring(10) // '"y":200}\n'

    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(chunk1))
        controller.enqueue(new TextEncoder().encode(chunk2))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(1)
    expect(objects[0]).toEqual({ x: 100, y: 200 })
  })

  it('buffers across multiple chunks', async () => {
    const full = '{"a":1}\n{"b":2}\n{"c":3}\n'
    const chunk1 = full.substring(0, 8) // '{"a":1}\n'
    const chunk2 = full.substring(8, 16) // '{"b":2}\n'
    const chunk3 = full.substring(16) // '{"c":3}\n'

    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(chunk1))
        controller.enqueue(new TextEncoder().encode(chunk2))
        controller.enqueue(new TextEncoder().encode(chunk3))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(3)
    expect(objects[0]).toEqual({ a: 1 })
    expect(objects[1]).toEqual({ b: 2 })
    expect(objects[2]).toEqual({ c: 3 })
  })

  it('skips malformed JSON lines silently', async () => {
    const input = '{"good":1}\n{not json}\n{"good":2}\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(2)
    expect(objects[0]).toEqual({ good: 1 })
    expect(objects[1]).toEqual({ good: 2 })
  })

  it('ignores empty lines', async () => {
    const input = '{"a":1}\n\n{"b":2}\n\n\n{"c":3}\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(3)
    expect(objects[0]).toEqual({ a: 1 })
    expect(objects[1]).toEqual({ b: 2 })
    expect(objects[2]).toEqual({ c: 3 })
  })

  it('handles stream ending without final newline', async () => {
    const input = '{"final":true}'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(1)
    expect(objects[0]).toEqual({ final: true })
  })

  it('handles mixed single and multi-line JSON objects (no nested newlines)', async () => {
    const input = '{"id":1,"name":"alice"}\n{"id":2,"text":"bob"}\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(2)
    expect(objects[0].name).toBe('alice')
    expect(objects[1].text).toBe('bob')
  })

  it('yields Ollama /api/chat objects verbatim including the done:true terminator', async () => {
    // A representative Ollama native NDJSON transcript: text deltas, then a
    // final {done:true} stats object. The PARSER must yield ALL of them,
    // including the done:true object — termination is the ADAPTER's job.
    const input =
      '{"model":"llama3.2","message":{"role":"assistant","content":"Hel"},"done":false}\n' +
      '{"model":"llama3.2","message":{"role":"assistant","content":"lo"},"done":false}\n' +
      '{"model":"llama3.2","message":{"role":"assistant","content":""},"done":true,"prompt_eval_count":9,"eval_count":4}\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(3)
    expect(objects[0].message.content).toBe('Hel')
    expect(objects[1].message.content).toBe('lo')
    // The parser MUST NOT swallow / stop on done:true — it just yields it.
    expect(objects[2].done).toBe(true)
    expect(objects[2].prompt_eval_count).toBe(9)
    expect(objects[2].eval_count).toBe(4)
  })

  it('splits an Ollama transcript delivered as one byte chunk per object', async () => {
    // Ollama streams one JSON object per flush; assert buffering across
    // chunk boundaries that land mid-object.
    const lines = [
      '{"message":{"content":"a"},"done":false}\n',
      '{"message":{"content":"b"},"done":false}\n',
      '{"done":true}\n',
    ]
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        for (const line of lines) {
          // split each line in half to force mid-line buffering
          const mid = Math.floor(line.length / 2)
          controller.enqueue(new TextEncoder().encode(line.slice(0, mid)))
          controller.enqueue(new TextEncoder().encode(line.slice(mid)))
        }
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(3)
    expect(objects.map((o) => o.message?.content ?? null)).toEqual(['a', 'b', null])
    expect(objects[2].done).toBe(true)
  })

  it('flushes a final Ollama object with no trailing newline (EOF without done)', async () => {
    // Mirrors Rust NdjsonStream flushing the trailing buffer line at EOF;
    // here the stream ends WITHOUT a done:true object and no final newline.
    const input = '{"message":{"content":"partial"},"done":false}'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(1)
    expect(objects[0].message.content).toBe('partial')
    expect(objects[0].done).toBe(false)
  })

  it('skips a malformed Ollama line and keeps the surrounding valid objects', async () => {
    // Transport hiccup mid-stream: a truncated/garbage line is dropped, the
    // valid objects on either side survive (M3 mid-stream-resilience contract).
    const input =
      '{"message":{"content":"ok1"},"done":false}\n' +
      '{"message":{"content":"ok2",broken}\n' +
      '{"done":true}\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(2)
    expect(objects[0].message.content).toBe('ok1')
    expect(objects[1].done).toBe(true)
  })
})
