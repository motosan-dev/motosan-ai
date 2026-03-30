import pytest

from motosan_ai.error import (
    AuthError,
    NetworkError,
    ProviderError,
    RateLimitError,
)
from motosan_ai.retry import _is_retryable, _parse_retry_after, with_retry


class TestParseRetryAfter:
    def test_with_seconds(self):
        assert _parse_retry_after("Rate limited. Retry-After: 5") == 5.0

    def test_with_decimal(self):
        assert _parse_retry_after("retry-after: 1.5") == 1.5

    def test_no_match(self):
        assert _parse_retry_after("some error") is None

    def test_retry_after_dash_variant(self):
        assert _parse_retry_after("Retry-After 10") == 10.0


class TestIsRetryable:
    def test_rate_limit_error(self):
        assert _is_retryable(RateLimitError("429")) is True

    def test_network_error(self):
        assert _is_retryable(NetworkError("connection reset")) is True

    def test_provider_error_5xx(self):
        assert _is_retryable(ProviderError("Error code: 500 - server error")) is True

    def test_provider_error_502(self):
        assert _is_retryable(ProviderError("502 Bad Gateway")) is True

    def test_provider_error_503(self):
        assert _is_retryable(ProviderError("503 Service Unavailable")) is True

    def test_provider_error_4xx_not_retryable(self):
        assert _is_retryable(ProviderError("Error code: 400 - bad request")) is False

    def test_auth_error_not_retryable(self):
        assert _is_retryable(AuthError("401 unauthorized")) is False

    def test_value_error_not_retryable(self):
        assert _is_retryable(ValueError("bad")) is False


class TestWithRetry:
    @pytest.mark.asyncio
    async def test_succeeds_first_try(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            return "ok"

        result = await with_retry(fn, max_retries=3)
        assert result == "ok"
        assert calls == 1

    @pytest.mark.asyncio
    async def test_retries_on_rate_limit(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            if calls < 3:
                raise RateLimitError("429")
            return "ok"

        result = await with_retry(fn, max_retries=3, initial_backoff=0.001)
        assert result == "ok"
        assert calls == 3

    @pytest.mark.asyncio
    async def test_retries_on_network_error(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            if calls < 2:
                raise NetworkError("connection reset")
            return "ok"

        result = await with_retry(fn, max_retries=3, initial_backoff=0.001)
        assert result == "ok"
        assert calls == 2

    @pytest.mark.asyncio
    async def test_retries_on_5xx_provider_error(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            if calls < 2:
                raise ProviderError("Error code: 500 - Internal Server Error")
            return "ok"

        result = await with_retry(fn, max_retries=3, initial_backoff=0.001)
        assert result == "ok"
        assert calls == 2

    @pytest.mark.asyncio
    async def test_does_not_retry_4xx_provider_error(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            raise ProviderError("Error code: 400 - bad request")

        with pytest.raises(ProviderError):
            await with_retry(fn, max_retries=3, initial_backoff=0.001)
        assert calls == 1

    @pytest.mark.asyncio
    async def test_does_not_retry_auth_error(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            raise AuthError("401")

        with pytest.raises(AuthError):
            await with_retry(fn, max_retries=3, initial_backoff=0.001)
        assert calls == 1

    @pytest.mark.asyncio
    async def test_exhausts_retries(self):
        async def fn():
            raise RateLimitError("429")

        with pytest.raises(RateLimitError):
            await with_retry(fn, max_retries=2, initial_backoff=0.001)

    @pytest.mark.asyncio
    async def test_non_retryable_error_not_retried(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            raise ValueError("bad")

        with pytest.raises(ValueError):
            await with_retry(fn, max_retries=3, initial_backoff=0.001)
        assert calls == 1

    @pytest.mark.asyncio
    async def test_uses_retry_after_from_message(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            if calls == 1:
                raise RateLimitError("Rate limited. Retry-After: 0.001")
            return "ok"

        result = await with_retry(fn, max_retries=2, initial_backoff=0.001, max_backoff=1.0)
        assert result == "ok"
        assert calls == 2

    @pytest.mark.asyncio
    async def test_max_retries_zero_does_not_retry(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            raise RateLimitError("429")

        with pytest.raises(RateLimitError):
            await with_retry(fn, max_retries=0)
        assert calls == 1

    @pytest.mark.asyncio
    async def test_mixed_errors_retries_then_succeeds(self):
        """Network error then rate limit then success."""
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            if calls == 1:
                raise NetworkError("timeout")
            if calls == 2:
                raise RateLimitError("429")
            return "ok"

        result = await with_retry(fn, max_retries=3, initial_backoff=0.001)
        assert result == "ok"
        assert calls == 3
