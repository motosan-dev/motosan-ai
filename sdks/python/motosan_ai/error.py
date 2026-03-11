class MotosanError(Exception):
    pass


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
