import httpx
import pytest
import respx

from motosan_ai.error import InvalidRequestError, ProviderError, RateLimitError
from motosan_ai.provider_base import ProviderCapabilities
from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


@pytest.mark.asyncio
async def test_chat_rejects_unsupported_image_before_http(provider):
    provider.capabilities = ProviderCapabilities.text_only()
    req = ChatRequest(messages=[Message.user_with_image("x", "abc", "image/png")])
    with pytest.raises(InvalidRequestError, match="image"):
        await provider.chat(req)


@pytest.mark.asyncio
async def test_stream_rejects_unsupported_document_before_http(provider):
    provider.capabilities = ProviderCapabilities.with_image()
    req = ChatRequest(messages=[Message.user_with_pdf_base64("x", "abc")])
    with pytest.raises(InvalidRequestError, match="document"):
        async for _ in provider.stream(req):
            pass


@pytest.mark.asyncio
async def test_full_capabilities_accept_image_and_document(provider):
    with respx.mock:
        respx.post("https://mock.anthropic.com/v1/messages").mock(
            return_value=httpx.Response(
                200,
                json={
                    "model": "claude-sonnet-4-6",
                    "stop_reason": "end_turn",
                    "usage": {"input_tokens": 1, "output_tokens": 1},
                    "content": [{"type": "text", "text": "ok"}],
                },
            )
        )
        req = ChatRequest(messages=[Message.user_with_image("x", "abc", "image/png")])
        resp = await provider.chat(req)
        assert resp.content == "ok"


@respx.mock
@pytest.mark.asyncio
async def test_retry_after_header_is_preserved_in_error_message(provider):
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            429,
            headers={"retry-after": "2"},
            json={"error": {"message": "slow down"}},
        )
    )
    with pytest.raises(RateLimitError) as exc:
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    assert exc.value.retry_after == 2.0


@respx.mock
@pytest.mark.asyncio
async def test_error_message_redacts_api_key(provider):
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            500,
            json={"error": {"message": "proxy reflected test-key in body"}},
        )
    )
    with pytest.raises(ProviderError) as exc:
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    assert "test-key" not in str(exc.value)
    assert "[REDACTED]" in str(exc.value)
