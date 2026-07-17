import { IncompleteStreamError } from '../error.js'
import { postJson, postStream } from '../http/fetch.js'
import { parseSse } from '../http/sse.js'
import { DEFAULT_GEMINI_MODEL } from '../models.js'
import { withImage, type ProviderCapabilities, type ProviderRequestOptions } from '../provider.js'
import { attemptWithCancellation, classifyForRetry, RetryPolicy, withRetry } from '../retry.js'
import { serializeGeminiRequest } from '../serialize/gemini.js'
import {
  BoxStream,
  doneWithStopReason,
  textEvent,
  toolCallArgsWithId,
  toolCallEndWithId,
  toolCallStart,
  usageEvent,
} from '../stream.js'
import type { ChatRequest, ChatResponse, StopReason, ToolCall } from '../types.js'

/** Default generativelanguage REST base (gemini.rs:25). */
const BASE_URL = 'https://generativelanguage.googleapis.com/v1beta'

/**
 * Module-level monotonic tool-call id generator. Gemini omits call ids on
 * functionCall, so we synthesize `call_${n}`. Mirrors Rust's `static AtomicU64`
 * (gemini.rs:27-32). Process-global (shared across instances) is intentional —
 * tests assert ids via the regex /^call_\d+$/, never exact values.
 */
let toolCallCounter = 0
function genToolCallId(): string {
  return `call_${toolCallCounter++}`
}

/**
 * Map a Gemini finishReason to the SDK StopReason union (gemini.rs:280-286,
 * 513-520). CRITICAL: "STOP" maps to 'end_turn', NOT 'stop' — do NOT reuse
 * OpenAI's FINISH_REASON_MAP.
 */
function mapFinishReason(reason: string, hasToolCalls: boolean): StopReason {
  if (reason === 'STOP') return hasToolCalls ? 'tool_use' : 'end_turn'
  if (reason === 'MAX_TOKENS') return 'max_tokens'
  // SAFETY / RECITATION / ... — tool_use if tools were produced, else other.
  return hasToolCalls ? 'tool_use' : 'other'
}


/**
 * Gemini provider over the generativelanguage REST API (generateContent +
 * streamGenerateContent?alt=sse). Structurally satisfies ProviderImpl. All
 * wire behavior mirrors Rust gemini.rs; infra idioms mirror providers/openai.ts.
 */
export class GeminiProvider {
  private readonly model: string
  private readonly baseUrl: string
  private retryPolicy: RetryPolicy

  constructor(
    private readonly apiKey: string,
    model?: string,
    baseUrl = BASE_URL,
  ) {
    this.model = model ?? DEFAULT_GEMINI_MODEL // (gemini.rs:52)
    this.baseUrl = baseUrl.replace(/\/+$/, '') // trim trailing slash(es), like OpenAIProvider (openai.ts:68)
    this.retryPolicy = RetryPolicy.default()
  }

  withRetryPolicy(policy: RetryPolicy): this {
    this.retryPolicy = policy
    return this
  }

  capabilities(): ProviderCapabilities {
    // Post-M4 withImage() returns { supportsImage:true, supportsDocument:false,
    // supportsMcp:false } (gemini.rs:316).
    return withImage()
  }

  private resolveModel(request: ChatRequest): string {
    return request.model ?? this.model // (gemini.rs:64,69)
  }

  private generateUrl(model: string): string {
    return `${this.baseUrl}/models/${model}:generateContent` // (gemini.rs:65)
  }

  private streamUrl(model: string): string {
    return `${this.baseUrl}/models/${model}:streamGenerateContent?alt=sse` // (gemini.rs:71-72)
  }

  private headers(): Record<string, string> {
    // content-type is injected by http/fetch.ts postJson/postStream (fetch.ts:31,55).
    return { 'x-goog-api-key': this.apiKey } // (gemini.rs:77)
  }

  async chat(request: ChatRequest, opts?: ProviderRequestOptions): Promise<ChatResponse> {
    const model = this.resolveModel(request)
    const url = this.generateUrl(model)
    const body = serializeGeminiRequest(request, model)

    const payload = await withRetry(
      this.retryPolicy,
      async () =>
        attemptWithCancellation(opts?.callerSignal, () =>
          postJson<any>(url, this.headers(), body, {
            signal: opts?.signal,
            preHeadersTimeoutMs: opts?.preHeadersTimeoutMs,
          }),
        ),
      classifyForRetry,
    )

    return this.parseResponse(payload, model)
  }

  private parseResponse(payload: any, defaultModel: string): ChatResponse {
    const candidate = payload?.candidates?.[0] ?? {}
    const parts: any[] = candidate?.content?.parts ?? []

    let content = ''
    const toolCalls: ToolCall[] = []

    for (const part of parts) {
      if (typeof part?.text === 'string') {
        content += part.text // (gemini.rs:257-259)
      }
      if (part?.functionCall) {
        const fc = part.functionCall
        toolCalls.push({
          id: genToolCallId(), // client-generated; no id on the wire (gemini.rs:268)
          name: typeof fc?.name === 'string' ? fc.name : '',
          input:
            fc?.args && typeof fc.args === 'object'
              ? (fc.args as Record<string, unknown>)
              : {}, // input from wire `args` (gemini.rs:266)
        })
      }
    }

    const finishReason =
      typeof candidate?.finishReason === 'string' ? candidate.finishReason : 'STOP' // (gemini.rs:275-278)
    const stopReason = mapFinishReason(finishReason, toolCalls.length > 0)

    const usageMeta = payload?.usageMetadata ?? {}
    const inputTokens = Number(usageMeta?.promptTokenCount ?? 0)
    const outputTokens = Number(usageMeta?.candidatesTokenCount ?? 0)

    const model =
      typeof payload?.modelVersion === 'string' ? payload.modelVersion : defaultModel // (gemini.rs:298-302)

    return {
      content,
      toolCalls,
      model,
      usage: { inputTokens, outputTokens },
      stopReason,
    }
  }

  stream(request: ChatRequest, opts?: ProviderRequestOptions): BoxStream {
    return this.streamImpl(request, opts)
  }

  private async *streamImpl(request: ChatRequest, opts?: ProviderRequestOptions) {
    const model = this.resolveModel(request)
    const url = this.streamUrl(model)
    const body = serializeGeminiRequest(request, model)

    // Retry ONLY the initial postStream fetch via the shared engine. Once the
    // body is obtained, parseSse drives with NO mid-stream retry (gemini.rs:372-413).
    const responseBody = await withRetry(
      this.retryPolicy,
      async () =>
        attemptWithCancellation(opts?.callerSignal, () =>
          postStream(url, this.headers(), body, {
            signal: opts?.signal,
            preHeadersTimeoutMs: opts?.preHeadersTimeoutMs,
          }),
        ),
      classifyForRetry,
    )

    // Drive parseSse. Gemini sends NO [DONE]; each data: line is a full
    // GenerateContentResponse chunk. Termination is finishReason-driven (emit
    // doneWithStopReason) or stream EOF. NO defensive EOF done (gemini.rs:531;
    // contrast openai.ts:460-474). (sse.ts [DONE] is advisory and never
    // terminates — sse.ts:7-9,134-139.)
    let sawTerminal = false
    for await (const evt of parseSse(responseBody)) {
      const data = evt.data

      // Defensive: skip a stray [DONE] or empty data (gemini.rs:447-449).
      if (data === '[DONE]' || !data) continue
      if (typeof data !== 'object') continue

      const candidate = data?.candidates?.[0]
      if (!candidate) continue // (gemini.rs:455-458)

      const parts: any[] = candidate?.content?.parts ?? [] // (gemini.rs:460-465)
      const finishReason =
        typeof candidate?.finishReason === 'string' ? candidate.finishReason : undefined

      let hasToolCalls = false

      // Chunk pending-queue order: text events, then tool-call triplets
      // (gemini.rs:471-494).
      for (const part of parts) {
        if (typeof part?.text === 'string' && part.text !== '') {
          yield textEvent(part.text) // (gemini.rs:472-476)
        }
        if (part?.functionCall) {
          hasToolCalls = true
          const fc = part.functionCall
          const name = typeof fc?.name === 'string' ? fc.name : ''
          const args = fc?.args && typeof fc.args === 'object' ? fc.args : {}
          const id = genToolCallId()
          // Synthesize three events; args serialized in ONE shot (gemini.rs:485,490).
          yield toolCallStart(id, name)
          yield toolCallArgsWithId(id, JSON.stringify(args))
          yield toolCallEndWithId(id)
        }
      }

      // usage AFTER tool triplets (gemini.rs:496-511).
      const usageMeta = data?.usageMetadata
      if (usageMeta) {
        yield usageEvent({
          inputTokens: Number(usageMeta?.promptTokenCount ?? 0),
          outputTokens: Number(usageMeta?.candidatesTokenCount ?? 0),
        })
      }

      // done LAST, only when finishReason present — the ONLY terminator
      // (gemini.rs:513-523).
      if (finishReason !== undefined) {
        sawTerminal = true
        yield doneWithStopReason(mapFinishReason(finishReason, hasToolCalls))
      }
    }
    // EOF without any finishReason: truncation, not completion (M3/E2).
    if (!sawTerminal) {
      throw new IncompleteStreamError('incomplete stream: gemini ended without a terminal event')
    }
  }
}
