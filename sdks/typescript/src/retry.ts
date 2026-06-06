export interface RetryPolicyOptions {
  maxRetries?: number
  baseDelayMs?: number
  maxDelayMs?: number
  jitter?: boolean
  respectRetryAfter?: boolean
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

  constructor(opts?: RetryPolicyOptions) {
    this.maxRetries = opts?.maxRetries ?? DEFAULT_MAX_RETRIES
    this.baseDelayMs = opts?.baseDelayMs ?? DEFAULT_BASE_DELAY_MS
    this.maxDelayMs = opts?.maxDelayMs ?? DEFAULT_MAX_DELAY_MS
    this.jitter = opts?.jitter ?? DEFAULT_JITTER
    this.respectRetryAfter =
      opts?.respectRetryAfter ?? DEFAULT_RESPECT_RETRY_AFTER
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

  delayForAttempt(attempt: number): number {
    const exponent = Math.min(Math.max(attempt - 1, 0), 31)
    const expFactor = 2 ** exponent
    let delayMs = Math.min(this.baseDelayMs * expFactor, this.maxDelayMs)

    if (this.jitter) {
      const jitterSeed = attempt * 1_103_515_245 + 12_345
      const jitterPercent = jitterSeed % 100
      const jittered = delayMs + Math.floor((delayMs * jitterPercent) / 100)
      delayMs = Math.min(jittered, this.maxDelayMs)
    }

    return delayMs
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
        const delay = policy.respectRetryAfter
          ? retryAfterMs ?? policy.delayForAttempt(attempt)
          : policy.delayForAttempt(attempt)

        await new Promise((resolve) => setTimeout(resolve, delay))
        continue
      }

      throw error
    }
  }
}
