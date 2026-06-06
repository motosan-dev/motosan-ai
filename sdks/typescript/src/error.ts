export class MotosanError extends Error {}
export class AuthError extends MotosanError {}
export class RateLimitError extends MotosanError {}
export class InvalidRequestError extends MotosanError {}
export class ConfigError extends MotosanError {}
export class ProviderError extends MotosanError {}
export class NetworkError extends MotosanError {}
export class StreamError extends MotosanError {}

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

export function mapHttpError(status: number, message: string): MotosanError {
  if (status === 401) return new AuthError(message)
  if (status === 429) return new RateLimitError(message)
  if (status === 400) return new InvalidRequestError(message)
  return new ProviderError(message)
}

/**
 * Determine if an HTTP status code is retryable.
 * Retryable statuses: 429 (rate limit) or >= 500 (server error).
 *
 * Mirrors Rust `is_retryable_status`.
 */
export function isRetryableStatus(status: number): boolean {
  return status === 429 || status >= 500
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

/**
 * Parse the Retry-After header value (integer seconds) into milliseconds.
 *
 * Returns undefined if the header is null, empty, or contains a non-integer value.
 * Mirrors Rust `parse_retry_after`: trim, parse as u64 seconds, convert to ms.
 */
export function parseRetryAfter(headerValue: string | null): number | undefined {
  if (headerValue === null) {
    return undefined
  }

  const trimmed = headerValue.trim()
  if (!/^\d+$/.test(trimmed)) {
    return undefined
  }

  return Number(trimmed) * 1000
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
