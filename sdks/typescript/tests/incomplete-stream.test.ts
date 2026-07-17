import { describe, it, expect, vi, afterEach } from 'vitest'
import * as sdk from '../src/index.js'
import { AnthropicProvider } from '../src/providers/anthropic.js'
import { GeminiProvider } from '../src/providers/gemini.js'
import { OllamaProvider } from '../src/providers/ollama.js'
import { MinimaxProvider } from '../src/providers/minimax.js'
import { ChatGptCodexProvider } from '../src/providers/chatgpt_codex.js'
import { collectStream } from '../src/stream.js'
import { IncompleteStreamError, MotosanError, StreamError } from '../src/error.js'
import type { ChatRequest, StreamEvent } from '../src/types.js'

const REQ: ChatRequest = { messages: [{ role: 'user', content: 'hi' }] }

function stubBodyFetch(transcript: string, contentType = 'text/event-stream'): void {
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(new TextEncoder().encode(transcript))
      controller.close()
    },
  })
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => new Response(body, { status: 200, headers: { 'content-type': contentType } })),
  )
}

async function drain(stream: AsyncIterable<StreamEvent>) {
  const events: StreamEvent[] = []
  let error: unknown
  try {
    for await (const e of stream) events.push(e)
  } catch (e) {
    error = e
  }
  return { events, error }
}

describe('IncompleteStreamError (E1)', () => {
  it('subclasses StreamError (migration softener) + MotosanError, and is exported from the package root', () => {
    const err = new IncompleteStreamError('incomplete stream: anthropic ended without a terminal event')
    expect(err).toBeInstanceOf(StreamError)
    expect(err).toBeInstanceOf(MotosanError)
    expect(err.name).toBe('IncompleteStreamError')
    expect(typeof sdk.IncompleteStreamError).toBe('function')
  })
})

describe('adapter EOF-without-terminal-event enforcement (E2/E3)', () => {
  afterEach(() => vi.unstubAllGlobals())

  const ANTHROPIC_PARTIAL =
    'event: content_block_delta\n' +
    'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}\n\n'

  const cases = [
    {
      name: 'gemini',
      transcript: 'data: {"candidates":[{"content":{"parts":[{"text":"partial"}],"role":"model"}}]}\n\n',
      contentType: 'text/event-stream',
      make: () => new GeminiProvider('k'),
    },
    {
      name: 'ollama',
      transcript: '{"message":{"content":"partial"},"done":false}\n',
      contentType: 'application/x-ndjson',
      make: () => new OllamaProvider('llama3.2', 'http://localhost:11434'),
    },
    {
      name: 'minimax',
      transcript: ANTHROPIC_PARTIAL,
      contentType: 'text/event-stream',
      make: () => new MinimaxProvider('key'),
    },
    {
      name: 'chatgpt_codex',
      transcript: 'data: {"type":"response.output_text.delta","delta":"partial"}\n\n',
      contentType: 'text/event-stream',
      make: () => new ChatGptCodexProvider('tok', 'acct'),
    },
  ]

  for (const c of cases) {
    it(`${c.name}: EOF after a text delta throws IncompleteStreamError (partial text already yielded)`, async () => {
      stubBodyFetch(c.transcript, c.contentType)
      const { events, error } = await drain(c.make().stream(REQ))
      expect(error).toBeInstanceOf(IncompleteStreamError)
      expect((error as Error).message).toBe(`incomplete stream: ${c.name} ended without a terminal event`)
      expect(events.filter((e) => e.eventType === 'text' && !e.done).map((e) => e.content)).toEqual(['partial'])
      expect(events.some((e) => e.done)).toBe(false)
    })
  }

  it('anthropic: a message_delta stop_reason without message_stop is still incomplete (only message_stop is terminal)', async () => {
    stubBodyFetch(
      ANTHROPIC_PARTIAL +
        'event: message_delta\n' +
        'data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}\n\n',
    )
    const { error } = await drain(new AnthropicProvider('key', 'claude-3-5-sonnet-20241022').stream(REQ))
    expect(error).toBeInstanceOf(IncompleteStreamError)
  })

  it('collectStream propagates IncompleteStreamError (no stop_reason fallback for truncation)', async () => {
    stubBodyFetch(ANTHROPIC_PARTIAL)
    const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
    await expect(collectStream(provider.stream(REQ))).rejects.toBeInstanceOf(IncompleteStreamError)
  })
})
