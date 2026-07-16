"""Mirrors specs/retry.md — update BOTH or neither.

Cross-SDK siblings assert the SAME tables:
  sdks/rust/src/providers/mod.rs (mod retry_conformance)
  sdks/typescript/tests/retry-conformance.test.ts
A failure means the implementation drifted from specs/retry.md — fix the
implementation (or the spec plus all three suites), never this file alone.
"""

import pytest

from motosan_ai.error import (
    AuthError,
    InvalidRequestError,
    MotosanError,
    NetworkError,
    ProviderError,
    RateLimitError,
)
from motosan_ai.retry import (
    RetryPolicy,
    _is_retryable,
    compute_delay,
    parse_retry_after_header,
)

RETRYABLE_STATUSES = [408, 409, 429, 500, 502, 503, 529, 599]
NON_RETRYABLE_STATUSES = [400, 401, 403, 404, 422, 499]


def error_for_status(status: int) -> MotosanError:
    """Construct the exception map_http_error produces for this status (D1 mapping).

    429 -> RateLimitError, so its retryability is exercised via isinstance,
    matching real provider raise sites. (200/301 are not error statuses, so
    Python's attribute-based table omits them; Rust/TS classify raw codes.)
    """
    message = f"HTTP {status}: boom"
    if status == 401:
        return AuthError(message, status_code=status)
    if status == 429:
        return RateLimitError(message, status_code=status)
    if status == 400:
        return InvalidRequestError(message, status_code=status)
    return ProviderError(message, status_code=status)


class TestClassificationTable:
    @pytest.mark.parametrize("status", RETRYABLE_STATUSES)
    def test_retryable(self, status):
        assert _is_retryable(error_for_status(status)) is True

    @pytest.mark.parametrize("status", NON_RETRYABLE_STATUSES)
    def test_non_retryable(self, status):
        assert _is_retryable(error_for_status(status)) is False

    def test_network_error_is_retryable(self):
        assert _is_retryable(NetworkError("connection reset")) is True

    def test_message_text_is_ignored(self):
        # D9: classification is attribute-based; "500" in the text no longer counts.
        assert _is_retryable(ProviderError("Error code: 500 - server error")) is False

    def test_plain_exception_not_retryable(self):
        assert _is_retryable(ValueError("bad")) is False


class TestParseRetryAfterHeader:
    def test_integer_seconds(self):
        assert parse_retry_after_header("5") == 5.0
        assert parse_retry_after_header("0") == 0.0
        assert parse_retry_after_header(" 7 ") == 7.0

    def test_capped_at_60s(self):
        assert parse_retry_after_header("61") == 60.0
        assert parse_retry_after_header("86400") == 60.0

    def test_http_date_clamped(self):
        # Deterministic: past date clamps to 0; far-future date clamps to the cap.
        assert parse_retry_after_header("Wed, 21 Oct 2015 07:28:00 GMT") == 0.0
        assert parse_retry_after_header("Fri, 31 Dec 2100 23:59:59 GMT") == 60.0

    def test_garbage_is_none(self):
        # A negative integer is invalid, NOT clamp-to-0 (only past HTTP-dates clamp).
        assert parse_retry_after_header(None) is None
        assert parse_retry_after_header("") is None
        assert parse_retry_after_header("soon") is None
        assert parse_retry_after_header("-5") is None


class TestPolicyMath:
    def test_full_jitter_scales_capped_exp_delay_by_rng(self):
        policy = RetryPolicy()  # base 0.1s, max 2.0s, jitter on
        # compute_delay is a free function; RetryPolicy has no delay method.
        assert compute_delay(policy, 1, rng=lambda: 0.5) == pytest.approx(0.05)
        assert compute_delay(policy, 2, rng=lambda: 0.5) == pytest.approx(0.1)
        assert compute_delay(policy, 3, rng=lambda: 0.5) == pytest.approx(0.2)
        assert compute_delay(policy, 4, rng=lambda: 0.0) == 0.0
        # attempt 6: uncapped exp = 0.1 * 2**5 = 3.2 -> capped at 2.0 BEFORE jitter.
        assert compute_delay(policy, 6, rng=lambda: 1.0) == pytest.approx(2.0)

    def test_jitter_disabled_is_pure_exponential(self):
        policy = RetryPolicy(jitter=False)
        assert compute_delay(policy, 1) == pytest.approx(0.1)
        assert compute_delay(policy, 2) == pytest.approx(0.2)
        assert compute_delay(policy, 5) == pytest.approx(1.6)
        assert compute_delay(policy, 6) == pytest.approx(2.0)

    def test_defaults_match_spec(self):
        policy = RetryPolicy()
        assert policy.max_retries == 3
        assert policy.base_delay == 0.1
        assert policy.max_delay == 2.0
        assert policy.jitter is True
        assert policy.respect_retry_after is True
