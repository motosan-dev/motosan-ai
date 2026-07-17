import { IncompleteStreamError } from '../error.js'
import { postJson, postStream } from '../http/fetch.js'
import { parseNdjson } from '../http/ndjson.js'
import { DEFAULT_OLLAMA_MODEL } from '../models.js'
import { textOnly, type ProviderCapabilities, type ProviderRequestOptions } from '../provider.js'
import { attemptWithCancellation, classifyForRetry, RetryPolicy, withRetry } from '../retry.js'
import {
  doneEvent,
  textEvent,
  toolCallArgsWithId,
  toolCallEndWithId,
  toolCallStart,
  type BoxStream,
} from '../stream.js'
import type { ChatRequest, ChatResponse, StopReason, ToolCall } from '../types.js'


/**
 * A native-API tool call before id-defaulting. Mirrors Rust ToolCall but
 * `input` may be any JSON value (string fallback on parse failure), matching
 * Rust's `Value::String(s)` keep-raw behavior (ollama.rs:225-227).
 */
interface NativeToolCall {
  id: string
  name: string
  input: unknown
}

/**
 * Native Ollama provider hitting `/api/chat` with NDJSON streaming.
 * Mirrors Rust `OllamaProvider` (providers/ollama.rs). Text-only; no auth header.
 * The OpenAI-compat path is NOT here — it reuses OpenAIProvider (wired in
 * client.ts buildProvider). Constructor `(model, baseUrl)` mirrors
 * `OllamaProvider::new` (ollama.rs:29-39).
 */
export class OllamaProvider {
  private readonly model: string
  private readonly baseUrl: string
  private think?: string
  private keepAlive?: string
  private numCtx?: number
  private retryPolicy: RetryPolicy

  constructor(model: string, baseUrl: string) {
    this.model = model
    this.baseUrl = baseUrl.replace(/\/+$/, '') // trim trailing slash(es)
    this.retryPolicy = RetryPolicy.default()
  }

  withThink(think?: string): this {
    this.think = think
    return this
  }

  withKeepAlive(keepAlive?: string): this {
    this.keepAlive = keepAlive
    return this
  }

  withNumCtx(numCtx?: number): this {
    this.numCtx = numCtx
    return this
  }

  withRetryPolicy(policy: RetryPolicy): this {
    this.retryPolicy = policy
    return this
  }

  capabilities(): ProviderCapabilities {
    // text_only(); supportsMcp:false comes for free via the M4 factory.
    return textOnly()
  }

  private endpoint(): string {
    return `${this.baseUrl}/api/chat`
  }

  /** Native /api/chat body. stream=false for chat(), true for stream(). */
  private buildRequestBody(req: ChatRequest, stream: boolean): Record<string, unknown> {
    const model = req.model ?? this.model
    const messages: Array<Record<string, unknown>> = []

    // 1. System first: systemBlocks priority over system string.
    if (req.systemBlocks) {
      const joined = req.systemBlocks.map((b) => b.text).join('\n').trim()
      if (joined) messages.push({ role: 'system', content: joined })
    } else if (req.system) {
      const trimmed = req.system.trim()
      if (trimmed) messages.push({ role: 'system', content: trimmed })
    }

    // 2. Then each message by role.
    for (const message of req.messages) {
      switch (message.role) {
        case 'system': {
          const trimmed = message.content.trim()
          if (trimmed) messages.push({ role: 'system', content: trimmed })
          break
        }
        case 'user': {
          // Native path is text-only: flat string content (images rejected
          // by validate before reaching here).
          messages.push({ role: 'user', content: message.content })
          break
        }
        case 'assistant': {
          if (!message.toolCalls || message.toolCalls.length === 0) {
            messages.push({ role: 'assistant', content: message.content })
          } else {
            const toolCalls = message.toolCalls.map((tc) => ({
              // arguments is the PARSED OBJECT tc.input directly (NOT a JSON
              // string — contrast OpenAI). No top-level id/type.
              function: { name: tc.name, arguments: tc.input },
            }))
            messages.push({
              role: 'assistant',
              content: message.content,
              tool_calls: toolCalls,
            })
          }
          break
        }
        case 'tool': {
          // NO tool_call_id on the native path (contrast OpenAI compat).
          messages.push({ role: 'tool', content: message.content })
          break
        }
      }
    }

    const body: Record<string, unknown> = { model, messages, stream }

    // think coercion (ollama.rs:145-157): trim; omit if blank.
    if (this.think !== undefined) {
      const trimmed = this.think.trim()
      if (trimmed) {
        switch (trimmed.toLowerCase()) {
          case 'true':
          case 'yes':
          case 'on':
          case '1':
            body.think = true
            break
          case 'false':
          case 'no':
          case 'off':
          case '0':
            body.think = false
            break
          default:
            // verbatim ORIGINAL trimmed casing (e.g. "low"/"medium"/"high")
            body.think = trimmed
        }
      }
    }

    // keep_alive verbatim string at root.
    if (this.keepAlive !== undefined) {
      body.keep_alive = this.keepAlive
    }

    // options object — OMIT entirely if empty.
    const options: Record<string, unknown> = {}
    if (req.temperature !== undefined) options.temperature = req.temperature
    if (this.numCtx !== undefined) options.num_ctx = this.numCtx
    if (req.stopSequences && req.stopSequences.length > 0) options.stop = req.stopSequences
    if (Object.keys(options).length > 0) body.options = options

    // tools — only set if non-empty. TS Tool fields optional; match the
    // serialize/openai.ts defaults (openai.ts:151-152).
    if (req.tools) {
      const mapped = req.tools.map((tool) => ({
        type: 'function',
        function: {
          name: tool.name,
          description: tool.description ?? '',
          parameters: tool.inputSchema ?? { type: 'object', properties: {} },
        },
      }))
      if (mapped.length > 0) body.tools = mapped
    }

    // providerOptions merged into root LAST (Object.assign).
    if (req.providerOptions && typeof req.providerOptions === 'object') {
      Object.assign(body, req.providerOptions)
    }

    return body
  }

  /** Shared by chat + stream (ollama.rs:212-242). */
  private static extractToolCalls(message: any): NativeToolCall[] {
    const calls = message?.tool_calls
    if (!Array.isArray(calls)) return []
    const out: NativeToolCall[] = []
    calls.forEach((call: any, idx: number) => {
      const fn = call?.function
      if (!fn) return // skip if missing
      const name = fn?.name
      if (typeof name !== 'string') return // skip if missing
      const args = fn?.arguments
      let input: unknown
      if (typeof args === 'string') {
        try {
          input = JSON.parse(args)
        } catch {
          input = args // keep raw string on parse failure (Value::String)
        }
      } else if (args === undefined || args === null) {
        input = {}
      } else {
        input = args
      }
      const id =
        typeof call?.id === 'string' && call.id.length > 0 ? call.id : `call_${idx}`
      out.push({ id, name, input })
    })
    return out
  }

  async chat(req: ChatRequest, opts?: ProviderRequestOptions): Promise<ChatResponse> {
    const body = this.buildRequestBody(req, false)
    const payload = await withRetry(
      this.retryPolicy,
      async () =>
        attemptWithCancellation(opts?.callerSignal, () =>
          postJson<any>(this.endpoint(), {}, body, {
            signal: opts?.signal,
            preHeadersTimeoutMs: opts?.preHeadersTimeoutMs,
          }),
        ),
      classifyForRetry,
    )

    const message = payload?.message
    const content =
      typeof message?.content === 'string' ? message.content : ''
    const thinking =
      typeof message?.thinking === 'string' ? message.thinking : ''

    // final_content thinking-fold (ollama.rs:302-309).
    let finalContent: string
    if (content === '' && thinking !== '') {
      finalContent = thinking
    } else if (thinking !== '') {
      finalContent = `<think>${thinking}</think>\n\n${content}`
    } else {
      finalContent = content
    }

    const native = message ? OllamaProvider.extractToolCalls(message) : []
    const toolCalls: ToolCall[] = native.map((tc) => ({
      id: tc.id,
      name: tc.name,
      // extractToolCalls keeps the RAW JSON value when JSON.parse fails (Rust
      // Value::String fallback, ollama.rs), so tc.input may be a string.
      input: tc.input,
    }))

    let stopReason: StopReason
    if (toolCalls.length > 0) {
      stopReason = 'tool_use'
    } else {
      stopReason = payload?.done === true ? 'stop' : 'other'
    }

    const model = String(payload?.model ?? DEFAULT_OLLAMA_MODEL)
    const inputTokens = Number(payload?.prompt_eval_count ?? 0)
    const outputTokens = Number(payload?.eval_count ?? 0)

    return {
      content: finalContent,
      // native chat() folds thinking into content via <think>; it does NOT
      // populate ChatResponse.thinking (ollama.rs never calls .thinking()).
      toolCalls,
      model,
      usage: { inputTokens, outputTokens },
      stopReason,
    }
  }

  stream(req: ChatRequest, opts?: ProviderRequestOptions): BoxStream {
    return this.streamImpl(req, opts)
  }

  private async *streamImpl(req: ChatRequest, opts?: ProviderRequestOptions) {
    const body = this.buildRequestBody(req, true)

    // Retry ONLY the initial postStream fetch via the shared engine.
    const responseBody = await withRetry(
      this.retryPolicy,
      async () =>
        attemptWithCancellation(opts?.callerSignal, () =>
          postStream(this.endpoint(), {}, body, {
            signal: opts?.signal,
            preHeadersTimeoutMs: opts?.preHeadersTimeoutMs,
          }),
        ),
      classifyForRetry,
    )

    // Adapter over parseNdjson: terminal event is done:true. Body errors
    // propagate (M3 removed the M1-era swallow).
    for await (const obj of parseNdjson(responseBody)) {
      const done = (obj as { done?: unknown }).done === true
      if (done) {
        // Rust StreamEvent::done() carries NO stop_reason — plain doneEvent.
        yield doneEvent()
        return
      }

      const message = (obj as { message?: any }).message
      const content =
        typeof message?.content === 'string' ? message.content : ''
      const thinking =
        typeof message?.thinking === 'string' ? message.thinking : ''

      // text selection (ollama.rs:459-465). Streamed thinking is surfaced as
      // a plain textEvent (folded into the text stream).
      let text: string
      if (thinking !== '' && content === '') {
        text = thinking
      } else if (content !== '') {
        text = content
      } else {
        text = ''
      }
      if (text !== '') {
        yield textEvent(text)
      }

      // 3-event pattern per tool call (ollama.rs:471-483).
      if (message) {
        for (const tc of OllamaProvider.extractToolCalls(message)) {
          yield toolCallStart(tc.id, tc.name)
          yield toolCallArgsWithId(tc.id, JSON.stringify(tc.input))
          yield toolCallEndWithId(tc.id)
        }
      }
    }
    // EOF without done:true: truncation, not completion (M3/E2). done:true
    // returns from the generator above, so reaching here means no terminal.
    throw new IncompleteStreamError('incomplete stream: ollama ended without a terminal event')
  }
}
