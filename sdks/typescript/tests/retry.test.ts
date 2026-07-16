import { afterEach, describe, expect, it, vi } from 'vitest'
import { isRetryableNetworkError, isRetryableStatus, parseRetryAfter } from '../src/error.js'
import { classifyForRetry, RetryPolicy, withRetry, type RetryEvent } from '../src/retry.js'

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

describe('RetryPolicy full jitter', () => {
  it('uses injectable random: delay = random() * expDelay', () => {
    const policy = new RetryPolicy({ random: () => 0.5 })

    expect(policy.delayForAttempt(1)).toBe(50)
    expect(policy.delayForAttempt(2)).toBe(100)
    expect(policy.delayForAttempt(3)).toBe(200)
    expect(policy.delayForAttempt(6)).toBe(1000)
  })

  it('random extremes map to 0 and the full exponential delay', () => {
    const floor = new RetryPolicy({ random: () => 0 })
    const ceil = new RetryPolicy({ random: () => 1 })

    expect(floor.delayForAttempt(3)).toBe(0)
    expect(ceil.delayForAttempt(3)).toBe(400)
    expect(ceil.delayForAttempt(6)).toBe(2000)
  })

  it('defaults random to Math.random and stays within [0, expDelay]', () => {
    const policy = RetryPolicy.default()

    expect(policy.random).toBe(Math.random)
    for (let i = 0; i < 200; i += 1) {
      const first = policy.delayForAttempt(1)
      expect(first).toBeGreaterThanOrEqual(0)
      expect(first).toBeLessThanOrEqual(100)

      const capped = policy.delayForAttempt(6)
      expect(capped).toBeGreaterThanOrEqual(0)
      expect(capped).toBeLessThanOrEqual(2000)
    }
  })

  it('jitter=false returns the exact exponential delay and ignores random', () => {
    const policy = new RetryPolicy({ jitter: false, random: () => 0.123 })

    expect(policy.delayForAttempt(1)).toBe(100)
    expect(policy.delayForAttempt(2)).toBe(200)
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

describe('withRetry onRetry', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    vi.useRealTimers()
  })

  it('fires before each sleep with attempt, delayMs, cause', async () => {
    vi.useFakeTimers()
    const events: RetryEvent[] = []
    const policy = new RetryPolicy({
      maxRetries: 3,
      jitter: false,
      onRetry: (evt) => events.push(evt),
    })
    const op = vi.fn(async (attempt: number) => {
      if (attempt < 3) {
        const err = new Error(`boom-${attempt}`) as Error & { status?: number }
        err.status = 503
        throw err
      }
      return 'ok'
    })
    const classify = (err: unknown) => ({
      retryable: (err as { status?: number }).status === 503,
    })

    const promise = withRetry(policy, op, classify)
    await vi.runAllTimersAsync()

    await expect(promise).resolves.toBe('ok')
    expect(events).toEqual([
      { attempt: 1, delayMs: 100, cause: 'boom-1' },
      { attempt: 2, delayMs: 200, cause: 'boom-2' },
    ])
  })

  it('reports verbatim retryAfterMs as delayMs even with jitter enabled', async () => {
    vi.useFakeTimers()
    const events: RetryEvent[] = []
    const policy = new RetryPolicy({ onRetry: (evt) => events.push(evt) })
    const op = vi.fn(async (attempt: number) => {
      if (attempt === 1) {
        throw new Error('throttled')
      }
      return 'ok'
    })
    const classify = () => ({ retryable: true, retryAfterMs: 500 })

    const promise = withRetry(policy, op, classify)
    await vi.runAllTimersAsync()

    await expect(promise).resolves.toBe('ok')
    expect(events).toEqual([{ attempt: 1, delayMs: 500, cause: 'throttled' }])
  })

  it('does not fire for non-retryable errors', async () => {
    const events: RetryEvent[] = []
    const policy = new RetryPolicy({ onRetry: (evt) => events.push(evt) })
    const op = vi.fn(async () => {
      throw new Error('fatal')
    })

    await expect(withRetry(policy, op, () => ({ retryable: false }))).rejects.toThrow('fatal')
    expect(events).toEqual([])
  })
})

describe('classifyForRetry', () => {
  it('classifies retryable-status errors and carries retryAfterMs', () => {
    const err = new Error('rate limited') as Error & { status?: number; retryAfterMs?: number }
    err.status = 429
    err.retryAfterMs = 1500
    expect(classifyForRetry(err)).toEqual({ retryable: true, retryAfterMs: 1500 })
  })

  it('returns retryable:false for non-retryable statuses without throwing', () => {
    const err = new Error('bad request') as Error & { status?: number }
    err.status = 400
    expect(classifyForRetry(err)).toEqual({ retryable: false })
  })

  it('classifies retryable network errors', () => {
    const err = new Error('refused') as Error & { code?: string }
    err.code = 'ECONNREFUSED'
    expect(classifyForRetry(err).retryable).toBe(true)
  })

  it('accepts a bare numeric status', () => {
    expect(classifyForRetry(503)).toEqual({ retryable: true })
    expect(classifyForRetry(404)).toEqual({ retryable: false })
  })

  it('treats non-Error, non-number values as not retryable', () => {
    expect(classifyForRetry('boom')).toEqual({ retryable: false })
  })
})
