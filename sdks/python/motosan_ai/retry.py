"""Retry logic with exponential backoff for rate limit errors."""
from __future__ import annotations

import asyncio
import logging
import re
from typing import TypeVar, Callable, Awaitable

from motosan_ai.error import RateLimitError

logger = logging.getLogger(__name__)

T = TypeVar("T")

DEFAULT_MAX_RETRIES = 3
DEFAULT_INITIAL_BACKOFF = 1.0  # seconds
DEFAULT_MAX_BACKOFF = 30.0


def _parse_retry_after(error_message: str) -> float | None:
    """Try to extract Retry-After seconds from error message."""
    match = re.search(r"[Rr]etry[- ][Aa]fter[:\s]+(\d+\.?\d*)", error_message)
    if match:
        return float(match.group(1))
    return None


async def with_retry(
    fn: Callable[[], Awaitable[T]],
    max_retries: int = DEFAULT_MAX_RETRIES,
    initial_backoff: float = DEFAULT_INITIAL_BACKOFF,
    max_backoff: float = DEFAULT_MAX_BACKOFF,
) -> T:
    """Execute fn with retry on RateLimitError.

    Backoff: initial_backoff * 2^attempt (1s, 2s, 4s, ...) capped at max_backoff.
    Uses Retry-After header value if present in the error message.
    """
    last_error: RateLimitError | None = None
    for attempt in range(max_retries + 1):
        try:
            return await fn()
        except RateLimitError as e:
            last_error = e
            if attempt >= max_retries:
                break
            # Check for Retry-After hint
            retry_after = _parse_retry_after(str(e))
            if retry_after is not None:
                wait = min(retry_after, max_backoff)
            else:
                wait = min(initial_backoff * (2 ** attempt), max_backoff)
            logger.warning(
                "Rate limited (attempt %d/%d), retrying in %.1fs",
                attempt + 1, max_retries, wait,
            )
            await asyncio.sleep(wait)
    raise last_error  # type: ignore[misc]
