import random
from datetime import datetime, timedelta, timezone
from email.utils import format_datetime

import pytest

from motosan_ai.error import AuthError, NetworkError, ProviderError, RateLimitError
from motosan_ai.retry import (
    RETRY_AFTER_CAP_SECS,
    RetryEvent,
    RetryPolicy,
    _is_retryable,
    compute_delay,
    parse_retry_after_header,
    with_retry,
)


class TestParseRetryAfterHeader:
    def test_integer_seconds(self):
        assert parse_retry_after_header("5") == pytest.approx(5.0)

    def test_decimal_seconds(self):
        assert parse_retry_after_header("1.5") == pytest.approx(1.5)

    def test_caps_seconds(self):
        assert parse_retry_after_header("120") == pytest.approx(RETRY_AFTER_CAP_SECS)

    def test_negative_seconds_returns_none(self):
        assert parse_retry_after_header("-1") is None

    def test_future_http_date(self):
        retry_at = datetime.now(timezone.utc) + timedelta(seconds=25)

        parsed = parse_retry_after_header(format_datetime(retry_at, usegmt=True))

        assert parsed is not None
        assert 20.0 <= parsed <= 30.0

    def test_past_http_date(self):
        retry_at = datetime.now(timezone.utc) - timedelta(seconds=25)

        assert parse_retry_after_header(format_datetime(retry_at, usegmt=True)) == pytest.approx(
            0.0
        )

    @pytest.mark.parametrize("value", [None, "", "not a date"])
    def test_none_empty_and_garbage_return_none(self, value):
        assert parse_retry_after_header(value) is None


class TestIsRetryable:
    def test_retryable_classes_and_statuses(self):
        assert _is_retryable(RateLimitError("slow down", status_code=429)) is True
        assert _is_retryable(NetworkError("connection reset")) is True
        assert _is_retryable(ProviderError("HTTP 500: boom", status_code=500)) is True
        assert _is_retryable(ProviderError("HTTP 408: timeout", status_code=408)) is True
        assert _is_retryable(ProviderError("HTTP 409: conflict", status_code=409)) is True

    def test_not_retryable(self):
        assert _is_retryable(ProviderError("HTTP 400: bad request", status_code=400)) is False
        assert _is_retryable(AuthError("unauthorized", status_code=401)) is False
        assert _is_retryable(ValueError("bad")) is False

    def test_provider_error_without_status_not_retryable(self):
        assert _is_retryable(ProviderError("Error code: 500 - server error")) is False


class TestComputeDelay:
    def test_full_jitter_scales_rng_against_exponential_ceiling(self):
        policy = RetryPolicy()

        assert compute_delay(policy, 1, None, rng=lambda: 0.5) == pytest.approx(0.05)
        assert compute_delay(policy, 2, None, rng=lambda: 0.5) == pytest.approx(0.1)
        assert compute_delay(policy, 6, None, rng=lambda: 1.0) == pytest.approx(2.0)
        assert compute_delay(policy, 3, None, rng=lambda: 0.0) == pytest.approx(0.0)

    def test_seeded_rng_stays_within_bounds(self):
        policy = RetryPolicy()
        rng = random.Random(42).random

        for attempt in (1, 2, 3, 6):
            ceiling = min(0.1 * 2 ** (attempt - 1), 2.0)
            assert 0.0 <= compute_delay(policy, attempt, None, rng=rng) <= ceiling

    def test_no_jitter_is_pure_exponential(self):
        policy = RetryPolicy(jitter=False)

        assert compute_delay(policy, 1, None) == pytest.approx(0.1)
        assert compute_delay(policy, 3, None) == pytest.approx(0.4)

    def test_retry_after_verbatim_capped_at_60_not_max_delay(self):
        policy = RetryPolicy()

        assert compute_delay(policy, 1, 7.0, rng=lambda: 0.5) == pytest.approx(7.0)
        assert compute_delay(policy, 1, 120.0) == pytest.approx(60.0)

    def test_retry_after_ignored_when_respect_disabled(self):
        policy = RetryPolicy(respect_retry_after=False, jitter=False)

        assert compute_delay(policy, 1, 7.0) == pytest.approx(0.1)


class TestWithRetry:
    @pytest.mark.asyncio
    async def test_succeeds_first_try(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            return "ok"

        assert await with_retry(fn, max_retries=3) == "ok"
        assert calls == 1

    @pytest.mark.asyncio
    async def test_retries_on_rate_limit_with_policy(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            if calls < 3:
                raise RateLimitError("slow down", status_code=429)
            return "ok"

        policy = RetryPolicy(max_retries=3, base_delay=0.001)

        assert await with_retry(fn, policy=policy) == "ok"
        assert calls == 3

    @pytest.mark.asyncio
    async def test_legacy_positional_args_still_work(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            if calls < 2:
                raise NetworkError("connection reset")
            return "ok"

        assert await with_retry(fn, 3, 0.001, 2.0) == "ok"
        assert calls == 2

    @pytest.mark.asyncio
    async def test_does_not_retry_provider_error_without_status(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            raise ProviderError("Error code: 500 - server error")

        with pytest.raises(ProviderError):
            await with_retry(fn, max_retries=3, initial_backoff=0.001)
        assert calls == 1

    @pytest.mark.asyncio
    async def test_exhausts_retries(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            raise ProviderError("HTTP 503: overloaded", status_code=503)

        with pytest.raises(ProviderError):
            await with_retry(fn, policy=RetryPolicy(max_retries=2, base_delay=0.001))
        assert calls == 3

    @pytest.mark.asyncio
    async def test_uses_retry_after_attribute_verbatim(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            if calls == 1:
                raise RateLimitError("slow down", status_code=429, retry_after=0.001)
            return "ok"

        events: list[RetryEvent] = []
        policy = RetryPolicy(max_retries=2, base_delay=0.05, on_retry=events.append)

        assert await with_retry(fn, policy=policy) == "ok"
        assert events[0].delay == pytest.approx(0.001)

    @pytest.mark.asyncio
    async def test_on_retry_fires_before_each_sleep(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            if calls == 1:
                raise NetworkError("timeout")
            if calls == 2:
                raise RateLimitError("slow down", status_code=429)
            return "ok"

        events: list[RetryEvent] = []
        policy = RetryPolicy(max_retries=3, base_delay=0.001, on_retry=events.append)

        assert await with_retry(fn, policy=policy) == "ok"
        assert [e.attempt for e in events] == [1, 2]
        assert events[0].cause == "network:timeout"
        assert events[1].cause == "status:429"
        assert all(e.delay >= 0.0 for e in events)

    @pytest.mark.asyncio
    async def test_injected_rng_drives_jitter(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            if calls == 1:
                raise RateLimitError("slow down", status_code=429)
            return "ok"

        events: list[RetryEvent] = []
        policy = RetryPolicy(max_retries=1, base_delay=0.002, on_retry=events.append)

        assert await with_retry(fn, policy=policy, rng=lambda: 0.5) == "ok"
        assert events[0].delay == pytest.approx(0.001)

    @pytest.mark.asyncio
    async def test_max_retries_zero_does_not_retry(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            raise RateLimitError("slow down", status_code=429)

        with pytest.raises(RateLimitError):
            await with_retry(fn, max_retries=0)
        assert calls == 1
