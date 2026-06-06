/**
 * MiniMax provider. MiniMax exposes an Anthropic-compatible endpoint, so this
 * is a thin delegate over an internal AnthropicProvider with text-only caps —
 * mirroring Rust `build_minimax_provider` (client.rs:567-586), which builds an
 * AnthropicProvider with `ProviderCapabilities::text_only()`.
 *
 * Wire: serializeAnthropicRequest → POST {base}/v1/messages, x-api-key auth,
 * anthropic-version, Anthropic chat/SSE parsing — all inherited from
 * AnthropicProvider. The constructor's `baseUrl` is the BASE (the final URL is
 * `{base}/v1/messages`); default base `https://api.minimax.io/anthropic`
 * (Rust client.rs:577).
 */
import { DEFAULT_MINIMAX_MODEL } from '../models.js'
import { textOnly, type ProviderCapabilities } from '../provider.js'
import { RetryPolicy } from '../retry.js'
import type { BoxStream } from '../stream.js'
import type { ChatRequest, ChatResponse } from '../types.js'
import { AnthropicProvider } from './anthropic.js'

/** Default MiniMax Anthropic-compatible base URL (Rust client.rs:577). */
export const DEFAULT_MINIMAX_BASE_URL = 'https://api.minimax.io/anthropic'

export class MinimaxProvider {
  private readonly inner: AnthropicProvider

  constructor(apiKey: string, model?: string, baseUrl?: string) {
    this.inner = new AnthropicProvider(
      apiKey,
      model ?? DEFAULT_MINIMAX_MODEL,
      baseUrl ?? DEFAULT_MINIMAX_BASE_URL,
    )
  }

  withRetryPolicy(policy: RetryPolicy): this {
    this.inner.withRetryPolicy(policy)
    return this
  }

  chat(request: ChatRequest): Promise<ChatResponse> {
    return this.inner.chat(request)
  }

  stream(request: ChatRequest): BoxStream {
    return this.inner.stream(request)
  }

  /** Text-only — images/documents rejected by validateRequest before any HTTP call. */
  capabilities(): ProviderCapabilities {
    return textOnly()
  }
}
