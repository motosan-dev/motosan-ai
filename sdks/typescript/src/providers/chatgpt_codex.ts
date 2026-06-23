/**
 * ChatGPT-Codex provider: streams the OpenAI Responses API at
 * `https://chatgpt.com/backend-api/codex/responses` using a caller-supplied
 * OAuth `accessToken` + `accountId` (codex CLI headers; no api key). Mirrors the
 * verified Python `chatgpt_codex.py` (a port of authoritative Rust
 * `chatgpt_codex.rs`) in idiomatic TS.
 */

import { DEFAULT_CHATGPT_CODEX_MODEL } from '../models.js'
import { textOnly, type ProviderCapabilities } from '../provider.js'
import { RetryPolicy } from '../retry.js'
import type { ChatRequest } from '../types.js'

export const DEFAULT_CHATGPT_CODEX_URL = 'https://chatgpt.com/backend-api/codex/responses'
const CHATGPT_CODEX_ORIGINATOR = 'codex_cli_rs'

// Re-exported here (load-bearing): the T1 unit test imports the default model
// from this provider module path.
export { DEFAULT_CHATGPT_CODEX_MODEL }

/**
 * No-api-key OAuth-Bearer HTTP provider over the OpenAI Responses API.
 * Constructor `(accessToken, accountId, model?, baseUrl?)` mirrors Python
 * `ChatGptCodexProvider.__init__`. Text-only capabilities.
 */
export class ChatGptCodexProvider {
  private readonly model: string
  private readonly baseUrl: string
  private retryPolicy: RetryPolicy
  private _reasoningEffort?: string

  constructor(
    private readonly accessToken: string,
    private readonly accountId: string,
    model?: string,
    baseUrl: string = DEFAULT_CHATGPT_CODEX_URL,
  ) {
    this.model = model ?? DEFAULT_CHATGPT_CODEX_MODEL
    this.baseUrl = baseUrl
    this.retryPolicy = RetryPolicy.default()
  }

  withRetryPolicy(policy: RetryPolicy): this {
    this.retryPolicy = policy
    return this
  }

  /** Set the provider-default reasoning effort. Pass undefined to clear. Returns this. */
  reasoningEffort(effort: string | undefined): this {
    this._reasoningEffort = effort
    return this
  }

  /** Test/introspection accessor: the resolved default model id. */
  modelId(): string {
    return this.model
  }

  /** The full POST endpoint (base URL verbatim). */
  endpointUrl(): string {
    return this.baseUrl
  }

  capabilities(): ProviderCapabilities {
    return textOnly()
  }

  private headers(): Record<string, string> {
    return {
      authorization: `Bearer ${this.accessToken}`,
      'chatgpt-account-id': this.accountId,
      originator: CHATGPT_CODEX_ORIGINATOR,
      'openai-beta': 'responses=experimental',
      accept: 'text/event-stream',
      'content-type': 'application/json',
    }
  }

  /** Build the OpenAI Responses request body. Public for unit tests. */
  buildResponsesBody(request: ChatRequest, model: string): Record<string, any> {
    const instructionsParts: string[] = []
    if (request.systemBlocks !== undefined) {
      for (const block of request.systemBlocks) {
        const trimmed = block.text.trim()
        if (trimmed) instructionsParts.push(trimmed)
      }
    } else if (request.system !== undefined) {
      const trimmed = request.system.trim()
      if (trimmed) instructionsParts.push(trimmed)
    }

    const inputItems: Array<Record<string, any>> = []
    for (const message of request.messages) {
      switch (message.role) {
        case 'system': {
          const trimmed = message.content.trim()
          if (trimmed) instructionsParts.push(trimmed)
          break
        }
        case 'user':
          inputItems.push({
            type: 'message',
            role: 'user',
            content: [{ type: 'input_text', text: message.content }],
          })
          break
        case 'assistant': {
          if (message.content) {
            inputItems.push({
              type: 'message',
              role: 'assistant',
              content: [{ type: 'output_text', text: message.content }],
            })
          }
          for (const tc of message.toolCalls ?? []) {
            inputItems.push({
              type: 'function_call',
              call_id: tc.id,
              name: tc.name,
              arguments: JSON.stringify(tc.input),
            })
          }
          break
        }
        case 'tool':
          if (message.toolCallId !== undefined) {
            inputItems.push({
              type: 'function_call_output',
              call_id: message.toolCallId,
              output: message.content,
            })
          }
          break
      }
    }

    const instructions =
      instructionsParts.length > 0 ? instructionsParts.join('\n\n') : 'You are a helpful assistant.'

    const body: Record<string, any> = {
      model,
      store: false,
      stream: true,
      instructions,
      input: inputItems,
      include: ['reasoning.encrypted_content'],
      tool_choice: 'auto',
      parallel_tool_calls: true,
    }

    if (request.tools !== undefined) {
      const mapped = request.tools.map((tool) => ({
        type: 'function',
        name: tool.name,
        description: tool.description ?? null,
        parameters: tool.inputSchema ?? null,
        strict: null,
      }))
      if (mapped.length > 0) body.tools = mapped
    }

    // Reasoning effort: a per-request provider_options string value wins; else
    // the provider-level default; else the `reasoning` object is omitted.
    let effort: string | undefined
    const candidate = request.providerOptions?.reasoning_effort
    if (typeof candidate === 'string') effort = candidate
    if (effort === undefined) effort = this._reasoningEffort
    if (effort !== undefined) body.reasoning = { effort, summary: 'auto' }

    if (request.temperature !== undefined) body.temperature = request.temperature

    return body
  }

  // chat()/stream() land in T2/T3.
}
