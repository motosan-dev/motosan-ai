import { describe, it, expect } from 'vitest'
import {
  textEvent,
  doneEvent,
  doneWithStopReason,
  usageEvent,
  toolCallStart,
  toolCallArgs,
  toolCallArgsWithId,
  toolCallEnd,
  toolCallEndWithId,
  thinkingDelta,
  thinkingDone,
  collectStream,
  type BoxStream,
} from '../src/stream.js'
import type { StreamEvent } from '../src/types.js'

describe('stream.ts', () => {
  describe('collectStream', () => {
    it('accumulates text and usage for simple response (no thinking)', async () => {
      const events: StreamEvent[] = [
        textEvent('Hello '),
        textEvent('world'),
        usageEvent({ inputTokens: 10, outputTokens: 5 }),
        doneEvent(),
      ]
      const stream = (async function* () {
        for (const ev of events) yield ev
      })() as BoxStream

      const response = await collectStream(stream)

      expect(response.content).toBe('Hello world')
      expect(response.usage.inputTokens).toBe(10)
      expect(response.usage.outputTokens).toBe(5)
      expect(response.model).toBe('')
      expect(response.stopReason).toBe('end_turn')
      expect(response.thinking).toBeUndefined()
      expect(response.toolCalls).toEqual([])
    })

    it('converts empty thinkingDone block to undefined', async () => {
      const events: StreamEvent[] = [
        thinkingDone(''),
        textEvent('Answer: 42'),
        doneEvent(),
      ]
      const stream = (async function* () {
        for (const ev of events) yield ev
      })() as BoxStream

      const response = await collectStream(stream)

      expect(response.content).toBe('Answer: 42')
      expect(response.thinking).toBeUndefined()
    })

    it('falls back to accumulated thinkingDeltas if no thinkingDone fired', async () => {
      const events: StreamEvent[] = [
        thinkingDelta('A '),
        thinkingDelta('B '),
        thinkingDelta('C'),
        textEvent('answer'),
        doneEvent(),
      ]
      const stream = (async function* () {
        for (const ev of events) yield ev
      })() as BoxStream

      const response = await collectStream(stream)

      expect(response.thinking).toBe('A B C')
      expect(response.content).toBe('answer')
    })

    it('prefers thinkingDone over accumulated deltas', async () => {
      const events: StreamEvent[] = [
        thinkingDelta('wrong '),
        thinkingDelta('data'),
        thinkingDone('Correct thinking text'),
        textEvent('Final answer'),
        doneEvent(),
      ]
      const stream = (async function* () {
        for (const ev of events) yield ev
      })() as BoxStream

      const response = await collectStream(stream)

      expect(response.thinking).toBe('Correct thinking text')
      expect(response.content).toBe('Final answer')
    })

    it('accumulates tool calls with correct input parsing', async () => {
      const events: StreamEvent[] = [
        toolCallStart('call_1', 'get_weather'),
        toolCallArgs('{"city":"'),
        toolCallArgs('Tokyo",'),
        toolCallArgs('"units":"celsius"}'),
        toolCallEnd(),
        textEvent('Got weather'),
        usageEvent({ inputTokens: 20, outputTokens: 15 }),
        doneWithStopReason('tool_use'),
      ]
      const stream = (async function* () {
        for (const ev of events) yield ev
      })() as BoxStream

      const response = await collectStream(stream)

      expect(response.toolCalls).toHaveLength(1)
      expect(response.toolCalls[0]).toEqual({
        id: 'call_1',
        name: 'get_weather',
        input: { city: 'Tokyo', units: 'celsius' },
      })
      expect(response.content).toBe('Got weather')
      expect(response.stopReason).toBe('tool_use')
    })

    it('uses stopReason heuristic (tool_use when toolCalls present, end_turn otherwise)', async () => {
      const noToolEvents: StreamEvent[] = [textEvent('hello'), doneEvent()]
      const noToolStream = (async function* () {
        for (const ev of noToolEvents) yield ev
      })() as BoxStream
      const noToolResponse = await collectStream(noToolStream)
      expect(noToolResponse.stopReason).toBe('end_turn')

      const toolEvents: StreamEvent[] = [
        toolCallStart('call_1', 'func'),
        toolCallArgs('{}'),
        toolCallEnd(),
        doneEvent(),
      ]
      const toolStream = (async function* () {
        for (const ev of toolEvents) yield ev
      })() as BoxStream
      const toolResponse = await collectStream(toolStream)
      expect(toolResponse.stopReason).toBe('tool_use')
    })

    it('sums cache tokens with lazy initialization', async () => {
      const events: StreamEvent[] = [
        usageEvent({
          inputTokens: 10,
          outputTokens: 5,
          cacheCreationInputTokens: 100,
        }),
        usageEvent({
          inputTokens: 5,
          outputTokens: 3,
          cacheReadInputTokens: 50,
        }),
        usageEvent({
          inputTokens: 2,
          outputTokens: 1,
          cacheCreationInputTokens: 20,
        }),
        doneEvent(),
      ]
      const stream = (async function* () {
        for (const ev of events) yield ev
      })() as BoxStream

      const response = await collectStream(stream)

      expect(response.usage.inputTokens).toBe(17)
      expect(response.usage.outputTokens).toBe(9)
      expect(response.usage.cacheCreationInputTokens).toBe(120)
      expect(response.usage.cacheReadInputTokens).toBe(50)
    })
  })

  describe('constructor helpers', () => {
    it('toolCallArgsWithId includes id in event', () => {
      const event = toolCallArgsWithId('call_1', '{"key":"value"')
      expect(event).toEqual({
        content: '',
        done: false,
        eventType: 'tool_call_args',
        toolCallId: 'call_1',
        toolCallArgsDelta: '{"key":"value"',
      })
    })

    it('toolCallEnd sets eventType but no id', () => {
      const event = toolCallEnd()
      expect(event.eventType).toBe('tool_call_end')
      expect(event.toolCallId).toBeUndefined()
    })

    it('toolCallEndWithId includes id', () => {
      const event = toolCallEndWithId('call_2')
      expect(event.eventType).toBe('tool_call_end')
      expect(event.toolCallId).toBe('call_2')
    })

    it('thinkingDelta and thinkingDone set correct eventTypes', () => {
      const delta = thinkingDelta('chunk')
      const done = thinkingDone('full text')
      expect(delta.eventType).toBe('thinking_delta')
      expect(delta.content).toBe('chunk')
      expect(done.eventType).toBe('thinking_done')
      expect(done.content).toBe('full text')
    })
  })
})
