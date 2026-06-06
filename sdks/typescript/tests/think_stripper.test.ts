import { describe, expect, it } from 'vitest'
import { ThinkStripper, stripThink } from '../src/think_stripper.js'
import { doneWithStopReason, textEvent, toolCallStart, usageEvent } from '../src/stream.js'
import type { StreamEvent } from '../src/types.js'

async function collectEvents(stream: AsyncIterable<StreamEvent>): Promise<StreamEvent[]> {
  const events: StreamEvent[] = []
  for await (const event of stream) {
    events.push(event)
  }
  return events
}

describe('ThinkStripper', () => {
  it('strips a complete think block in one chunk', () => {
    const stripper = new ThinkStripper()

    const output = stripper.feed('<think>reasoning</think>answer is here') + stripper.flush()

    expect(output).toBe('answer is here')
  })

  it('strips a think block across multiple chunks', () => {
    const stripper = new ThinkStripper()
    let output = ''

    output += stripper.feed('<think>reas')
    output += stripper.feed('oning</think>answer is here')
    output += stripper.flush()

    expect(output).toBe('answer is here')
  })

  it('strips a think block with a split opening tag', () => {
    const stripper = new ThinkStripper()
    let output = ''

    output += stripper.feed('hello<thi')
    output += stripper.feed('nk>hidden</think>world!')
    output += stripper.flush()

    expect(output).toBe('helloworld!')
  })

  it('strips a think block with a split closing tag', () => {
    const stripper = new ThinkStripper()
    let output = ''

    output += stripper.feed('<think>hidden</thi')
    output += stripper.feed('nk>visible text')
    output += stripper.flush()

    expect(output).toBe('visible text')
  })

  it('passes through text without think tags', () => {
    const stripper = new ThinkStripper()
    let output = ''

    output += stripper.feed('hello ')
    output += stripper.feed('world')
    output += stripper.flush()

    expect(output).toBe('hello world')
  })

  it('strips multiple think blocks in one chunk', () => {
    const stripper = new ThinkStripper()

    const output = stripper.feed('<think>a</think>between<think>c</think>after!') + stripper.flush()

    expect(output).toBe('betweenafter!')
  })

  it('flushes a partial opening-tag tail when not inside think', () => {
    const stripper = new ThinkStripper()

    const output = stripper.feed('hello <')

    expect(output).toBe('h')
    expect(stripper.flush()).toBe('ello <')
  })

  it('drops an unclosed think block on flush and resets state', () => {
    const stripper = new ThinkStripper()

    stripper.feed('<think>still thinking')
    expect(stripper.flush()).toBe('')

    const output = stripper.feed('visible</think>') + stripper.flush()
    expect(output).toBe('visible</think>')
  })

  it('does not emit half of a UTF-16 surrogate pair', () => {
    const stripper = new ThinkStripper()
    const emoji = '😀'
    const input = `hello ${emoji}`
    const firstChunk = input.slice(0, 7)
    const secondChunk = input.slice(7)

    const firstOutput = stripper.feed(firstChunk)
    const secondOutput = stripper.feed(secondChunk)
    const combined = firstOutput + secondOutput + stripper.flush()

    expect(firstOutput).not.toContain('\ud83d')
    expect(secondOutput).not.toContain('\udc00')
    expect(combined).toBe(input)
  })
})

describe('stripThink stream wrapper', () => {
  it('yields text events with stripped content', async () => {
    const innerStream = (async function* () {
      yield textEvent('before<think>hidden')
      yield textEvent('</think>after')
      yield doneWithStopReason('end_turn')
    })()

    const events = await collectEvents(stripThink(innerStream))

    expect(events).toHaveLength(3)
    expect(events[0]).toMatchObject({ eventType: 'text', content: 'before' })
    expect(events[1]).toMatchObject({ eventType: 'text', content: 'after' })
    expect(events[2]).toMatchObject({ done: true, stopReason: 'end_turn' })
  })

  it('skips empty text events after stripping', async () => {
    const innerStream = (async function* () {
      yield textEvent('<think>hidden</think>')
      yield textEvent('visible')
      yield doneWithStopReason('end_turn')
    })()

    const events = await collectEvents(stripThink(innerStream))

    const textEvents = events.filter((event) => event.eventType === 'text' && !event.done)
    expect(textEvents.map((event) => event.content).join('')).toBe('visible')
    expect(events.at(-1)).toMatchObject({ done: true })
  })

  it('flushes tail before the done event', async () => {
    const innerStream = (async function* () {
      yield textEvent('tail')
      yield doneWithStopReason('end_turn')
    })()

    const events = await collectEvents(stripThink(innerStream))

    expect(events).toHaveLength(2)
    expect(events[0]).toMatchObject({ eventType: 'text', content: 'tail' })
    expect(events[1]).toMatchObject({ done: true, stopReason: 'end_turn' })
  })

  it('preserves stopReason on the done event', async () => {
    const innerStream = (async function* () {
      yield textEvent('output<think>x</think>')
      yield doneWithStopReason('tool_use')
    })()

    const events = await collectEvents(stripThink(innerStream))

    expect(events.find((event) => event.done)?.stopReason).toBe('tool_use')
  })

  it('passes through non-text events unchanged', async () => {
    const usage = usageEvent({ inputTokens: 10, outputTokens: 5 })
    const toolStart = toolCallStart('id1', 'tool')
    const innerStream = (async function* () {
      yield usage
      yield textEvent('hello<think>x</think>world')
      yield toolStart
      yield doneWithStopReason('end_turn')
    })()

    const events = await collectEvents(stripThink(innerStream))

    expect(events[0]).toBe(usage)
    expect(events[1]).toMatchObject({ eventType: 'text', content: 'hello' })
    expect(events[2]).toBe(toolStart)
    expect(
      events.filter((event) => event.eventType === 'text' && !event.done).map((event) => event.content).join(''),
    ).toBe('helloworld')
    expect(events.at(-1)).toMatchObject({ done: true })
  })

  it('handles a stream with no done event', async () => {
    const innerStream = (async function* () {
      yield textEvent('hello<think>hidden')
      yield textEvent('</think>world')
    })()

    const events = await collectEvents(stripThink(innerStream))

    expect(events).toHaveLength(2)
    expect(events[0]).toMatchObject({ eventType: 'text', content: 'hello' })
    expect(events[1]).toMatchObject({ eventType: 'text', content: 'world' })
    expect(events.every((event) => !event.done)).toBe(true)
  })

  it('regression: emits buffered tail before done', async () => {
    const innerStream = (async function* () {
      yield textEvent('<think>x</think>')
      yield textEvent('tail text')
      yield doneWithStopReason('stop')
    })()

    const events = await collectEvents(stripThink(innerStream))
    const doneIndex = events.findIndex((event) => event.done)
    const textBeforeDone = events
      .slice(0, doneIndex)
      .filter((event) => event.eventType === 'text')
      .map((event) => event.content)
      .join('')

    expect(doneIndex).toBeGreaterThanOrEqual(0)
    expect(textBeforeDone).toBe('tail text')
    expect(events.at(-1)).toMatchObject({ done: true, stopReason: 'stop' })
  })
})
