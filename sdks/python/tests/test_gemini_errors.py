import httpx
import pytest
import respx

from motosan_ai.error import AuthError, ProviderError, RateLimitError
from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.retry import _parse_retry_after
from motosan_ai.types import ChatRequest, Message


@pytest.fixture
def provider():
    return GeminiProvider(api_key="test-key")


def _url():
    return (
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent"
    )


@respx.mock
@pytest.mark.asyncio
async def test_401_raises_auth_error(provider):
    respx.post(_url()).mock(
        return_value=httpx.Response(401, json={"error": {"message": "bad key"}})
    )
    with pytest.raises(AuthError):
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))


@respx.mock
@pytest.mark.asyncio
async def test_429_raises_rate_limit(provider):
    respx.post(_url()).mock(
        return_value=httpx.Response(429, json={"error": {"message": "slow down"}})
    )
    with pytest.raises(RateLimitError):
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))


@respx.mock
@pytest.mark.asyncio
async def test_500_raises_provider_error(provider):
    respx.post(_url()).mock(
        return_value=httpx.Response(500, json={"error": {"message": "backend blew up"}})
    )
    with pytest.raises(ProviderError):
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))


@respx.mock
@pytest.mark.asyncio
async def test_retry_after_header_is_preserved_in_error_message(provider):
    respx.post(_url()).mock(
        return_value=httpx.Response(
            429,
            headers={"retry-after": "1.5"},
            json={"error": {"message": "slow down"}},
        )
    )
    with pytest.raises(RateLimitError) as exc:
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    assert _parse_retry_after(str(exc.value)) == 1.5


@respx.mock
@pytest.mark.asyncio
async def test_error_message_redacts_api_key(provider):
    respx.post(_url()).mock(
        return_value=httpx.Response(
            500,
            json={"error": {"message": "proxy reflected test-key in body"}},
        )
    )
    with pytest.raises(ProviderError) as exc:
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    assert "test-key" not in str(exc.value)
    assert "[REDACTED]" in str(exc.value)
