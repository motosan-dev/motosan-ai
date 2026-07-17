/**
 * Provider capabilities, validation, dispatch, and stream wrappers.
 *
 * Mirrors Rust `providers/mod.rs:76-96` (validate_request) and
 * `types.rs:903-930` (ProviderCapabilities).
 */

import { StreamReadTimeoutError, UnsupportedFeatureError } from './error.js'
import type { BoxStream } from './stream.js'
import type { ChatRequest, ChatResponse, ContentBlock, StreamEvent } from './types.js'

/**
 * Describes what features a provider supports.
 *
 * Mirrors Rust `ProviderCapabilities` (types.rs:903-907) PLUS a TS-only
 * `supportsMcp` flag. Rust has no MCP capability — it achieves "no MCP on
 * OpenAI" by serializer omission. TS adds the flag so validateRequest can
 * reject MCP on non-Anthropic providers per the M4 Done-when requirement.
 */
export interface ProviderCapabilities {
  supportsImage: boolean
  supportsDocument: boolean
  /** TS-only divergence from Rust: whether the provider accepts MCP server/tool config. */
  supportsMcp: boolean
}

/**
 * Provider with text only — no images, no documents.
 *
 * Mirrors Rust `ProviderCapabilities::text_only()` (types.rs:910-915).
 */
export function textOnly(): ProviderCapabilities {
  return { supportsImage: false, supportsDocument: false, supportsMcp: false }
}

/**
 * Provider with image support — images but no documents.
 *
 * Mirrors Rust `ProviderCapabilities::with_image()` (types.rs:917-922).
 */
export function withImage(): ProviderCapabilities {
  return { supportsImage: true, supportsDocument: false, supportsMcp: false }
}

/**
 * Provider with full support — images and documents.
 *
 * Mirrors Rust `ProviderCapabilities::full()` (types.rs:924-929).
 */
export function fullCaps(): ProviderCapabilities {
  return { supportsImage: true, supportsDocument: true, supportsMcp: true }
}

/**
 * MiniMax capabilities: text-only (no images/documents) but MCP-capable,
 * because MiniMax routes through the Anthropic-compatible wire (contract §5/§6).
 * Distinct from `textOnly()` so MCP isn't blanket-blocked on the text-only path.
 */
export function minimaxCaps(): ProviderCapabilities {
  return { supportsImage: false, supportsDocument: false, supportsMcp: true }
}

/**
 * Validate a chat request against provider capabilities.
 *
 * Iterates request.messages[].contentBlocks and throws UnsupportedFeatureError if:
 * - Content block is an image and !caps.supportsImage
 * - Content block is a document and !caps.supportsDocument
 *
 * Mirrors Rust Provider::validate_request (providers/mod.rs:76-96).
 * Throws BEFORE any HTTP call.
 */
export function validateRequest(req: ChatRequest, caps: ProviderCapabilities): void {
  for (const msg of req.messages) {
    const blocks: ContentBlock[] = msg.contentBlocks ?? []
    for (const block of blocks) {
      if (block.type === 'image' && !caps.supportsImage) {
        throw new UnsupportedFeatureError('provider does not support image input')
      }
      if (block.type === 'document' && !caps.supportsDocument) {
        throw new UnsupportedFeatureError('provider does not support document input')
      }
    }
  }

  // TS-only MCP gating (contract §6): reject MCP config on non-MCP providers.
  const hasMcp = (req.mcpServers?.length ?? 0) > 0 || (req.mcpToolConfigs?.length ?? 0) > 0
  if (hasMcp && !caps.supportsMcp) {
    throw new UnsupportedFeatureError('provider does not support MCP server config')
  }
}

/**
 * String-tagged provider discriminator. 'ollama' added in M5, 'gemini' in M6.
 * Auto-routing between Ollama's native /api/chat and the OpenAI-compat path is
 * decided at provider-construction time in ClientBuilder.buildProvider (see
 * client.ts) — dispatchChat/dispatchStream stay provider-agnostic, so NO
 * 'ollama' arm is added here.
 * chatgpt_codex added in 0.11.0 — built only via ClientBuilder.chatgptCodex
 * (no env-key dispatch arm; keyed '' in ENV_KEY_BY_PROVIDER, kept out of HTTP_PROVIDERS).
 */
export type Provider = 'anthropic' | 'openai' | 'minimax' | 'ollama' | 'gemini' | 'chatgpt_codex'

/** Per-request options accepted by Client.chat / Client.stream. */
export interface RequestOptions {
  /** Caller cancellation signal. Abort => CancelledError, never retried (E6). */
  signal?: AbortSignal
}

/**
 * What providers receive: `signal` is the fetch signal (caller signal, plus
 * the opt-in totalMs AbortSignal.timeout on chat paths only); `callerSignal`
 * is the raw caller signal, kept separate so the CancelledError-vs-
 * retryable-abort split can test callerSignal.aborted; `preHeadersTimeoutMs`
 * is the E4 connect budget, disarmed by http/fetch.ts once headers arrive.
 */
export interface ProviderRequestOptions extends RequestOptions {
  callerSignal?: AbortSignal
  preHeadersTimeoutMs?: number
}

/**
 * Minimal shape a provider must expose to be dispatched: a capability reshape
 * plus chat/stream. Concrete providers (AnthropicProvider/OpenAIProvider/
 * MinimaxProvider) structurally satisfy this.
 */
export interface ProviderImpl {
  capabilities(): ProviderCapabilities
  chat(req: ChatRequest, opts?: ProviderRequestOptions): Promise<ChatResponse>
  stream(req: ChatRequest, opts?: ProviderRequestOptions): BoxStream
}

/**
 * Dispatch a chat request: validateRequest(caps) BEFORE any HTTP call, then
 * provider.chat (which owns its retry loop). NO retry here; mirrors Rust
 * client.rs dispatch_chat (validate → p.chat).
 */
export async function dispatchChat(
  provider: ProviderImpl,
  req: ChatRequest,
  opts?: ProviderRequestOptions,
): Promise<ChatResponse> {
  validateRequest(req, provider.capabilities())
  return provider.chat(req, opts)
}

/**
 * Dispatch a stream request: validate BEFORE connecting, then provider.stream
 * (which owns the initial-fetch retry; NO retry here). Sync return; the returned
 * generator throws on initial-fetch failure after provider internal retries.
 */
export function dispatchStream(
  provider: ProviderImpl,
  req: ChatRequest,
  opts?: ProviderRequestOptions,
): BoxStream {
  validateRequest(req, provider.capabilities())
  return provider.stream(req, opts)
}

/**
 * Stream wrapper that applies a read-idle timeout. THROWS StreamReadTimeoutError
 * when no event arrives within the deadline (E7 — the pre-M3 silent-end behavior
 * is retired); the deadline resets on each yielded event and the inner iterator
 * is cancelled before throwing.
 */
export async function* readTimeoutStream(inner: BoxStream, timeoutSecs: number): BoxStream {
  const timeoutMs = timeoutSecs * 1000
  const iterator = inner[Symbol.asyncIterator]()
  let closed = false

  async function closeInner(): Promise<void> {
    if (!closed) {
      closed = true
      await iterator.return?.()
    }
  }

  function cancelInnerWithoutWaiting(): void {
    if (!closed) {
      closed = true
      void iterator.return?.().catch(() => {})
    }
  }

  try {
    while (true) {
      let timer: ReturnType<typeof setTimeout> | undefined
      try {
        const timeout = new Promise<'__timeout__'>((resolve) => {
          timer = setTimeout(() => resolve('__timeout__'), timeoutMs)
        })
        const next = iterator.next()
        const raced = await Promise.race([next, timeout])
        if (timer !== undefined) {
          clearTimeout(timer)
          timer = undefined
        }

        if (raced === '__timeout__') {
          cancelInnerWithoutWaiting()
          throw new StreamReadTimeoutError(timeoutSecs)
        }

        const result = raced as IteratorResult<StreamEvent>
        if (result.done) {
          closed = true
          return
        }
        yield result.value
      } finally {
        if (timer !== undefined) clearTimeout(timer)
      }
    }
  } finally {
    await closeInner()
  }
}
