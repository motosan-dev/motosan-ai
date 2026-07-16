// Mirrors specs/retry.md — update BOTH or neither.
// Cross-SDK siblings assert the SAME tables:
//   sdks/rust/src/providers/mod.rs (mod retry_conformance)
//   sdks/python/tests/test_retry_conformance.py
// A failure means the implementation drifted from specs/retry.md — fix the
// implementation (or the spec plus all three suites), never this file alone.
import { describe, expect, it } from 'vitest'
import { isRetryableStatus, parseRetryAfter } from '../src/error.js'
import { RetryPolicy } from '../src/retry.js'

const RETRYABLE = [408, 409, 429, 500, 502, 503, 529, 599]
const NON_RETRYABLE = [200, 301, 400, 401, 403, 404, 422, 499]

describe('specs/retry.md § classification', () => {
  it.each(RETRYABLE)('status %i is retryable', (status) => {
    expect(isRetryableStatus(status)).toBe(true)
  })

  it.each(NON_RETRYABLE)('status %i is NOT retryable', (status) => {
    expect(isRetryableStatus(status)).toBe(false)
  })
})

describe('specs/retry.md § Retry-After', () => {
  it('parses integer seconds to milliseconds', () => {
    expect(parseRetryAfter('5')).toBe(5000)
    expect(parseRetryAfter('0')).toBe(0)
    expect(parseRetryAfter(' 7 ')).toBe(7000)
  })

  it('caps at 60 seconds', () => {
    expect(parseRetryAfter('61')).toBe(60_000)
    expect(parseRetryAfter('86400')).toBe(60_000)
  })

  it('parses HTTP-date, clamped to [0, 60s]', () => {
    // Deterministic: past date clamps to 0; far-future date clamps to the cap.
    expect(parseRetryAfter('Wed, 21 Oct 2015 07:28:00 GMT')).toBe(0)
    expect(parseRetryAfter('Fri, 31 Dec 2100 23:59:59 GMT')).toBe(60_000)
  })

  it('returns undefined for garbage', () => {
    // A negative integer is invalid, NOT clamp-to-0 (only past HTTP-dates clamp).
    expect(parseRetryAfter(null)).toBeUndefined()
    expect(parseRetryAfter('')).toBeUndefined()
    expect(parseRetryAfter('soon')).toBeUndefined()
    expect(parseRetryAfter('-5')).toBeUndefined()
  })
})

describe('specs/retry.md § backoff (full jitter)', () => {
  it('scales the capped exponential delay by the injected random()', () => {
    const half = new RetryPolicy({ random: () => 0.5 }) // base 100, max 2000
    expect(half.delayForAttempt(1)).toBe(50)
    expect(half.delayForAttempt(2)).toBe(100)
    expect(half.delayForAttempt(3)).toBe(200)
    expect(new RetryPolicy({ random: () => 0 }).delayForAttempt(4)).toBe(0)
    // attempt 6: uncapped exp = 100 * 2^5 = 3200 → capped at 2000 BEFORE jitter.
    expect(new RetryPolicy({ random: () => 1 }).delayForAttempt(6)).toBe(2000)
  })

  it('jitter=false is pure exponential and ignores random', () => {
    const policy = new RetryPolicy({ jitter: false, random: () => 0.123 })
    expect(policy.delayForAttempt(1)).toBe(100)
    expect(policy.delayForAttempt(2)).toBe(200)
    expect(policy.delayForAttempt(5)).toBe(1600)
    expect(policy.delayForAttempt(6)).toBe(2000)
  })

  it('defaults match spec', () => {
    const policy = RetryPolicy.default()
    expect(policy.maxRetries).toBe(3)
    expect(policy.baseDelayMs).toBe(100)
    expect(policy.maxDelayMs).toBe(2000)
    expect(policy.jitter).toBe(true)
    expect(policy.respectRetryAfter).toBe(true)
  })
})
