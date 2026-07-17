import { extractErrorMessage, mapHttpError, ProviderError } from '../error.js'

export interface FetchOptions {
  /** Caller signal (streams) or caller+totalMs composition (chat). Stays armed for the whole call. */
  signal?: AbortSignal
  /**
   * E4 connect budget. Arms a timer-driven AbortController that is DISARMED
   * the moment `await fetch(...)` resolves (headers received), so it never
   * bounds body reads and a slow generation cannot trip it.
   */
  preHeadersTimeoutMs?: number
}

async function fetchWithTimeouts(
  url: string,
  fetchOptions: RequestInit,
  options?: FetchOptions,
): Promise<Response> {
  const signals: AbortSignal[] = []
  if (options?.signal) signals.push(options.signal)
  let timer: ReturnType<typeof setTimeout> | undefined
  if (options?.preHeadersTimeoutMs !== undefined) {
    const connectController = new AbortController()
    timer = setTimeout(() => connectController.abort(), options.preHeadersTimeoutMs)
    signals.push(connectController.signal)
  }
  if (signals.length === 1) fetchOptions.signal = signals[0]
  if (signals.length > 1) fetchOptions.signal = AbortSignal.any(signals)
  try {
    return await fetch(url, fetchOptions)
  } finally {
    if (timer !== undefined) clearTimeout(timer)
  }
}

async function throwMappedError(response: Response): Promise<never> {
  const text = await response.text()
  let payload: unknown
  try {
    payload = JSON.parse(text)
  } catch {
    payload = text
  }
  const message = extractErrorMessage(payload, `HTTP ${response.status}`)
  const requestId =
    response.headers?.get('request-id') ?? response.headers?.get('x-request-id') ?? null
  throw mapHttpError(
    response.status,
    message,
    response.headers?.get('retry-after') ?? null,
    requestId,
  )
}

export async function postJson<T = unknown>(
  url: string,
  headers: Record<string, string>,
  body: unknown,
  options?: FetchOptions,
): Promise<T> {
  const fetchOptions: RequestInit = {
    method: 'POST',
    headers: { 'content-type': 'application/json', ...headers },
    body: JSON.stringify(body),
  }

  const response = await fetchWithTimeouts(url, fetchOptions, options)

  if (!response.ok) {
    await throwMappedError(response)
  }

  return response.json() as Promise<T>
}

export async function postStream(
  url: string,
  headers: Record<string, string>,
  body: unknown,
  options?: FetchOptions,
): Promise<ReadableStream<Uint8Array>> {
  const fetchOptions: RequestInit = {
    method: 'POST',
    headers: { 'content-type': 'application/json', ...headers },
    body: JSON.stringify(body),
  }

  const response = await fetchWithTimeouts(url, fetchOptions, options)

  if (!response.ok) {
    await throwMappedError(response)
  }

  if (!response.body) {
    throw new ProviderError('response body is null')
  }

  return response.body
}
