import { describe, expect, it } from 'vitest'
import { StreamReadTimeoutError, UnsupportedFeatureError } from '../src/error.js'
import {
  dispatchModelChat,
  dispatchModelStream,
  readTimeoutModelStream,
  textOnly,
  validateModelRequest,
  withFreeformTools,
  withImage,
  withImageAndFreeformTools,
  type ProviderImpl,
} from '../src/provider.js'
import { collectModelStream } from '../src/stream.js'
import type { ModelChatRequest, ModelStreamDelta } from '../src/types.js'

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

const FREEFORM_SPEC_REQUEST: ModelChatRequest = {
  context: [{ kind: 'message', message: { role: 'user', content: 'run js' } }],
  toolSpecs: [
    {
      kind: 'freeform',
      tool: {
        name: 'exec',
        description: 'Run JavaScript',
        format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
      },
    },
  ],
}

describe('freeform capability factories (D5)', () => {
  it('withFreeformTools() is text-only plus freeform', () => {
    expect(withFreeformTools()).toEqual({
      supportsImage: false,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: true,
    })
  })

  it('withImageAndFreeformTools() adds images', () => {
    expect(withImageAndFreeformTools()).toEqual({
      supportsImage: true,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: true,
    })
  })

  it('the pre-existing factories keep freeform false', () => {
    expect(textOnly().supportsFreeformTools).toBe(false)
    expect(withImage().supportsFreeformTools).toBe(false)
  })
})

describe('validateModelRequest', () => {
  it('rejects a freeform tool spec on a non-freeform provider', () => {
    expect(() => validateModelRequest(FREEFORM_SPEC_REQUEST, withImage())).toThrow(
      UnsupportedFeatureError,
    )
    expect(() => validateModelRequest(FREEFORM_SPEC_REQUEST, withImage())).toThrow(
      'provider does not support native freeform tools',
    )
  })

  it('rejects freeform history even without a freeform spec', () => {
    const withCall: ModelChatRequest = {
      context: [
        { kind: 'toolCall', call: { kind: 'freeform', id: 'c', name: 'exec', input: 'x' } },
      ],
    }
    const withOutput: ModelChatRequest = {
      context: [{ kind: 'toolOutput', output: { kind: 'custom', callId: 'c', output: 'x' } }],
    }
    expect(() => validateModelRequest(withCall, withImage())).toThrow(UnsupportedFeatureError)
    expect(() => validateModelRequest(withOutput, withImage())).toThrow(UnsupportedFeatureError)
  })

  it('accepts freeform on a freeform-capable provider', () => {
    expect(() => validateModelRequest(FREEFORM_SPEC_REQUEST, withFreeformTools())).not.toThrow()
  })

  it('accepts function specs and function history everywhere', () => {
    const req: ModelChatRequest = {
      context: [
        { kind: 'toolCall', call: { kind: 'function', id: 'c', name: 'f', arguments: '{}' } },
        { kind: 'toolOutput', output: { kind: 'function', callId: 'c', output: 'ok' } },
      ],
      toolSpecs: [{ kind: 'function', tool: { name: 'f', description: 'F', inputSchema: {} } }],
    }
    expect(() => validateModelRequest(req, textOnly())).not.toThrow()
  })

  it('rejects image blocks in context on a text-only provider', () => {
    const req: ModelChatRequest = {
      context: [
        {
          kind: 'message',
          message: {
            role: 'user',
            content: '',
            contentBlocks: [
              { type: 'image', source: { type: 'url', url: 'https://e.example/a.png' } },
            ],
          },
        },
      ],
    }
    expect(() => validateModelRequest(req, withFreeformTools())).toThrow(
      'provider does not support image input',
    )
    expect(() => validateModelRequest(req, withImageAndFreeformTools())).not.toThrow()
  })

  it('rejects document blocks in context', () => {
    const req: ModelChatRequest = {
      context: [
        {
          kind: 'message',
          message: {
            role: 'user',
            content: '',
            contentBlocks: [
              { type: 'document', source: { type: 'url', url: 'https://e.example/a.pdf' } },
            ],
          },
        },
      ],
    }
    expect(() => validateModelRequest(req, withImageAndFreeformTools())).toThrow(
      'provider does not support document input',
    )
  })
})

describe('native dispatch', () => {
  const bareProvider: ProviderImpl = {
    capabilities: withFreeformTools,
    chat: async () => {
      throw new Error('unused')
    },
    stream: () => {
      throw new Error('unused')
    },
  }

  it('rejects modelChat on a provider that implements no native surface', async () => {
    await expect(dispatchModelChat(bareProvider, { context: [] })).rejects.toThrow(
      'provider does not support native model requests',
    )
  })

  it('rejects modelStream on a provider that implements no native surface', () => {
    expect(() => dispatchModelStream(bareProvider, { context: [] })).toThrow(
      'provider does not support native model streams',
    )
  })

  it('validates BEFORE consulting the native surface', async () => {
    const imageOnly: ProviderImpl = { ...bareProvider, capabilities: withImage }
    await expect(dispatchModelChat(imageOnly, FREEFORM_SPEC_REQUEST)).rejects.toThrow(
      'provider does not support native freeform tools',
    )
  })
})

describe('readTimeoutModelStream', () => {
  it('passes deltas through untouched', async () => {
    const out: ModelStreamDelta[] = []
    for await (const delta of readTimeoutModelStream(
      deltas([{ type: 'text', delta: 'a' }, { type: 'done', stopReason: 'end_turn' }]),
      5,
    )) {
      out.push(delta)
    }
    expect(out).toEqual([{ type: 'text', delta: 'a' }, { type: 'done', stopReason: 'end_turn' }])
  })

  it('throws StreamReadTimeoutError on an idle gap and fabricates no done', async () => {
    async function* stalls(): AsyncGenerator<ModelStreamDelta> {
      yield { type: 'text', delta: 'tick' }
      await new Promise((resolve) => setTimeout(resolve, 500))
      yield { type: 'done', stopReason: 'end_turn' }
    }

    const out: ModelStreamDelta[] = []
    let caught: unknown
    try {
      for await (const delta of readTimeoutModelStream(stalls(), 0.05)) out.push(delta)
    } catch (error) {
      caught = error
    }
    expect(caught).toBeInstanceOf(StreamReadTimeoutError)
    expect(out).toEqual([{ type: 'text', delta: 'tick' }])
  })
})
