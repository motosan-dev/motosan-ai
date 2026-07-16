from datetime import datetime, timedelta, timezone
from email.utils import format_datetime

import httpx
import pytest
import respx

from motosan_ai.error import (
    AuthError,
    InvalidRequestError,
    MotosanError,
    NetworkError,
    ProviderError,
    RateLimitError,
    StreamError,
)
from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.providers.minimax import MinimaxProvider
from motosan_ai.types import ChatRequest, Message


def test_minimax_error_mapping():
    with pytest.raises(AuthError):
        MinimaxProvider._raise_for_status(401, "unauthorized")
    with pytest.raises(RateLimitError):
        MinimaxProvider._raise_for_status(429, "rate")
    with pytest.raises(InvalidRequestError):
        MinimaxProvider._raise_for_status(400, "bad")
    with pytest.raises(ProviderError):
        MinimaxProvider._raise_for_status(500, "oops")


def test_error_attributes_are_stored_without_changing_str():
    err = MotosanError(
        "HTTP 429: slow down",
        status_code=429,
        retry_after=1.5,
        request_id="req_abc",
    )

    assert str(err) == "HTTP 429: slow down"
    assert err.status_code == 429
    assert err.retry_after == 1.5
    assert err.request_id == "req_abc"


def test_error_metadata_defaults_to_none():
    err = MotosanError("boom")

    assert err.status_code is None
    assert err.retry_after is None
    assert err.request_id is None


@pytest.mark.parametrize(
    "error_cls",
    [AuthError, RateLimitError, InvalidRequestError, ProviderError, NetworkError, StreamError],
)
def test_error_subclasses_accept_metadata_kwargs(error_cls):
    err = error_cls("boom", status_code=503, retry_after=2.0, request_id="req_123")

    assert str(err) == "boom"
    assert err.status_code == 503
    assert err.retry_after == 2.0
    assert err.request_id == "req_123"


@respx.mock
@pytest.mark.asyncio
async def test_anthropic_429_populates_http_metadata():
    provider = AnthropicProvider("test-key", base_url="https://mock.anthropic.com")
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            429,
            headers={"retry-after": "120", "request-id": "req_abc"},
            json={"error": {"message": "slow down"}},
        )
    )

    with pytest.raises(RateLimitError) as exc:
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))

    assert str(exc.value) == "Retry-After: 120\nHTTP 429: slow down"
    assert exc.value.status_code == 429
    assert exc.value.retry_after == 60.0
    assert exc.value.request_id == "req_abc"


@respx.mock
@pytest.mark.asyncio
async def test_anthropic_retry_after_http_date_populates_seconds():
    provider = AnthropicProvider("test-key", base_url="https://mock.anthropic.com")
    retry_at = datetime.now(timezone.utc) + timedelta(seconds=25)
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            429,
            headers={"retry-after": format_datetime(retry_at, usegmt=True)},
            json={"error": {"message": "slow down"}},
        )
    )

    with pytest.raises(RateLimitError) as exc:
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))

    assert exc.value.status_code == 429
    assert exc.value.retry_after is not None
    assert 20.0 <= exc.value.retry_after <= 30.0


@respx.mock
@pytest.mark.asyncio
async def test_anthropic_500_populates_request_id_fallback():
    provider = AnthropicProvider("test-key", base_url="https://mock.anthropic.com")
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            500,
            headers={"x-request-id": "req_xyz"},
            json={"error": {"message": "backend blew up"}},
        )
    )

    with pytest.raises(ProviderError) as exc:
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))

    assert str(exc.value) == "HTTP 500: backend blew up"
    assert exc.value.status_code == 500
    assert exc.value.retry_after is None
    assert exc.value.request_id == "req_xyz"
