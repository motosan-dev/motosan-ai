import { describe, expect, it } from 'vitest'
import type {
  ChatRequest,
  ChatResponse,
  ContentBlock,
  DocumentSource,
  ImageSource,
  Message,
  StopReason,
  StreamEvent,
  StreamEventType,
  Tool,
  ToolCall,
  ToolChoice,
  Usage,
} from '../src/types.js'

// JSON roundtrip helper: structural equality after a serialize/parse cycle.
const roundtrip = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T

describe('ContentBlock variants roundtrip', () => {
  it('text block', () => {
    const block: ContentBlock = { type: 'text', text: 'hello' }
    expect(roundtrip(block)).toEqual(block)
  })

  it('image base64 + url sources', () => {
    const b64: ImageSource = { type: 'base64', mediaType: 'image/png', data: 'AAAA' }
    const url: ImageSource = { type: 'url', url: 'https://example.com/x.png' }
    const imgB64: ContentBlock = { type: 'image', source: b64 }
    const imgUrl: ContentBlock = { type: 'image', source: url }
    expect(roundtrip(imgB64)).toEqual(imgB64)
    expect(roundtrip(imgUrl)).toEqual(imgUrl)
  })

  it('document base64 + url sources', () => {
    const b64: DocumentSource = { type: 'base64', mediaType: 'application/pdf', data: 'JVBERi0' }
    const url: DocumentSource = { type: 'url', url: 'https://example.com/d.pdf' }
    const docB64: ContentBlock = { type: 'document', source: b64 }
    const docUrl: ContentBlock = { type: 'document', source: url }
    expect(roundtrip(docB64)).toEqual(docB64)
    expect(roundtrip(docUrl)).toEqual(docUrl)
  })
})

describe('Message with contentBlocks roundtrips', () => {
  it('preserves blocks, content, and tool fields', () => {
    const tc: ToolCall = { id: 'call_1', name: 'get_weather', input: { city: 'Taipei' } }
    const msg: Message = {
      role: 'user',
      content: 'look at this',
      contentBlocks: [
        { type: 'text', text: 'look at this' },
        { type: 'image', source: { type: 'base64', mediaType: 'image/png', data: 'AAAA' } },
      ],
      toolCalls: [tc],
      cache: true,
    }
    expect(roundtrip(msg)).toEqual(msg)
  })
})

describe('StreamEvent for each eventType', () => {
  const cases: Array<{ kind: StreamEventType; event: StreamEvent }> = [
    { kind: 'text', event: { content: 'hi', done: false, eventType: 'text' } },
    {
      kind: 'tool_call_start',
      event: { content: '', done: false, eventType: 'tool_call_start', toolCallId: 't1', toolCallName: 'f' },
    },
    {
      kind: 'tool_call_args',
      event: { content: '', done: false, eventType: 'tool_call_args', toolCallArgsDelta: '{"a":' },
    },
    { kind: 'tool_call_end', event: { content: '', done: false, eventType: 'tool_call_end' } },
    {
      kind: 'usage',
      event: { content: '', done: false, eventType: 'usage', usage: { inputTokens: 1, outputTokens: 2 } },
    },
    { kind: 'thinking_delta', event: { content: 'mm', done: false, eventType: 'thinking_delta' } },
    { kind: 'thinking_done', event: { content: 'done', done: false, eventType: 'thinking_done' } },
  ]

  for (const { kind, event } of cases) {
    it(`roundtrips ${kind}`, () => {
      expect(roundtrip(event)).toEqual(event)
      expect(event.eventType).toBe(kind)
    })
  }

  it('terminal done event carries a stopReason', () => {
    const done: StreamEvent = { content: '', done: true, eventType: 'text', stopReason: 'end_turn' }
    expect(roundtrip(done)).toEqual(done)
  })
})

describe('Usage with and without cache tokens', () => {
  it('without cache tokens omits the optional keys', () => {
    const usage: Usage = { inputTokens: 10, outputTokens: 5 }
    const json = JSON.parse(JSON.stringify(usage))
    expect(json).toEqual({ inputTokens: 10, outputTokens: 5 })
    expect('cacheCreationInputTokens' in json).toBe(false)
    expect('cacheReadInputTokens' in json).toBe(false)
  })

  it('with cache tokens preserves them', () => {
    const usage: Usage = {
      inputTokens: 10,
      outputTokens: 5,
      cacheCreationInputTokens: 3,
      cacheReadInputTokens: 7,
    }
    expect(roundtrip(usage)).toEqual(usage)
  })
})

describe('undefined optionals are omitted by JSON serialization', () => {
  it('StreamEvent: only required keys survive when optionals are unset', () => {
    const event: StreamEvent = { content: 'x', done: false, eventType: 'text' }
    const json = JSON.parse(JSON.stringify(event))
    expect(json).toEqual({ content: 'x', done: false, eventType: 'text' })
    for (const k of ['toolCallId', 'toolCallName', 'toolCallArgsDelta', 'usage', 'stopReason']) {
      expect(k in json).toBe(false)
    }
  })

  it('Message: optional blocks/tool fields/cache omitted', () => {
    const msg: Message = { role: 'assistant', content: 'ok' }
    const json = JSON.parse(JSON.stringify(msg))
    expect(json).toEqual({ role: 'assistant', content: 'ok' })
    for (const k of ['contentBlocks', 'toolCallId', 'toolCalls', 'cache']) {
      expect(k in json).toBe(false)
    }
  })

  it('ChatResponse: thinking omitted when unset', () => {
    const resp: ChatResponse = {
      content: 'hi',
      toolCalls: [],
      model: 'm',
      usage: { inputTokens: 1, outputTokens: 1 },
      stopReason: 'end_turn',
    }
    const json = JSON.parse(JSON.stringify(resp))
    expect('thinking' in json).toBe(false)
  })

  it('ChatRequest: minimal request omits every optional', () => {
    const req: ChatRequest = { messages: [{ role: 'user', content: 'hi' }] }
    const json = JSON.parse(JSON.stringify(req))
    expect(Object.keys(json)).toEqual(['messages'])
  })
})

describe('Tool, ToolChoice, StopReason shapes', () => {
  it('Tool optional fields omitted', () => {
    const tool: Tool = { name: 'f' }
    const json = JSON.parse(JSON.stringify(tool))
    expect(json).toEqual({ name: 'f' })
  })

  it('ToolChoice variants roundtrip', () => {
    const choices: ToolChoice[] = [{ type: 'auto' }, { type: 'required' }, { type: 'none' }, { type: 'tool', name: 'f' }]
    for (const c of choices) expect(roundtrip(c)).toEqual(c)
  })

  it('StopReason union values are usable', () => {
    const reasons: StopReason[] = ['end_turn', 'max_tokens', 'tool_use', 'stop', 'stop_sequence', 'other']
    expect(reasons).toContain('tool_use')
  })
})
