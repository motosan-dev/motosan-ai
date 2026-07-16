"""Retry engine with full-jitter exponential backoff.

- Classification is attribute-based: RateLimitError, NetworkError, and
  ProviderError with status 408/409/5xx retry.
- Backoff is full jitter: uniform(0, min(base_delay * 2**(attempt-1), max_delay)).
- error.retry_after is used verbatim with no jitter and a 60s hard cap.
"""

from __future__ import annotations

import asyncio
import logging
import random
from collections.abc import Awaitable, Callable
from dataclasses import dataclass
from datetime import UTC, datetime
from email.utils import parsedate_to_datetime
from typing import TypeVar

from motosan_ai.error import MotosanError, NetworkError, ProviderError, RateLimitError

logger = logging.getLogger(__name__)

T = TypeVar("T")

DEFAULT_MAX_RETRIES = 3
DEFAULT_INITIAL_BACKOFF = 0.1  # seconds (aligned with Rust: 100ms)
DEFAULT_MAX_BACKOFF = 2.0  # seconds (aligned with Rust: 2000ms)
RETRY_AFTER_CAP_SECS = 60.0


def parse_retry_after_header(value: str | None) -> float | None:
    """Parse a Retry-After header as capped seconds."""
    if value is None:
        return None
    stripped = value.strip()
    if not stripped:
        return None
    try:
        seconds = float(stripped)
    except ValueError:
        pass
    else:
        if seconds < 0:
            return None
        return min(seconds, RETRY_AFTER_CAP_SECS)

    try:
        retry_at = parsedate_to_datetime(stripped)
    except (TypeError, ValueError, IndexError, OverflowError):
        return None
    if retry_at.tzinfo is None:
        retry_at = retry_at.replace(tzinfo=UTC)
    seconds = (retry_at - datetime.now(UTC)).total_seconds()
    return min(max(seconds, 0.0), RETRY_AFTER_CAP_SECS)


@dataclass
class RetryEvent:
    """Passed to RetryPolicy.on_retry before each retry sleep."""

    attempt: int
    delay: float
    cause: str


@dataclass
class RetryPolicy:
    """Cross-SDK retry policy."""

    max_retries: int = DEFAULT_MAX_RETRIES
    base_delay: float = DEFAULT_INITIAL_BACKOFF
    max_delay: float = DEFAULT_MAX_BACKOFF
    jitter: bool = True
    respect_retry_after: bool = True
    on_retry: Callable[[RetryEvent], None] | None = None


def _is_retryable(error: Exception) -> bool:
    """Check whether an error is retryable using structured metadata."""
    if isinstance(error, RateLimitError):
        return True
    if isinstance(error, NetworkError):
        return True
    if isinstance(error, ProviderError):
        status_code = error.status_code or 0
        return error.status_code in {408, 409, 429} or status_code >= 500
    return False


def retry_cause(error: Exception) -> str:
    """Render a stable RetryEvent cause tag."""
    if isinstance(error, NetworkError):
        return f"network:{error}"
    status_code = getattr(error, "status_code", None)
    if status_code is not None:
        return f"status:{status_code}"
    return type(error).__name__


def compute_delay(
    policy: RetryPolicy,
    attempt: int,
    retry_after: float | None = None,
    rng: Callable[[], float] = random.random,
) -> float:
    """Compute seconds to sleep before retry number attempt, which is 1-based."""
    if policy.respect_retry_after and retry_after is not None:
        return min(retry_after, RETRY_AFTER_CAP_SECS)
    exp_delay = min(policy.base_delay * (2 ** (attempt - 1)), policy.max_delay)
    return rng() * exp_delay if policy.jitter else exp_delay


async def with_retry(
    fn: Callable[[], Awaitable[T]],
    max_retries: int = DEFAULT_MAX_RETRIES,
    initial_backoff: float = DEFAULT_INITIAL_BACKOFF,
    max_backoff: float = DEFAULT_MAX_BACKOFF,
    *,
    policy: RetryPolicy | None = None,
    rng: Callable[[], float] = random.random,
) -> T:
    """Execute fn with retry on transient errors.

    The legacy positional params remain in their original order. When policy is
    provided, policy values take precedence over the legacy params.
    """
    if policy is None:
        policy = RetryPolicy(
            max_retries=max_retries,
            base_delay=initial_backoff,
            max_delay=max_backoff,
        )

    last_error: Exception | None = None
    for attempt in range(policy.max_retries + 1):
        try:
            return await fn()
        except Exception as e:
            if not _is_retryable(e):
                raise
            last_error = e
            if attempt >= policy.max_retries:
                break
            retry_number = attempt + 1
            retry_after = e.retry_after if isinstance(e, MotosanError) else None
            wait = compute_delay(policy, retry_number, retry_after, rng)
            if policy.on_retry is not None:
                policy.on_retry(RetryEvent(attempt=retry_number, delay=wait, cause=retry_cause(e)))
            logger.warning(
                "Retryable error (attempt %d/%d), retrying in %.2fs: %s",
                retry_number,
                policy.max_retries,
                wait,
                type(e).__name__,
            )
            await asyncio.sleep(wait)
    raise last_error  # type: ignore[misc]
