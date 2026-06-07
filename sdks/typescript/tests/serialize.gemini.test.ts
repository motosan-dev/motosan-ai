import { describe, it, expect } from 'vitest'
import { serializeGeminiRequest } from '../src/serialize/gemini.js'
import type { ChatRequest } from '../src/types.js'

const MODEL = 'gemini-2.5-flash'

describe('serializeGeminiRequest — contents & role mapping', () => {
  it('serializes a simple user message (gemini.rs:565-569)', () => {
    const req: ChatRequest = { messages: [{ role: 'user', content: 'Hello' }] }
    const body = serializeGeminiRequest(req, MODEL)
    const contents = body.contents as any[]
    expect(contents[0].role).toBe('user')
    expect(contents[0].parts[0].text).toBe('Hello')
  })

  it('maps assistant role to "model" (gemini.rs:571-576)', () => {
    const req: ChatRequest = {
      messages: [
        { role: 'user', content: 'Hi' },
        { role: 'assistant', content: 'Hello back' },
      ],
    }
    const body = serializeGeminiRequest(req, MODEL)
    const contents = body.contents as any[]
    expect(contents[1].role).toBe('model')
    expect(contents[1].parts[0].text).toBe('Hello back')
  })

  it('serializes assistant text + tool calls; functionCall uses wire field "args" (gemini.rs:914-928)', () => {
    const req: ChatRequest = {
      messages: [
        { role: 'user', content: 'go' },
        {
          role: 'assistant',
          content: 'Let me check.',
          toolCalls: [{ id: 'c1', name: 'foo', input: { x: 1 } }],
        },
      ],
    }
    const body = serializeGeminiRequest(req, MODEL)
    const parts = (body.contents as any[])[1].parts
    expect((body.contents as any[])[1].role).toBe('model')
    expect(parts[0].text).toBe('Let me check.')
    expect(parts[1].functionCall.name).toBe('foo')
    expect(parts[1].functionCall.args.x).toBe(1)
  })

  it('does NOT place model in the body (URL path owns it)', () => {
    const req: ChatRequest = { messages: [{ role: 'user', content: 'hi' }] }
    const body = serializeGeminiRequest(req, MODEL)
    expect(body.model).toBeUndefined()
  })
})

describe('serializeGeminiRequest — systemInstruction (separate top-level field)', () => {
  it('extracts a system MESSAGE to systemInstruction; contents excludes it (gemini.rs:578-587)', () => {
    const req: ChatRequest = {
      messages: [
        { role: 'system', content: 'Be concise.' },
        { role: 'user', content: 'Hi' },
      ],
    }
    const body = serializeGeminiRequest(req, MODEL)
    expect((body.systemInstruction as any).parts[0].text).toBe('Be concise.')
    expect((body.contents as any[]).length).toBe(1)
    expect((body.contents as any[])[0].role).toBe('user')
  })

  it('uses req.system when set (no system message)', () => {
    const req: ChatRequest = { messages: [{ role: 'user', content: 'Hi' }], system: 'Be brief.' }
    const body = serializeGeminiRequest(req, MODEL)
    expect((body.systemInstruction as any).parts[0].text).toBe('Be brief.')
  })

  it('joins systemBlocks with \\n (gemini.rs:870-883)', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      systemBlocks: [{ text: 'Block 1.' }, { text: 'Block 2.' }],
    }
    const body = serializeGeminiRequest(req, MODEL)
    expect((body.systemInstruction as any).parts[0].text).toBe('Block 1.\nBlock 2.')
  })

  it('systemBlocks take priority over system field (gemini.rs:885-898)', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      system: 'fallback',
      systemBlocks: [{ text: 'from blocks' }],
    }
    const body = serializeGeminiRequest(req, MODEL)
    const text = (body.systemInstruction as any).parts[0].text
    expect(text).toBe('from blocks')
    expect(text).not.toContain('fallback')
  })

  it('empty systemBlocks falls back to system field (gemini.rs:900-912)', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      system: 'use this',
      systemBlocks: [],
    }
    const body = serializeGeminiRequest(req, MODEL)
    expect((body.systemInstruction as any).parts[0].text).toBe('use this')
  })

  it('omits systemInstruction entirely when empty', () => {
    const req: ChatRequest = { messages: [{ role: 'user', content: 'hi' }] }
    const body = serializeGeminiRequest(req, MODEL)
    expect(body.systemInstruction).toBeUndefined()
  })
})

describe('serializeGeminiRequest — image content blocks', () => {
  it('base64 image -> inlineData.mimeType/data (gemini.rs:712-725)', () => {
    const req: ChatRequest = {
      messages: [
        {
          role: 'user',
          content: 'look at this',
          contentBlocks: [
            { type: 'image', source: { type: 'base64', mediaType: 'image/png', data: 'abc123' } },
          ],
        },
      ],
    }
    const body = serializeGeminiRequest(req, MODEL)
    const parts = (body.contents as any[])[0].parts
    // content 'look at this' becomes parts[0]; image is parts[1]
    const img = parts.find((p: any) => p.inlineData)
    expect(img.inlineData.mimeType).toBe('image/png')
    expect(img.inlineData.data).toBe('abc123')
  })

  it('url image -> fileData.fileUri (gemini.rs:727-737)', () => {
    const req: ChatRequest = {
      messages: [
        {
          role: 'user',
          content: '',
          contentBlocks: [
            { type: 'image', source: { type: 'url', url: 'https://example.com/img.jpg' } },
          ],
        },
      ],
    }
    const body = serializeGeminiRequest(req, MODEL)
    const part = (body.contents as any[])[0].parts[0]
    expect(part.fileData.fileUri).toBe('https://example.com/img.jpg')
  })

  it('throws on a document block (defensive — validateRequest rejects first) (gemini.rs:112)', () => {
    const req: ChatRequest = {
      messages: [
        {
          role: 'user',
          content: '',
          contentBlocks: [
            { type: 'document', source: { type: 'url', url: 'https://example.com/doc.pdf' } },
          ],
        },
      ],
    }
    expect(() => serializeGeminiRequest(req, MODEL)).toThrow(
      'Gemini does not support document content blocks',
    )
  })
})

describe('serializeGeminiRequest — tool (functionResponse) messages', () => {
  it('tool message -> role:user functionResponse; toolCallId is the NAME; JSON content parsed (gemini.rs:589-596)', () => {
    const req: ChatRequest = {
      messages: [
        { role: 'user', content: '?' },
        { role: 'tool', toolCallId: 'get_weather', content: '{"result": "sunny"}' },
      ],
    }
    const body = serializeGeminiRequest(req, MODEL)
    const part = (body.contents as any[])[1].parts[0]
    expect((body.contents as any[])[1].role).toBe('user')
    expect(part.functionResponse.name).toBe('get_weather')
    expect(part.functionResponse.response.result).toBe('sunny')
  })

  it('non-JSON tool content wraps as { result } (gemini.rs:599-605)', () => {
    const req: ChatRequest = {
      messages: [
        { role: 'user', content: '?' },
        { role: 'tool', toolCallId: '', content: 'done' },
      ],
    }
    const body = serializeGeminiRequest(req, MODEL)
    const part = (body.contents as any[])[1].parts[0]
    expect(part.functionResponse.name).toBe('')
    expect(part.functionResponse.response.result).toBe('done')
  })
})

describe('serializeGeminiRequest — generationConfig', () => {
  it('always emits maxOutputTokens, defaulting to 8192 (gemini.rs:174-175)', () => {
    const req: ChatRequest = { messages: [{ role: 'user', content: 'hi' }] }
    const body = serializeGeminiRequest(req, MODEL)
    expect((body.generationConfig as any).maxOutputTokens).toBe(8192)
  })

  it('maxTokens maps to maxOutputTokens (gemini.rs:847-855)', () => {
    const req: ChatRequest = { messages: [{ role: 'user', content: 'hi' }], maxTokens: 256 }
    const body = serializeGeminiRequest(req, MODEL)
    expect((body.generationConfig as any).maxOutputTokens).toBe(256)
  })

  it('temperature emitted only when set (gemini.rs:645-656)', () => {
    const withTemp = serializeGeminiRequest(
      { messages: [{ role: 'user', content: 'hi' }], temperature: 0.3 },
      MODEL,
    )
    expect((withTemp.generationConfig as any).temperature).toBeCloseTo(0.3, 6)
    const without = serializeGeminiRequest({ messages: [{ role: 'user', content: 'hi' }] }, MODEL)
    expect((without.generationConfig as any).temperature).toBeUndefined()
  })

  it('non-empty stopSequences emitted (gemini.rs:835-845)', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      stopSequences: ['END', 'STOP'],
    }
    const body = serializeGeminiRequest(req, MODEL)
    expect((body.generationConfig as any).stopSequences).toEqual(['END', 'STOP'])
  })

  it('does NOT emit thinkingConfig (parity — gemini.rs has none)', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      thinking: { budgetTokens: 1024 },
    }
    const body = serializeGeminiRequest(req, MODEL)
    expect((body.generationConfig as any).thinkingConfig).toBeUndefined()
  })
})

describe('serializeGeminiRequest — tools & toolConfig', () => {
  const tool = (name: string, description = '', inputSchema?: Record<string, unknown>) => ({
    name,
    description,
    inputSchema,
  })

  it('emits functionDeclarations with name + parameters (gemini.rs:740-791)', () => {
    const schema = { type: 'object', properties: { q: { type: 'string' } }, required: ['q'] }
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'find' }],
      tools: [tool('search', '', schema)],
    }
    const body = serializeGeminiRequest(req, MODEL)
    const decls = (body.tools as any[])[0].functionDeclarations
    expect(decls[0].name).toBe('search')
    expect(decls[0].parameters).toEqual(schema)
  })

  it('defaults missing description to "" and missing schema to {type:object,properties:{}} (serialize/openai.ts:151-152)', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      tools: [{ name: 'bare' }],
    }
    const body = serializeGeminiRequest(req, MODEL)
    const decl = (body.tools as any[])[0].functionDeclarations[0]
    expect(decl.description).toBe('')
    expect(decl.parameters).toEqual({ type: 'object', properties: {} })
  })

  it('multiple tools become multiple declarations (gemini.rs:740-768)', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      tools: [tool('search', 'Search'), tool('calc', 'Calculate')],
    }
    const body = serializeGeminiRequest(req, MODEL)
    const decls = (body.tools as any[])[0].functionDeclarations
    expect(decls.length).toBe(2)
    expect(decls[0].name).toBe('search')
    expect(decls[1].name).toBe('calc')
  })

  it('toolChoice undefined -> AUTO (gemini.rs:210)', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      tools: [tool('t')],
    }
    const body = serializeGeminiRequest(req, MODEL)
    expect((body.toolConfig as any).functionCallingConfig.mode).toBe('AUTO')
  })

  it('toolChoice auto -> AUTO (gemini.rs:794-810)', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      tools: [tool('t')],
      toolChoice: { type: 'auto' },
    }
    const body = serializeGeminiRequest(req, MODEL)
    expect((body.toolConfig as any).functionCallingConfig.mode).toBe('AUTO')
  })

  it('toolChoice required -> ANY (gemini.rs:607-624)', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'find it' }],
      tools: [tool('search', 'Search', { type: 'object', properties: {} })],
      toolChoice: { type: 'required' },
    }
    const body = serializeGeminiRequest(req, MODEL)
    expect((body.toolConfig as any).functionCallingConfig.mode).toBe('ANY')
  })

  it('toolChoice tool(name) -> ANY + allowedFunctionNames (gemini.rs:812-833)', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      tools: [tool('special')],
      toolChoice: { type: 'tool', name: 'special' },
    }
    const body = serializeGeminiRequest(req, MODEL)
    const fc = (body.toolConfig as any).functionCallingConfig
    expect(fc.mode).toBe('ANY')
    expect(fc.allowedFunctionNames).toEqual(['special'])
  })

  it('toolChoice none REMOVES tools and emits NO toolConfig (gemini.rs:627-643)', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      tools: [tool('search')],
      toolChoice: { type: 'none' },
    }
    const body = serializeGeminiRequest(req, MODEL)
    expect(body.tools).toBeUndefined()
    expect(body.toolConfig).toBeUndefined()
  })

  it('empty tools omits both tools and toolConfig (gemini.rs:930-939)', () => {
    const req: ChatRequest = { messages: [{ role: 'user', content: 'hi' }], tools: [] }
    const body = serializeGeminiRequest(req, MODEL)
    expect(body.tools).toBeUndefined()
    expect(body.toolConfig).toBeUndefined()
  })
})

describe('serializeGeminiRequest — providerOptions merge', () => {
  it('merges providerOptions at the top level last (gemini.rs:857-867)', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      providerOptions: { safetySettings: [{ category: 'ALL', threshold: 'BLOCK_NONE' }] },
    }
    const body = serializeGeminiRequest(req, MODEL)
    expect(body.safetySettings).toBeDefined()
  })
})
