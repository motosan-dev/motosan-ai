import { afterEach, describe, expect, it, vi } from 'vitest'
import { isRetryableNetworkError, isRetryableStatus, parseRetryAfter } from '../src/error.js'
import { RetryPolicy, withRetry } from '../src/retry.js'

function retryableByStatusOrNetwork(err: unknown) {
  const status = (err as { status?: number })?.status
  return {
    retryable:
      (status !== undefined && isRetryableStatus(status)) ||
      isRetryableNetworkError(err),
  }
}

describe('RetryPolicy default', () => {
  it('uses the canonical retry defaults', () => {
    const policy = RetryPolicy.default()

    expect(policy.maxRetries).toBe(3)
    expect(policy.baseDelayMs).toBe(100)
    expect(policy.maxDelayMs).toBe(2000)
    expect(policy.jitter).toBe(true)
    expect(policy.respectRetryAfter).toBe(true)
  })

  it('mutates and returns this from fluent setters', () => {
    const policy = RetryPolicy.default()

    const returned = policy
      .withMaxRetries(5)
      .withBaseDelayMs(25)
      .withMaxDelayMs(750)
      .withJitter(false)
      .withRespectRetryAfter(false)

    expect(returned).toBe(policy)
    expect(policy.maxRetries).toBe(5)
    expect(policy.baseDelayMs).toBe(25)
    expect(policy.maxDelayMs).toBe(750)
    expect(policy.jitter).toBe(false)
    expect(policy.respectRetryAfter).toBe(false)
  })
})

describe('RetryPolicy.delayForAttempt', () => {
  it('uses exponential backoff when jitter is disabled', () => {
    const policy = new RetryPolicy({ jitter: false })

    expect(policy.delayForAttempt(1)).toBe(100)
    expect(policy.delayForAttempt(2)).toBe(200)
    expect(policy.delayForAttempt(3)).toBe(400)
    expect(policy.delayForAttempt(4)).toBe(800)
    expect(policy.delayForAttempt(5)).toBe(1600)
  })

  it('caps delays at maxDelayMs', () => {
    const policy = new RetryPolicy({ jitter: false })

    expect(policy.delayForAttempt(6)).toBe(2000)
    expect(policy.delayForAttempt(10)).toBe(2000)
    expect(policy.delayForAttempt(40)).toBe(2000)
  })

  it('treats attempt zero as the base attempt', () => {
    const policy = new RetryPolicy({ jitter: false })

    expect(policy.delayForAttempt(0)).toBe(100)
  })
})

describe('RetryPolicy jitter (deterministic)', () => {
  it('uses attempt-derived deterministic jitter by default', () => {
    const policy = RetryPolicy.default()

    expect(policy.delayForAttempt(1)).toBe(190)
    expect(policy.delayForAttempt(2)).toBe(270)
    expect(policy.delayForAttempt(3)).toBe(720)
  })

  it('same attempt always produces the same jittered delay', () => {
    const policy = RetryPolicy.default()

    expect(policy.delayForAttempt(2)).toBe(policy.delayForAttempt(2))
  })

  it('uses integer millisecond jitter like Rust for odd base delays', () => {
    const policy = new RetryPolicy({ baseDelayMs: 101, maxDelayMs: 10_000 })

    expect(policy.delayForAttempt(1)).toBe(191)
    expect(policy.delayForAttempt(2)).toBe(272)
  })

  it('jitter can be disabled', () => {
    const policy = new RetryPolicy({ jitter: false })

    expect(policy.delayForAttempt(1)).toBe(100)
    expect(policy.delayForAttempt(2)).toBe(200)
  })

  it('jittered delay respects maxDelayMs cap', () => {
    const policy = RetryPolicy.default()

    expect(policy.delayForAttempt(5)).toBeLessThanOrEqual(2000)
    expect(policy.delayForAttempt(6)).toBe(2000)
  })
})

describe('withRetry', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    vi.useRealTimers()
  })

  it('succeeds on first attempt', async () => {
    const op = vi.fn(async (attempt: number) => `result-${attempt}`)
    const classify = () => ({ retryable: false })

    const result = await withRetry(RetryPolicy.default(), op, classify)

    expect(result).toBe('result-1')
    expect(op).toHaveBeenCalledOnce()
    expect(op).toHaveBeenCalledWith(1)
  })

  it('retries on retryable error and eventually succeeds', async () => {
    vi.useFakeTimers()
    const op = vi.fn(async (attempt: number) => {
      if (attempt < 3) {
        throw new Error('retryable')
      }
      return `success-${attempt}`
    })
    const classify = (err: unknown) => ({
      retryable: err instanceof Error && err.message === 'retryable',
    })

    const promise = withRetry(RetryPolicy.default(), op, classify)
    await vi.runAllTimersAsync()

    await expect(promise).resolves.toBe('success-3')
    expect(op).toHaveBeenCalledTimes(3)
    expect(op).toHaveBeenNthCalledWith(1, 1)
    expect(op).toHaveBeenNthCalledWith(2, 2)
    expect(op).toHaveBeenNthCalledWith(3, 3)
  })

  it('respects maxRetries limit', async () => {
    vi.useFakeTimers()
    const op = vi.fn(async () => {
      throw new Error('always retryable')
    })
    const classify = () => ({ retryable: true })
    const policy = new RetryPolicy({ maxRetries: 2 })

    const promise = withRetry(policy, op, classify)
    const assertion = expect(promise).rejects.toThrow('always retryable')
    await vi.runAllTimersAsync()

    await assertion
    expect(op).toHaveBeenCalledTimes(3)
  })

  it('honors Retry-After when respectRetryAfter=true', async () => {
    vi.useFakeTimers()
    const op = vi.fn(async (attempt: number) => {
      if (attempt === 1) {
        throw new Error('retry-after-500ms')
      }
      return 'success'
    })
    const classify = (err: unknown) => ({
      retryable: err instanceof Error && err.message === 'retry-after-500ms',
      retryAfterMs: 500,
    })
    const setTimeoutSpy = vi.spyOn(globalThis, 'setTimeout')

    const promise = withRetry(RetryPolicy.default(), op, classify)
    await vi.runAllTimersAsync()

    await expect(promise).resolves.toBe('success')
    expect(setTimeoutSpy).toHaveBeenCalledWith(expect.any(Function), 500)
  })

  it('ignores Retry-After when respectRetryAfter=false', async () => {
    vi.useFakeTimers()
    const op = vi.fn(async (attempt: number) => {
      if (attempt === 1) {
        throw new Error('retryable')
      }
      return 'success'
    })
    const classify = () => ({ retryable: true, retryAfterMs: 10_000 })
    const policy = new RetryPolicy({ jitter: false, respectRetryAfter: false })
    const setTimeoutSpy = vi.spyOn(globalThis, 'setTimeout')

    const promise = withRetry(policy, op, classify)
    await vi.runAllTimersAsync()

    await expect(promise).resolves.toBe('success')
    expect(setTimeoutSpy).toHaveBeenCalledWith(expect.any(Function), 100)
  })

  it('throws non-retryable errors immediately', async () => {
    const op = vi.fn(async () => {
      throw new Error('non-retryable')
    })
    const classify = () => ({ retryable: false })

    await expect(withRetry(RetryPolicy.default(), op, classify)).rejects.toThrow(
      'non-retryable',
    )
    expect(op).toHaveBeenCalledOnce()
  })
})

describe('withRetry with error.ts classification', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    vi.useRealTimers()
  })

  it('retries on retryable status codes', async () => {
    vi.useFakeTimers()
    let callCount = 0
    const op = vi.fn(async () => {
      callCount += 1
      if (callCount === 1) {
        const err = new Error('429 Too Many Requests') as Error & { status?: number }
        err.status = 429
        throw err
      }
      return 'success'
    })

    const promise = withRetry(RetryPolicy.default(), op, retryableByStatusOrNetwork)
    await vi.runAllTimersAsync()

    await expect(promise).resolves.toBe('success')
    expect(op).toHaveBeenCalledTimes(2)
  })

  it('does not retry on non-retryable status codes', async () => {
    const op = vi.fn(async () => {
      const err = new Error('400 Bad Request') as Error & { status?: number }
      err.status = 400
      throw err
    })

    await expect(
      withRetry(RetryPolicy.default(), op, retryableByStatusOrNetwork),
    ).rejects.toThrow('400 Bad Request')
    expect(op).toHaveBeenCalledOnce()
  })

  it('parses and respects Retry-After header via parseRetryAfter', async () => {
    vi.useFakeTimers()
    let callCount = 0
    const op = vi.fn(async () => {
      callCount += 1
      if (callCount === 1) {
        throw new Error('retry-after-header')
      }
      return 'success'
    })
    const classify = (err: unknown) => ({
      retryable: err instanceof Error && err.message === 'retry-after-header',
      retryAfterMs: parseRetryAfter('2'),
    })
    const setTimeoutSpy = vi.spyOn(globalThis, 'setTimeout')

    const promise = withRetry(RetryPolicy.default(), op, classify)
    await vi.runAllTimersAsync()

    await expect(promise).resolves.toBe('success')
    expect(setTimeoutSpy).toHaveBeenCalledWith(expect.any(Function), 2000)
  })

  it('retries on retryable network errors', async () => {
    vi.useFakeTimers()
    let callCount = 0
    const op = vi.fn(async () => {
      callCount += 1
      if (callCount === 1) {
        const err = new Error('connection timeout') as Error & { code?: string }
        err.code = 'ETIMEDOUT'
        throw err
      }
      return 'success'
    })

    const promise = withRetry(RetryPolicy.default(), op, retryableByStatusOrNetwork)
    await vi.runAllTimersAsync()

    await expect(promise).resolves.toBe('success')
    expect(op).toHaveBeenCalledTimes(2)
  })
})
