import { describe, it, expect, vi, afterEach } from 'vitest'
import { postJson, postStream } from '../src/http/fetch.js'
import { ProviderError, AuthError, RateLimitError, InvalidRequestError } from '../src/error.js'

describe('http/fetch', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  describe('postJson', () => {
    it('returns parsed JSON body on 200 response', async () => {
      const mockResponse = {
        ok: true,
        status: 200,
        json: vi.fn().mockResolvedValue({ result: 'success' }),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      const result = await postJson('https://api.test.com/v1/messages', {}, { test: true })

      expect(result).toEqual({ result: 'success' })
      expect(fetch).toHaveBeenCalledWith(
        'https://api.test.com/v1/messages',
        expect.objectContaining({
          method: 'POST',
          headers: expect.any(Object),
          body: JSON.stringify({ test: true }),
        }),
      )
    })

    it('throws InvalidRequestError (mapped via extractErrorMessage) on 400', async () => {
      const mockResponse = {
        ok: false,
        status: 400,
        text: vi.fn().mockResolvedValue(JSON.stringify({ error: { message: 'bad request' } })),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await expect(
        postJson('https://api.test.com/v1/messages', {}, { test: true }),
      ).rejects.toThrow(InvalidRequestError)
    })

    it('throws AuthError on 401 with the extracted message', async () => {
      const mockResponse = {
        ok: false,
        status: 401,
        text: vi.fn().mockResolvedValue(JSON.stringify({ error: { message: 'unauthorized' } })),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await expect(
        postJson('https://api.test.com/v1/messages', {}, { test: true }),
      ).rejects.toThrow('unauthorized')
    })

    it('throws RateLimitError on 429 response', async () => {
      const mockResponse = {
        ok: false,
        status: 429,
        text: vi.fn().mockResolvedValue(JSON.stringify({ error: { message: 'rate limited' } })),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await expect(
        postJson('https://api.test.com/v1/messages', {}, { test: true }),
      ).rejects.toThrow(RateLimitError)
    })

    it('throws ProviderError on 500 response', async () => {
      const mockResponse = {
        ok: false,
        status: 500,
        text: vi.fn().mockResolvedValue(JSON.stringify({ error: { message: 'server error' } })),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await expect(
        postJson('https://api.test.com/v1/messages', {}, { test: true }),
      ).rejects.toThrow(ProviderError)
    })

    it('falls back to "HTTP <status>" when the body has no error message', async () => {
      const mockResponse = {
        ok: false,
        status: 500,
        text: vi.fn().mockResolvedValue('not json'),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await expect(
        postJson('https://api.test.com/v1/messages', {}, { test: true }),
      ).rejects.toThrow('HTTP 500')
    })

    it('respects AbortSignal in FetchOptions', async () => {
      const controller = new AbortController()
      const mockResponse = {
        ok: true,
        status: 200,
        json: vi.fn().mockResolvedValue({ result: 'success' }),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await postJson(
        'https://api.test.com/v1/messages',
        {},
        { test: true },
        { signal: controller.signal },
      )

      expect(fetch).toHaveBeenCalledWith(
        'https://api.test.com/v1/messages',
        expect.objectContaining({ signal: controller.signal }),
      )
    })

    it('includes custom headers in request', async () => {
      const mockResponse = {
        ok: true,
        status: 200,
        json: vi.fn().mockResolvedValue({ result: 'success' }),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await postJson(
        'https://api.test.com/v1/messages',
        { 'x-api-key': 'test-key', 'content-type': 'application/json' },
        { test: true },
      )

      expect(fetch).toHaveBeenCalledWith(
        'https://api.test.com/v1/messages',
        expect.objectContaining({
          headers: expect.objectContaining({
            'x-api-key': 'test-key',
            'content-type': 'application/json',
          }),
        }),
      )
    })
  })

  describe('postStream', () => {
    it('returns the ReadableStream body on 200 response', async () => {
      const mockReadableStream = new ReadableStream<Uint8Array>({
        start(controller) {
          controller.enqueue(new TextEncoder().encode('chunk1'))
          controller.close()
        },
      })
      const mockResponse = {
        ok: true,
        status: 200,
        body: mockReadableStream,
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      const result = await postStream('https://api.test.com/v1/stream', {}, { test: true })

      expect(result).toBe(mockReadableStream)
    })

    it('throws InvalidRequestError on 400 response', async () => {
      const mockResponse = {
        ok: false,
        status: 400,
        text: vi.fn().mockResolvedValue(JSON.stringify({ error: { message: 'bad request' } })),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await expect(
        postStream('https://api.test.com/v1/stream', {}, { test: true }),
      ).rejects.toThrow(InvalidRequestError)
    })

    it('throws ProviderError on 5xx response', async () => {
      const mockResponse = {
        ok: false,
        status: 502,
        text: vi.fn().mockResolvedValue(JSON.stringify({ error: { message: 'bad gateway' } })),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await expect(
        postStream('https://api.test.com/v1/stream', {}, { test: true }),
      ).rejects.toThrow(ProviderError)
    })
  })
})
