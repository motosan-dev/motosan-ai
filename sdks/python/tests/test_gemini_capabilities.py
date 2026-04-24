import pytest

from motosan_ai.error import InvalidRequestError
from motosan_ai.provider_base import ProviderCapabilities
from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.types import ChatRequest, Message


@pytest.fixture
def provider():
    return GeminiProvider(api_key="test-key")


def test_default_model_is_gemini_2_flash(provider):
    assert provider.model == "gemini-2.5-flash"


def test_capabilities_is_with_image(provider):
    assert provider.capabilities == ProviderCapabilities.with_image()


def test_generate_url_includes_model(provider):
    req = ChatRequest(messages=[Message.user("hi")])
    assert provider._generate_url(req).endswith("/v1beta/models/gemini-2.5-flash:generateContent")


def test_stream_url_has_alt_sse(provider):
    req = ChatRequest(messages=[Message.user("hi")])
    assert provider._stream_url(req).endswith(
        "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
    )


def test_per_request_model_overrides_default(provider):
    req = ChatRequest(messages=[Message.user("hi")], model="gemini-2.5-pro")
    assert "/gemini-2.5-pro:" in provider._generate_url(req)


def test_auth_header_uses_x_goog_api_key(provider):
    headers = provider._headers()
    assert headers["x-goog-api-key"] == "test-key"
    assert "authorization" not in headers


@pytest.mark.asyncio
async def test_validate_rejects_pdf_document(provider):
    req = ChatRequest(messages=[Message.user_with_pdf_base64("read", "abc")])
    with pytest.raises(InvalidRequestError, match="document"):
        provider.validate_request(req)


@pytest.mark.asyncio
async def test_validate_accepts_image(provider):
    req = ChatRequest(messages=[Message.user_with_image("see", "abc", "image/png")])
    provider.validate_request(req)
