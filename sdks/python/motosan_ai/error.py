class MotosanError(Exception):
    """Base class for all motosan-ai errors."""


class AuthError(MotosanError):
    """Invalid or missing API key."""


class RateLimitError(MotosanError):
    """Too many requests."""
    def __init__(self, message: str, retry_after: int | None = None):
        super().__init__(message)
        self.retry_after = retry_after


class InvalidRequestError(MotosanError):
    """Bad request parameters."""


class ProviderError(MotosanError):
    """Provider returned an error status."""
    def __init__(self, message: str, status: int):
        super().__init__(message)
        self.status = status


class NetworkError(MotosanError):
    """Connection or timeout failure."""


class StreamError(MotosanError):
    """SSE stream parsing failure."""
