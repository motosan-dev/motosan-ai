import { describe, expect, it } from 'vitest'
import {
  ChatGptCodexProvider,
  DEFAULT_CHATGPT_CODEX_MODEL,
} from '../src/providers/chatgpt_codex.js'
import type { ChatRequest } from '../src/types.js'

function p(): ChatGptCodexProvider {
  return new ChatGptCodexProvider('tok', 'acct')
}

describe('ChatGptCodexProvider buildResponsesBody', () => {
  it('uses default model gpt-5.5 and default base URL', () => {
    const prov = p()
    const body = prov.buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, prov.modelId())
    expect(DEFAULT_CHATGPT_CODEX_MODEL).toBe('gpt-5.5')
    expect(body.model).toBe('gpt-5.5')
    expect(prov.endpointUrl()).toBe('https://chatgpt.com/backend-api/codex/responses')
  })

  it('per-request model overrides the default', () => {
    const prov = p()
    const req: ChatRequest = { messages: [{ role: 'user', content: 'hi' }], model: 'gpt-x' }
    const body = prov.buildResponsesBody(req, req.model ?? prov.modelId())
    expect(body.model).toBe('gpt-x')
  })

  it('sets the required codex fields and a single input_text user item', () => {
    const body = p().buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, 'gpt-5.5')
    expect(body.store).toBe(false)
    expect(body.stream).toBe(true)
    expect(typeof body.instructions).toBe('string')
    expect(Array.isArray(body.input)).toBe(true)
    expect(body.include).toEqual(['reasoning.encrypted_content'])
    expect(body.tool_choice).toBe('auto')
    expect(body.parallel_tool_calls).toBe(true)
    expect(body.input).toHaveLength(1)
    expect(body.input[0]).toEqual({
      type: 'message',
      role: 'user',
      content: [{ type: 'input_text', text: 'hi' }],
    })
    expect(body.tools).toBeUndefined()
    expect(body.reasoning).toBeUndefined()
    expect(body.temperature).toBeUndefined()
  })

  it('falls back to the default instructions when nothing is supplied', () => {
    const body = p().buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, 'gpt-5.5')
    expect(body.instructions).toBe('You are a helpful assistant.')
  })

  it('routes a system message to instructions, not input', () => {
    const req: ChatRequest = {
      messages: [
        { role: 'system', content: 'be terse' },
        { role: 'user', content: 'hi' },
      ],
    }
    const body = p().buildResponsesBody(req, 'gpt-5.5')
    expect(body.instructions).toBe('be terse')
    expect(body.input).toHaveLength(1)
    expect(body.input[0].role).toBe('user')
  })

  it('uses the system field for instructions', () => {
    const body = p().buildResponsesBody(
      { messages: [{ role: 'user', content: 'hi' }], system: 'sys here' },
      'gpt-5.5',
    )
    expect(body.instructions).toBe('sys here')
  })

  it('prefers systemBlocks over system, joined with \\n\\n', () => {
    const body = p().buildResponsesBody(
      {
        messages: [{ role: 'user', content: 'hi' }],
        system: 'ignored',
        systemBlocks: [{ text: 'a' }, { text: '  ' }, { text: 'b' }],
      },
      'gpt-5.5',
    )
    expect(body.instructions).toBe('a\n\nb')
  })

  it('emits an output_text item for assistant text', () => {
    const body = p().buildResponsesBody(
      { messages: [{ role: 'assistant', content: 'prior answer' }] },
      'gpt-5.5',
    )
    expect(body.input[0]).toEqual({
      type: 'message',
      role: 'assistant',
      content: [{ type: 'output_text', text: 'prior answer' }],
    })
  })

  it('emits function_call + function_call_output for a tool round trip', () => {
    const req: ChatRequest = {
      messages: [
        {
          role: 'assistant',
          content: '',
          toolCalls: [{ id: 'call_1', name: 'get_weather', input: { city: 'Paris' } }],
        },
        { role: 'tool', content: '{"temp":20}', toolCallId: 'call_1' },
      ],
    }
    const body = p().buildResponsesBody(req, 'gpt-5.5')
    expect(body.input[0]).toEqual({
      type: 'function_call',
      call_id: 'call_1',
      name: 'get_weather',
      arguments: '{"city":"Paris"}',
    })
    expect(body.input[1]).toEqual({
      type: 'function_call_output',
      call_id: 'call_1',
      output: '{"temp":20}',
    })
  })

  it('maps tools to the flat Responses shape with strict:null', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      tools: [{ name: 'get_weather', description: 'gets weather', inputSchema: { type: 'object' } }],
    }
    const body = p().buildResponsesBody(req, 'gpt-5.5')
    expect(body.tools).toEqual([
      {
        type: 'function',
        name: 'get_weather',
        description: 'gets weather',
        parameters: { type: 'object' },
        strict: null,
      },
    ])
  })

  it('omits the tools key for an empty tools list', () => {
    const body = p().buildResponsesBody(
      { messages: [{ role: 'user', content: 'hi' }], tools: [] },
      'gpt-5.5',
    )
    expect(body.tools).toBeUndefined()
  })

  it('emits reasoning and temperature when supplied', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      temperature: 0.3,
      providerOptions: { reasoning_effort: 'high' },
    }
    const body = p().buildResponsesBody(req, 'gpt-5.5')
    expect(body.reasoning).toEqual({ effort: 'high', summary: 'auto' })
    expect(body.temperature).toBe(0.3)
  })

  it('omits reasoning when the per-request effort is not a string', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      providerOptions: { reasoning_effort: 5 },
    }
    const body = p().buildResponsesBody(req, 'gpt-5.5')
    expect(body.reasoning).toBeUndefined()
  })

  it('emits the provider-default reasoning effort, overridden by a per-request value', () => {
    const def = p().reasoningEffort('medium')
    expect(def.buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, 'gpt-5.5').reasoning)
      .toEqual({ effort: 'medium', summary: 'auto' })
    // per-request wins
    const body = def.buildResponsesBody(
      { messages: [{ role: 'user', content: 'hi' }], providerOptions: { reasoning_effort: 'high' } },
      'gpt-5.5',
    )
    expect(body.reasoning).toEqual({ effort: 'high', summary: 'auto' })
  })

  it('reasoningEffort(undefined) clears the default and the setter returns this', () => {
    const prov = p()
    expect(prov.reasoningEffort('high')).toBe(prov) // returns this
    prov.reasoningEffort(undefined)
    expect(prov.buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, 'gpt-5.5').reasoning)
      .toBeUndefined()
  })
})
