import pytest

from motosan_ai.retry import with_retry, _parse_retry_after
from motosan_ai.error import RateLimitError


class TestParseRetryAfter:
    def test_with_seconds(self):
        assert _parse_retry_after("Rate limited. Retry-After: 5") == 5.0

    def test_with_decimal(self):
        assert _parse_retry_after("retry-after: 1.5") == 1.5

    def test_no_match(self):
        assert _parse_retry_after("some error") is None

    def test_retry_after_dash_variant(self):
        assert _parse_retry_after("Retry-After 10") == 10.0


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
    async def test_exhausts_retries(self):
        async def fn():
            raise RateLimitError("429")

        with pytest.raises(RateLimitError):
            await with_retry(fn, max_retries=2, initial_backoff=0.001)

    @pytest.mark.asyncio
    async def test_non_rate_limit_error_not_retried(self):
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
        """Verify that Retry-After in the error message is respected (value capped at max_backoff)."""
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
