import { extractErrorMessage, mapHttpError, ProviderError } from '../error.js'

export interface FetchOptions {
  signal?: AbortSignal
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
  if (options?.signal) {
    fetchOptions.signal = options.signal
  }

  const response = await fetch(url, fetchOptions)

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
  if (options?.signal) {
    fetchOptions.signal = options.signal
  }

  const response = await fetch(url, fetchOptions)

  if (!response.ok) {
    await throwMappedError(response)
  }

  if (!response.body) {
    throw new ProviderError('response body is null')
  }

  return response.body
}
