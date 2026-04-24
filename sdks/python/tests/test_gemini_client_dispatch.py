import httpx
import pytest
import respx

from motosan_ai import Client, Provider
from motosan_ai.error import ConfigError
from motosan_ai.providers.gemini import GeminiProvider


def test_provider_enum_has_gemini():
    assert Provider.gemini == "gemini"


def test_client_gemini_classmethod():
    client = Client.gemini(api_key="key", model="gemini-2.5-flash")
    assert client.provider == Provider.gemini
    assert isinstance(client._provider, GeminiProvider)


def test_client_loads_gemini_api_key_from_env(monkeypatch):
    monkeypatch.setenv("GEMINI_API_KEY", "env-key")
    client = Client(provider=Provider.gemini)
    assert client.api_key == "env-key"


def test_client_raises_config_error_when_no_key(monkeypatch):
    monkeypatch.delenv("GEMINI_API_KEY", raising=False)
    with pytest.raises(ConfigError):
        Client(provider=Provider.gemini)


@respx.mock
@pytest.mark.asyncio
async def test_client_chat_dispatches_to_gemini(monkeypatch):
    monkeypatch.setenv("GEMINI_API_KEY", "k")
    url = "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent"
    respx.post(url).mock(
        return_value=httpx.Response(
            200,
            json={
                "candidates": [{"content": {"parts": [{"text": "ok"}]}, "finishReason": "STOP"}],
                "usageMetadata": {},
            },
        )
    )
    client = Client(provider=Provider.gemini)
    resp = await client.chat([{"role": "user", "content": "hi"}])
    assert resp.content == "ok"
