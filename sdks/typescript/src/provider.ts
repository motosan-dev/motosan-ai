/**
 * Provider capabilities, validation, dispatch, and stream wrappers.
 *
 * Mirrors Rust `providers/mod.rs:76-96` (validate_request) and
 * `types.rs:903-930` (ProviderCapabilities).
 */

import { StreamReadTimeoutError, UnsupportedFeatureError } from './error.js'
import type { BoxModelStream, BoxStream } from './stream.js'
import type {
  ChatRequest,
  ChatResponse,
  ContentBlock,
  ModelChatRequest,
  ModelChatResponse,
  ModelStreamDelta,
  StreamEvent,
} from './types.js'

/**
 * Describes what features a provider supports.
 *
 * Mirrors Rust `ProviderCapabilities` (types.rs:1518-1523) PLUS a TS-only
 * `supportsMcp` flag — Rust has no MCP capability and achieves "no MCP on
 * OpenAI" by serializer omission, while TS lets validateRequest reject it.
 * `supportsFreeformTools` gates the NATIVE model surface only; it says nothing
 * about the legacy function-tool surface.
 */
export interface ProviderCapabilities {
  supportsImage: boolean
  supportsDocument: boolean
  /** TS-only divergence from Rust: whether the provider accepts MCP server/tool config. */
  supportsMcp: boolean
  /** Whether the provider speaks native Freeform (custom) tools. */
  supportsFreeformTools: boolean
}

/**
 * Provider with text only — no images, no documents, no freeform tools.
 */
export function textOnly(): ProviderCapabilities {
  return {
    supportsImage: false,
    supportsDocument: false,
    supportsMcp: false,
    supportsFreeformTools: false,
  }
}

/**
 * Provider with image support — images but no documents, no freeform tools.
 */
export function withImage(): ProviderCapabilities {
  return {
    supportsImage: true,
    supportsDocument: false,
    supportsMcp: false,
    supportsFreeformTools: false,
  }
}

/**
 * Provider with full legacy support — images and documents.
 *
 * `supportsFreeformTools` stays FALSE on purpose, matching Rust
 * `ProviderCapabilities::full()` (types.rs:1558-1564). A provider that flipped
 * it here would silently claim native support it does not have.
 */
export function fullCaps(): ProviderCapabilities {
  return {
    supportsImage: true,
    supportsDocument: true,
    supportsMcp: true,
    supportsFreeformTools: false,
  }
}

/**
 * MiniMax capabilities: text-only (no images/documents) but MCP-capable,
 * because MiniMax routes through the Anthropic-compatible wire (contract §5/§6).
 * Distinct from `textOnly()` so MCP isn't blanket-blocked on the text-only path.
 */
export function minimaxCaps(): ProviderCapabilities {
  return {
    supportsImage: false,
    supportsDocument: false,
    supportsMcp: true,
    supportsFreeformTools: false,
  }
}

/**
 * Text-only provider that speaks native Freeform tools (ChatGPT Codex).
 * Mirrors Rust `ProviderCapabilities::with_freeform_tools()`.
 */
export function withFreeformTools(): ProviderCapabilities {
  return {
    supportsImage: false,
    supportsDocument: false,
    supportsMcp: false,
    supportsFreeformTools: true,
  }
}

/**
 * Image-capable provider that speaks native Freeform tools (OpenAI with the
 * Responses opt-in on). Mirrors Rust
 * `ProviderCapabilities::with_image_and_freeform_tools()`.
 */
export function withImageAndFreeformTools(): ProviderCapabilities {
  return {
    supportsImage: true,
    supportsDocument: false,
    supportsMcp: false,
    supportsFreeformTools: true,
  }
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
 * Minimal shape a provider must expose to be dispatched.
 *
 * `modelChat` / `modelStream` are OPTIONAL: this is a structural contract that
 * third parties implement, so requiring the native surface would be a breaking
 * change. Providers that omit them are rejected by dispatchModelChat /
 * dispatchModelStream with UnsupportedFeatureError.
 */
export interface ProviderImpl {
  capabilities(): ProviderCapabilities
  chat(req: ChatRequest, opts?: ProviderRequestOptions): Promise<ChatResponse>
  stream(req: ChatRequest, opts?: ProviderRequestOptions): BoxStream
  modelChat?(req: ModelChatRequest, opts?: ProviderRequestOptions): Promise<ModelChatResponse>
  modelStream?(req: ModelChatRequest, opts?: ProviderRequestOptions): BoxModelStream
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
 * Validate a native model request against provider capabilities.
 *
 * Mirrors Rust `ProviderImpl::validate_model_request` (providers/mod.rs:82-140)
 * MINUS the thinking and MCP arms: milestone D3 omits those request fields
 * entirely, so a caller reaching for them gets a type error rather than a
 * runtime UnsupportedFeatureError. Throws BEFORE any HTTP call.
 */
export function validateModelRequest(req: ModelChatRequest, caps: ProviderCapabilities): void {
  const hasFreeformSpec = (req.toolSpecs ?? []).some((spec) => spec.kind === 'freeform')
  const hasFreeformHistory = req.context.some(
    (item) =>
      (item.kind === 'toolCall' && item.call.kind === 'freeform') ||
      (item.kind === 'toolOutput' && item.output.kind === 'custom'),
  )
  if ((hasFreeformSpec || hasFreeformHistory) && !caps.supportsFreeformTools) {
    throw new UnsupportedFeatureError('provider does not support native freeform tools')
  }

  for (const item of req.context) {
    if (item.kind !== 'message') continue
    const blocks: ContentBlock[] = item.message.contentBlocks ?? []
    for (const block of blocks) {
      if (block.type === 'image' && !caps.supportsImage) {
        throw new UnsupportedFeatureError('provider does not support image input')
      }
      if (block.type === 'document' && !caps.supportsDocument) {
        throw new UnsupportedFeatureError('provider does not support document input')
      }
    }
  }
}

/**
 * Dispatch a native model request: validate BEFORE any HTTP call, then
 * provider.modelChat (which owns its retry loop). Mirrors Rust
 * client.rs dispatch_model_chat.
 */
export async function dispatchModelChat(
  provider: ProviderImpl,
  req: ModelChatRequest,
  opts?: ProviderRequestOptions,
): Promise<ModelChatResponse> {
  validateModelRequest(req, provider.capabilities())
  if (!provider.modelChat) {
    throw new UnsupportedFeatureError('provider does not support native model requests')
  }
  return provider.modelChat(req, opts)
}

/**
 * Dispatch a native model stream: validate BEFORE connecting, then
 * provider.modelStream. Sync return, so a rejected request throws
 * synchronously rather than on first iteration.
 */
export function dispatchModelStream(
  provider: ProviderImpl,
  req: ModelChatRequest,
  opts?: ProviderRequestOptions,
): BoxModelStream {
  validateModelRequest(req, provider.capabilities())
  if (!provider.modelStream) {
    throw new UnsupportedFeatureError('provider does not support native model streams')
  }
  return provider.modelStream(req, opts)
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

/**
 * Native-stream analogue of readTimeoutStream. THROWS StreamReadTimeoutError
 * when no delta arrives within the deadline; the deadline resets on each
 * yielded delta and the inner iterator is cancelled before throwing. Mirrors
 * Rust `ReadTimeoutModelStream` (client.rs:1239-1259).
 */
export async function* readTimeoutModelStream(
  inner: BoxModelStream,
  timeoutSecs: number,
): BoxModelStream {
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

        const result = raced as IteratorResult<ModelStreamDelta>
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
