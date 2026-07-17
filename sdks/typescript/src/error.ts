export class MotosanError extends Error {
  status?: number
  retryAfterMs?: number
  requestId?: string
}
export class AuthError extends MotosanError {}
export class RateLimitError extends MotosanError {}
export class InvalidRequestError extends MotosanError {}
export class ConfigError extends MotosanError {}
export class ProviderError extends MotosanError {}
export class NetworkError extends MotosanError {}
export class StreamError extends MotosanError {}

/**
 * Error thrown when the upstream byte/event stream ends (EOF) without the
 * provider's terminal event (Anthropic message_stop, OpenAI [DONE], Gemini
 * finishReason, Ollama done:true, Codex response.completed). Message
 * convention: `incomplete stream: <provider> ended without a terminal event`.
 * Subclasses StreamError deliberately (migration softener): existing
 * `instanceof StreamError` handlers keep catching truncation. Replaces the
 * retired v0.10.1 "exactly one terminal done even when upstream closes
 * without [DONE]" invariant (M3/E3; specs/types.md stream termination).
 */
export class IncompleteStreamError extends StreamError {
  constructor(message: string) {
    super(message)
    this.name = 'IncompleteStreamError'
  }
}

/**
 * Error thrown when a stream read operation times out.
 * Carries the timeout duration in seconds.
 */
export class StreamReadTimeoutError extends MotosanError {
  readonly timeoutSecs: number

  constructor(timeoutSecs: number) {
    super(`stream read timeout: no data received within ${timeoutSecs} seconds`)
    this.timeoutSecs = timeoutSecs
    this.name = 'StreamReadTimeoutError'
  }
}

/**
 * Error thrown when a provider does not support a requested feature
 * (e.g., document input on a provider that only supports text and images).
 */
export class UnsupportedFeatureError extends MotosanError {
  constructor(message: string) {
    super(message)
    this.name = 'UnsupportedFeatureError'
  }
}

export function mapHttpError(
  status: number,
  message: string,
  retryAfter?: string | null,
  requestId?: string | null,
): MotosanError {
  const error =
    status === 401
      ? new AuthError(message)
      : status === 429
        ? new RateLimitError(message)
        : status === 400
          ? new InvalidRequestError(message)
          : new ProviderError(message)
  error.status = status
  const retryAfterMs = parseRetryAfter(retryAfter ?? null)
  if (retryAfterMs !== undefined) {
    error.retryAfterMs = retryAfterMs
  }
  if (requestId) {
    error.requestId = requestId
  }
  return error
}

/**
 * Determine if an HTTP status code is retryable.
 * Retryable statuses: 408 (request timeout), 409 (conflict),
 * 429 (rate limit), or >= 500 (server error).
 *
 * Mirrors Rust `is_retryable_status`. See specs/retry.md.
 */
export function isRetryableStatus(status: number): boolean {
  return status === 408 || status === 409 || status === 429 || status >= 500
}

/**
 * Determine if a network error is retryable.
 * Retryable errors:
 * - AbortError (request cancelled)
 * - TypeError (fetch network failure)
 * - Error.code === 'ECONNREFUSED' (connection refused)
 * - Error.code === 'ENOTFOUND' (DNS resolution failure)
 * - Error.code === 'ETIMEDOUT' (connection timeout)
 *
 * Mirrors Rust `is_retryable_network_error` mapped to fetch/Node error shapes.
 */
export function isRetryableNetworkError(error: unknown): boolean {
  if (!(error instanceof Error)) {
    return false
  }

  // AbortError (fetch cancelled/timed out at fetch level)
  if (error.name === 'AbortError') {
    return true
  }

  // TypeError (fetch network failure — e.g., "Failed to fetch")
  if (error instanceof TypeError) {
    return true
  }

  // Node.js error codes (socket/connection failures)
  const code = (error as { code?: unknown }).code
  if (code === 'ECONNREFUSED' || code === 'ENOTFOUND' || code === 'ETIMEDOUT') {
    return true
  }

  return false
}

/** Cap applied to any parsed Retry-After value (specs/retry.md: RETRY_AFTER_CAP_SECS = 60). */
export const RETRY_AFTER_CAP_MS = 60_000

/**
 * Parse the Retry-After header value into milliseconds.
 *
 * Accepts both RFC 7231 forms:
 * - delay-seconds (e.g. "30") -> seconds * 1000
 * - HTTP-date (e.g. "Wed, 15 Jul 2026 12:00:30 GMT") -> date minus now
 *
 * The result is clamped to [0, RETRY_AFTER_CAP_MS]. Returns undefined if the
 * header is null, empty, or unparseable. Mirrors Rust `parse_retry_after`.
 */
export function parseRetryAfter(headerValue: string | null): number | undefined {
  if (headerValue === null) {
    return undefined
  }

  const trimmed = headerValue.trim()

  // delay-seconds form
  if (/^\d+$/.test(trimmed)) {
    return Math.min(Number(trimmed) * 1000, RETRY_AFTER_CAP_MS)
  }

  // HTTP-date form. Every RFC 7231 date contains letters (month name, "GMT");
  // requiring one keeps numeric junk like "-5" or "30.5" out of Date.parse,
  // which would otherwise interpret them as calendar dates (Node parses
  // "-5" as a valid date). Matches Rust rfc2822 / Python parsedate behavior.
  if (!/[A-Za-z]/.test(trimmed)) {
    return undefined
  }
  const parsedMs = Date.parse(trimmed)
  if (Number.isNaN(parsedMs)) {
    return undefined
  }
  return Math.max(0, Math.min(parsedMs - Date.now(), RETRY_AFTER_CAP_MS))
}

/**
 * Extract an error message from a response body.
 *
 * Attempts to extract `body.error.message` (Anthropic/OpenAI wire format).
 * Falls back to the provided fallback string if extraction fails.
 *
 * Mirrors Rust `extract_error_message`.
 */
export function extractErrorMessage(body: unknown, fallback: string): string {
  if (body === null || body === undefined) {
    return fallback
  }

  if (typeof body !== 'object') {
    return fallback
  }

  const error = (body as { error?: unknown }).error
  if (error === null || error === undefined || typeof error !== 'object') {
    return fallback
  }

  const message = (error as { message?: unknown }).message
  if (typeof message === 'string') {
    return message
  }

  return fallback
}
