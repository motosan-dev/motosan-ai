import { describe, expect, it } from 'vitest'
import { collectModelStream } from '../src/stream.js'
import type { ModelStreamDelta } from '../src/types.js'

async function* deltas(items: ModelStreamDelta[]): AsyncGenerator<ModelStreamDelta> {
  for (const item of items) yield item
}

describe('collectModelStream', () => {
  it('preserves the completed freeform call and never lowers accumulated deltas', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'freeform_input', callId: 'call_js', delta: 'console.' },
        { type: 'freeform_input', callId: 'call_js', delta: 'log(1);' },
        {
          type: 'tool_call_done',
          call: { kind: 'freeform', id: 'call_js', name: 'exec', input: 'console.log(1);' },
        },
        { type: 'usage', usage: { inputTokens: 2, outputTokens: 3 } },
        { type: 'done', stopReason: 'tool_use' },
      ]),
    )

    expect(response.toolCalls).toEqual([
      { kind: 'freeform', id: 'call_js', name: 'exec', input: 'console.log(1);' },
    ])
    expect(response.stopReason).toBe('tool_use')
    expect(response.usage.outputTokens).toBe(3)
    expect(response.model).toBe('')
  })

  it('prefers thinking_done over accumulated thinking deltas', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'thinking_delta', delta: 'think ' },
        { type: 'thinking_delta', delta: 'hard' },
        { type: 'thinking_done', thinking: 'think hard' },
        { type: 'text', delta: 'answer' },
        { type: 'done', stopReason: 'end_turn' },
      ]),
    )
    expect(response.content).toBe('answer')
    expect(response.thinking).toBe('think hard')
  })

  it('falls back to concatenated thinking deltas when no thinking_done arrives', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'thinking_delta', delta: 'partial ' },
        { type: 'thinking_delta', delta: 'reasoning' },
        { type: 'done', stopReason: 'end_turn' },
      ]),
    )
    expect(response.thinking).toBe('partial reasoning')
  })

  it('drops an empty thinking_done payload entirely', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'thinking_delta', delta: 'ignored' },
        { type: 'thinking_done', thinking: '' },
        { type: 'done', stopReason: 'end_turn' },
      ]),
    )
    expect(response.thinking).toBeUndefined()
  })

  it('REPLACES usage rather than merging it', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'usage', usage: { inputTokens: 100, outputTokens: 100, cacheReadInputTokens: 9 } },
        { type: 'usage', usage: { inputTokens: 1, outputTokens: 2 } },
        { type: 'done', stopReason: 'end_turn' },
      ]),
    )
    expect(response.usage).toEqual({ inputTokens: 1, outputTokens: 2 })
  })

  it('stops at the terminal done and ignores anything after it', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'text', delta: 'kept' },
        { type: 'done', stopReason: 'end_turn' },
        { type: 'text', delta: 'dropped' },
      ]),
    )
    expect(response.content).toBe('kept')
  })

  it('infers tool_use when no done delta carried a stop reason', async () => {
    const response = await collectModelStream(
      deltas([
        {
          type: 'tool_call_done',
          call: { kind: 'function', id: 'call_1', name: 'f', arguments: '{}' },
        },
      ]),
    )
    expect(response.stopReason).toBe('tool_use')
  })

  it('infers end_turn for a bare text stream with no done delta', async () => {
    const response = await collectModelStream(deltas([{ type: 'text', delta: 'hi' }]))
    expect(response.stopReason).toBe('end_turn')
  })

  it('propagates a mid-stream error instead of returning a partial response', async () => {
    async function* failing(): AsyncGenerator<ModelStreamDelta> {
      yield { type: 'text', delta: 'partial' }
      throw new Error('boom')
    }
    await expect(collectModelStream(failing())).rejects.toThrow('boom')
  })
})
