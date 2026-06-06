import { describe, it, expect } from 'vitest'
import {
  StreamReadTimeoutError,
  UnsupportedFeatureError,
  isRetryableStatus,
  isRetryableNetworkError,
  parseRetryAfter,
  extractErrorMessage,
} from '../src/error.js'

describe('StreamReadTimeoutError', () => {
  it('carries timeoutSecs property', () => {
    const error = new StreamReadTimeoutError(30)
    expect(error.timeoutSecs).toBe(30)
    expect(error.message).toContain('30')
  })

  it('has correct message format', () => {
    const error = new StreamReadTimeoutError(5)
    expect(error.message).toMatch(/stream.*timeout|timeout.*stream/i)
  })
})

describe('UnsupportedFeatureError', () => {
  it('extends MotosanError', () => {
    const error = new UnsupportedFeatureError('document input not supported')
    expect(error.message).toBe('document input not supported')
  })
})

describe('isRetryableStatus', () => {
  it('returns true for 429 (rate limit)', () => {
    expect(isRetryableStatus(429)).toBe(true)
  })

  it('returns true for status >= 500', () => {
    expect(isRetryableStatus(500)).toBe(true)
    expect(isRetryableStatus(502)).toBe(true)
    expect(isRetryableStatus(503)).toBe(true)
    expect(isRetryableStatus(599)).toBe(true)
  })

  it('returns false for 401, 400, 404, 4xx (except 429)', () => {
    expect(isRetryableStatus(401)).toBe(false)
    expect(isRetryableStatus(400)).toBe(false)
    expect(isRetryableStatus(404)).toBe(false)
    expect(isRetryableStatus(499)).toBe(false)
  })

  it('returns false for 2xx and 3xx', () => {
    expect(isRetryableStatus(200)).toBe(false)
    expect(isRetryableStatus(301)).toBe(false)
  })
})

describe('isRetryableNetworkError', () => {
  it('returns true for AbortError', () => {
    const error = new Error('cancelled')
    error.name = 'AbortError'
    expect(isRetryableNetworkError(error)).toBe(true)
  })

  it('returns true for TypeError (fetch network failure)', () => {
    const error = new TypeError('fetch failed')
    expect(isRetryableNetworkError(error)).toBe(true)
  })

  it('returns true for ECONNREFUSED (connection refused)', () => {
    const error = new Error('Connection refused')
    ;(error as any).code = 'ECONNREFUSED'
    expect(isRetryableNetworkError(error)).toBe(true)
  })

  it('returns true for ENOTFOUND (DNS resolution failure)', () => {
    const error = new Error('getaddrinfo ENOTFOUND example.com')
    ;(error as any).code = 'ENOTFOUND'
    expect(isRetryableNetworkError(error)).toBe(true)
  })

  it('returns true for ETIMEDOUT (connection timeout)', () => {
    const error = new Error('Connection timeout')
    ;(error as any).code = 'ETIMEDOUT'
    expect(isRetryableNetworkError(error)).toBe(true)
  })

  it('returns false for unrelated errors', () => {
    const error = new Error('some other error')
    expect(isRetryableNetworkError(error)).toBe(false)
  })

  it('returns false for non-Error objects', () => {
    expect(isRetryableNetworkError('not an error')).toBe(false)
    expect(isRetryableNetworkError(null)).toBe(false)
    expect(isRetryableNetworkError(undefined)).toBe(false)
  })
})

describe('parseRetryAfter', () => {
  it('parses integer seconds from header value', () => {
    const result = parseRetryAfter('30')
    expect(result).toBe(30000) // 30 seconds in milliseconds
  })

  it('parses with leading/trailing whitespace', () => {
    const result = parseRetryAfter('  60  ')
    expect(result).toBe(60000)
  })

  it('returns undefined for non-integer value', () => {
    expect(parseRetryAfter('invalid')).toBeUndefined()
    expect(parseRetryAfter('30.5')).toBeUndefined()
    expect(parseRetryAfter('abc')).toBeUndefined()
  })

  it('returns undefined for null/empty string', () => {
    expect(parseRetryAfter(null)).toBeUndefined()
    expect(parseRetryAfter('')).toBeUndefined()
  })

  it('returns undefined for negative numbers', () => {
    expect(parseRetryAfter('-5')).toBeUndefined()
  })

  it('handles zero seconds', () => {
    const result = parseRetryAfter('0')
    expect(result).toBe(0)
  })

  it('handles large numbers', () => {
    const result = parseRetryAfter('3600')
    expect(result).toBe(3600000) // 1 hour in milliseconds
  })
})

describe('extractErrorMessage', () => {
  it('extracts message from {error:{message}} (Anthropic/OpenAI format)', () => {
    const body = {
      error: {
        message: 'API key is invalid',
      },
    }
    expect(extractErrorMessage(body, 'default')).toBe('API key is invalid')
  })

  it('uses fallback when error.message is missing', () => {
    const body = {
      error: {
        type: 'auth_error',
      },
    }
    expect(extractErrorMessage(body, 'authentication failed')).toBe('authentication failed')
  })

  it('uses fallback when error object is missing', () => {
    const body = {
      status: 401,
    }
    expect(extractErrorMessage(body, 'request failed')).toBe('request failed')
  })

  it('uses fallback for null body', () => {
    expect(extractErrorMessage(null, 'unknown error')).toBe('unknown error')
  })

  it('uses fallback for undefined body', () => {
    expect(extractErrorMessage(undefined, 'unknown error')).toBe('unknown error')
  })

  it('uses fallback for empty object', () => {
    expect(extractErrorMessage({}, 'fallback')).toBe('fallback')
  })

  it('uses fallback when error.message is not a string', () => {
    const body = {
      error: {
        message: 123,
      },
    }
    expect(extractErrorMessage(body, 'default')).toBe('default')
  })

  it('handles nested error structures', () => {
    const body = {
      error: {
        message: 'Rate limit exceeded: 100 requests per minute',
      },
    }
    expect(extractErrorMessage(body, 'default')).toBe('Rate limit exceeded: 100 requests per minute')
  })
})
