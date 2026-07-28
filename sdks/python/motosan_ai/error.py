class MotosanError(Exception):
    def __init__(
        self,
        message: str = "",
        *,
        status_code: int | None = None,
        retry_after: float | None = None,
        request_id: str | None = None,
    ) -> None:
        super().__init__(message)
        self.status_code = status_code
        self.retry_after = retry_after
        self.request_id = request_id


class AuthError(MotosanError):
    pass


class RateLimitError(MotosanError):
    pass


class InvalidRequestError(MotosanError):
    pass


class ConfigError(MotosanError):
    pass


class ProviderError(MotosanError):
    pass


class NetworkError(MotosanError):
    pass


class StreamError(MotosanError):
    pass


class IncompleteStreamError(StreamError):
    """The upstream ended without the provider's terminal event.

    Deliberately subclasses StreamError as a migration softener: existing
    ``except StreamError`` handlers keep catching truncation; catch this type
    to handle truncation specifically. Message convention:
    ``"incomplete stream: <provider> ended without a terminal event"``.
    """


class StreamReadTimeoutError(MotosanError):
    """A streaming response body went idle past ``read_idle_timeout``.

    Raised after response headers arrive when no bytes show up within the
    idle window. Deliberately not a NetworkError: it is never retried -
    mid-stream, a retry would replay already-yielded deltas. Mirrors Rust
    ``StreamReadTimeout`` / TS ``StreamReadTimeoutError``.
    """


class UnsupportedFeatureError(InvalidRequestError):
    """The provider cannot serve a feature the request asked for.

    Raised before any network I/O: native Freeform tool specs or history on a
    provider that does not support them, image/document content on a provider
    that does not accept it, or a native model request on a provider with no
    native path.

    Deliberately subclasses InvalidRequestError as a migration softener:
    existing ``except InvalidRequestError`` handlers keep working, while
    callers that must distinguish match this subclass. Non-retryable by
    inheritance. Mirrors Rust ``MotosanError::UnsupportedFeature`` and
    TypeScript ``UnsupportedFeatureError``.
    """
