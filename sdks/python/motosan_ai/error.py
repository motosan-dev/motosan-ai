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
