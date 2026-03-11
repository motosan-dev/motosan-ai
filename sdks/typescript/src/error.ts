export class MotosanError extends Error {}
export class AuthError extends MotosanError {}
export class RateLimitError extends MotosanError {}
export class InvalidRequestError extends MotosanError {}
export class ConfigError extends MotosanError {}
export class ProviderError extends MotosanError {}
export class NetworkError extends MotosanError {}
export class StreamError extends MotosanError {}

export function mapHttpError(status: number, message: string): MotosanError {
  if (status === 401) return new AuthError(message)
  if (status === 429) return new RateLimitError(message)
  if (status === 400) return new InvalidRequestError(message)
  return new ProviderError(message)
}
