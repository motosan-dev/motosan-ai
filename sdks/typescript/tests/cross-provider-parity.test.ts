import { describe, it, expect, vi, afterEach } from 'vitest'
import { AnthropicProvider } from '../src/providers/anthropic.js'
import { OpenAIProvider } from '../src/providers/openai.js'
import { GeminiProvider } from '../src/providers/gemini.js'
import { serializeAnthropicRequest } from '../src/serialize/anthropic.js'
import { serializeOpenAiRequest } from '../src/serialize/openai.js'
import { serializeGeminiRequest } from '../src/serialize/gemini.js'
import {
  DEFAULT_ANTHROPIC_MODEL,
  DEFAULT_OPENAI_MODEL,
  DEFAULT_GEMINI_MODEL,
} from '../src/models.js'
import type { ChatRequest, Tool } from '../src/types.js'

// Cross-provider parity SANITY checks (NOT a re-test of each serializer's full
// matrix — that lives in serialize.*.test.ts). One canonical request is fed
// through every serializer; one canonical response through every chat parse.

const TOOL: Tool = {
  name: 'get_weather',
  description: 'Get current weather',
  inputSchema: {
    type: 'object',
    properties: { city: { type: 'string' } },
    required: ['city'],
  },
}

const CANONICAL: ChatRequest = {
  messages: [{ role: 'user', content: "What's the weather in Tokyo?" }],
  system: 'You are a helpful assistant.',
  tools: [TOOL],
  toolChoice: { type: 'auto' },
}

// Return a JSON Response from mocked fetch.
function stubJsonFetch(payload: unknown): void {
  vi.stubGlobal(
    'fetch',
    vi.fn(
      async () =>
        new Response(JSON.stringify(payload), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        }),
    ),
  )
}

describe('cross-provider parity', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  describe('serialize-shape invariants', () => {
    it('Anthropic: top-level system + tools as [{ name, description, input_schema }]', () => {
      const body = serializeAnthropicRequest(CANONICAL, DEFAULT_ANTHROPIC_MODEL)
      // Anthropic system prompt is a top-level field (string when no cache).
      expect(body.system).toBe('You are a helpful assistant.')
      expect(Array.isArray(body.tools)).toBe(true)
      const tool = body.tools[0]
      expect(tool.name).toBe('get_weather')
      expect(tool.description).toBe('Get current weather')
      // snake_case input_schema (NOT camelCase, NOT `parameters`).
      expect(tool.input_schema).toEqual(TOOL.inputSchema)
      expect(tool.parameters).toBeUndefined()
      expect(body.tool_choice).toEqual({ type: 'auto' })
    })

    it('OpenAI: role:system message + tools as [{ type:function, function:{ name, description, parameters } }]', () => {
      const body = serializeOpenAiRequest(CANONICAL, DEFAULT_OPENAI_MODEL)
      const messages = body.messages as Array<{ role: string; content: unknown }>
      // OpenAI system prompt is a role:system MESSAGE, not a top-level field.
      expect(body.system).toBeUndefined()
      const systemMsg = messages.find((m) => m.role === 'system')
      expect(systemMsg?.content).toBe('You are a helpful assistant.')
      const tools = body.tools as Array<{
        type: string
        function: { name: string; description: string; parameters: unknown }
      }>
      expect(Array.isArray(tools)).toBe(true)
      expect(tools[0].type).toBe('function')
      expect(tools[0].function.name).toBe('get_weather')
      expect(tools[0].function.description).toBe('Get current weather')
      expect(tools[0].function.parameters).toEqual(TOOL.inputSchema)
      expect(body.tool_choice).toBe('auto')
    })

    it('Gemini: contents array + tool defs under functionDeclarations w/ parameters', () => {
      const body = serializeGeminiRequest(CANONICAL, DEFAULT_GEMINI_MODEL)
      expect(Array.isArray(body.contents)).toBe(true)
      const contents = body.contents as Array<{ role: string }>
      // The user message lands in contents; the system goes to systemInstruction.
      expect(contents.some((c) => c.role === 'user')).toBe(true)
      const sysInstr = body.systemInstruction as { parts: Array<{ text: string }> }
      expect(sysInstr.parts[0].text).toBe('You are a helpful assistant.')
      const tools = body.tools as Array<{
        functionDeclarations: Array<{ name: string; description: string; parameters: unknown }>
      }>
      expect(Array.isArray(tools)).toBe(true)
      // Gemini emits `functionDeclarations` (camelCase), NOT `function_declarations`.
      const decls = tools[0].functionDeclarations
      expect(Array.isArray(decls)).toBe(true)
      expect(decls[0].name).toBe('get_weather')
      expect(decls[0].description).toBe('Get current weather')
      expect(decls[0].parameters).toEqual(TOOL.inputSchema)
    })
  })

  describe('parse-parity invariant (same ChatResponse surface)', () => {
    it('Anthropic chat yields content:string, toolCalls:array, usage tokens', async () => {
      stubJsonFetch({
        id: 'msg_1',
        type: 'message',
        role: 'assistant',
        content: [{ type: 'text', text: 'Sunny' }],
        model: 'claude-sonnet-4-6',
        stop_reason: 'end_turn',
        usage: { input_tokens: 11, output_tokens: 3 },
      })
      const provider = new AnthropicProvider('sk-ant-api03-x', DEFAULT_ANTHROPIC_MODEL)
      const resp = await provider.chat(CANONICAL)
      expect(typeof resp.content).toBe('string')
      expect(resp.content).toBe('Sunny')
      expect(Array.isArray(resp.toolCalls)).toBe(true)
      expect(resp.usage.inputTokens).toBe(11)
      expect(resp.usage.outputTokens).toBe(3)
    })

    it('OpenAI chat yields content:string, toolCalls:array, usage tokens', async () => {
      stubJsonFetch({
        id: 'cmpl_1',
        model: 'gpt-4o',
        choices: [
          { index: 0, message: { role: 'assistant', content: 'Sunny' }, finish_reason: 'stop' },
        ],
        usage: { prompt_tokens: 9, completion_tokens: 2 },
      })
      const provider = new OpenAIProvider('sk-test', DEFAULT_OPENAI_MODEL)
      const resp = await provider.chat(CANONICAL)
      expect(typeof resp.content).toBe('string')
      expect(resp.content).toBe('Sunny')
      expect(Array.isArray(resp.toolCalls)).toBe(true)
      expect(resp.usage.inputTokens).toBe(9)
      expect(resp.usage.outputTokens).toBe(2)
    })

    it('Gemini chat yields content:string, toolCalls:array, usage tokens', async () => {
      stubJsonFetch({
        candidates: [
          {
            content: { parts: [{ text: 'Sunny' }], role: 'model' },
            finishReason: 'STOP',
          },
        ],
        usageMetadata: { promptTokenCount: 7, candidatesTokenCount: 1 },
      })
      const provider = new GeminiProvider('gkey', DEFAULT_GEMINI_MODEL)
      const resp = await provider.chat(CANONICAL)
      expect(typeof resp.content).toBe('string')
      expect(resp.content).toBe('Sunny')
      expect(Array.isArray(resp.toolCalls)).toBe(true)
      expect(resp.usage.inputTokens).toBe(7)
      expect(resp.usage.outputTokens).toBe(1)
    })
  })
})
