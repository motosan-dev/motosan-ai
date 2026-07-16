"""Retry logic with exponential backoff for transient errors.

Aligned with Rust SDK behavior:
- Retries on RateLimitError (429) and ProviderError (5xx)
- Retries on NetworkError (timeout, connection)
- Exponential backoff: 100ms, 200ms, 400ms... capped at 2s
- Respects Retry-After header
"""

from __future__ import annotations

import asyncio
import logging
import re
from collections.abc import Awaitable, Callable
from datetime import UTC, datetime
from email.utils import parsedate_to_datetime
from typing import TypeVar

from motosan_ai.error import NetworkError, ProviderError, RateLimitError

logger = logging.getLogger(__name__)

T = TypeVar("T")

DEFAULT_MAX_RETRIES = 3
DEFAULT_INITIAL_BACKOFF = 0.1  # seconds (aligned with Rust: 100ms)
DEFAULT_MAX_BACKOFF = 2.0  # seconds (aligned with Rust: 2000ms)
RETRY_AFTER_CAP_SECS = 60.0

# 5xx status codes in error messages
_STATUS_5XX_RE = re.compile(r"\b5\d{2}\b")


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


def _parse_retry_after(error_message: str) -> float | None:
    """Try to extract Retry-After seconds from error message."""
    match = re.search(r"[Rr]etry[- ][Aa]fter[:\s]+(\d+\.?\d*)", error_message)
    if match:
        return float(match.group(1))
    return None


def _is_retryable(error: Exception) -> bool:
    """Check if an error is retryable (aligned with Rust is_retryable_status + is_retryable_network_error)."""
    if isinstance(error, RateLimitError):
        return True
    if isinstance(error, NetworkError):
        return True
    if isinstance(error, ProviderError):
        # Retry on 5xx errors (server errors)
        msg = str(error)
        if _STATUS_5XX_RE.search(msg):
            return True
    return False


async def with_retry(
    fn: Callable[[], Awaitable[T]],
    max_retries: int = DEFAULT_MAX_RETRIES,
    initial_backoff: float = DEFAULT_INITIAL_BACKOFF,
    max_backoff: float = DEFAULT_MAX_BACKOFF,
) -> T:
    """Execute fn with retry on transient errors.

    Retries on: RateLimitError (429), NetworkError, ProviderError (5xx).
    Backoff: initial_backoff * 2^attempt capped at max_backoff.
    Uses Retry-After header value if present in the error message.
    """
    last_error: Exception | None = None
    for attempt in range(max_retries + 1):
        try:
            return await fn()
        except Exception as e:
            if not _is_retryable(e):
                raise
            last_error = e
            if attempt >= max_retries:
                break
            retry_after = _parse_retry_after(str(e))
            if retry_after is not None:
                wait = min(retry_after, max_backoff)
            else:
                wait = min(initial_backoff * (2**attempt), max_backoff)
            logger.warning(
                "Retryable error (attempt %d/%d), retrying in %.1fs: %s",
                attempt + 1,
                max_retries,
                wait,
                type(e).__name__,
            )
            await asyncio.sleep(wait)
    raise last_error  # type: ignore[misc]
