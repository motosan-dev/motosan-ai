/** Fired via RetryPolicy.onRetry before each retry sleep (D7). */
export interface RetryEvent {
  attempt: number
  delayMs: number
  cause: string
}

export interface RetryPolicyOptions {
  maxRetries?: number
  baseDelayMs?: number
  maxDelayMs?: number
  jitter?: boolean
  respectRetryAfter?: boolean
  /** Injectable RNG in [0, 1) for full jitter. Defaults to Math.random. */
  random?: () => number
  onRetry?: (evt: RetryEvent) => void
}

export interface RetryClassification {
  retryable: boolean
  retryAfterMs?: number
}

const DEFAULT_MAX_RETRIES = 3
const DEFAULT_BASE_DELAY_MS = 100
const DEFAULT_MAX_DELAY_MS = 2000
const DEFAULT_JITTER = true
const DEFAULT_RESPECT_RETRY_AFTER = true

export class RetryPolicy {
  maxRetries: number
  baseDelayMs: number
  maxDelayMs: number
  jitter: boolean
  respectRetryAfter: boolean
  random: () => number
  onRetry?: (evt: RetryEvent) => void

  constructor(opts?: RetryPolicyOptions) {
    this.maxRetries = opts?.maxRetries ?? DEFAULT_MAX_RETRIES
    this.baseDelayMs = opts?.baseDelayMs ?? DEFAULT_BASE_DELAY_MS
    this.maxDelayMs = opts?.maxDelayMs ?? DEFAULT_MAX_DELAY_MS
    this.jitter = opts?.jitter ?? DEFAULT_JITTER
    this.respectRetryAfter =
      opts?.respectRetryAfter ?? DEFAULT_RESPECT_RETRY_AFTER
    this.random = opts?.random ?? Math.random
    this.onRetry = opts?.onRetry
  }

  static default(): RetryPolicy {
    return new RetryPolicy()
  }

  withMaxRetries(n: number): this {
    this.maxRetries = n
    return this
  }

  withBaseDelayMs(ms: number): this {
    this.baseDelayMs = ms
    return this
  }

  withMaxDelayMs(ms: number): this {
    this.maxDelayMs = ms
    return this
  }

  withJitter(enabled: boolean): this {
    this.jitter = enabled
    return this
  }

  withRespectRetryAfter(enabled: boolean): this {
    this.respectRetryAfter = enabled
    return this
  }

  withRandom(random: () => number): this {
    this.random = random
    return this
  }

  withOnRetry(onRetry: (evt: RetryEvent) => void): this {
    this.onRetry = onRetry
    return this
  }

  delayForAttempt(attempt: number): number {
    const exponent = Math.min(Math.max(attempt - 1, 0), 31)
    const expDelay = Math.min(this.baseDelayMs * 2 ** exponent, this.maxDelayMs)

    if (!this.jitter) {
      return expDelay
    }

    // Full jitter (specs/retry.md): uniform in [0, expDelay).
    return this.random() * expDelay
  }
}

export async function withRetry<T>(
  policy: RetryPolicy,
  op: (attempt: number) => Promise<T>,
  classify: (errOrResult: unknown) => RetryClassification,
): Promise<T> {
  let attempt = 0

  while (true) {
    try {
      return await op(attempt + 1)
    } catch (error) {
      const { retryable, retryAfterMs } = classify(error)

      if (attempt < policy.maxRetries && retryable) {
        attempt += 1
        // Retry-After (capped upstream in parseRetryAfter) is used VERBATIM — no jitter.
        const delay = policy.respectRetryAfter
          ? retryAfterMs ?? policy.delayForAttempt(attempt)
          : policy.delayForAttempt(attempt)

        policy.onRetry?.({
          attempt,
          delayMs: delay,
          cause: error instanceof Error ? error.message : String(error),
        })

        await new Promise((resolve) => setTimeout(resolve, delay))
        continue
      }

      throw error
    }
  }
}
