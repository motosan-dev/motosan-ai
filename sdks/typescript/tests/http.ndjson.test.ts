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
})
