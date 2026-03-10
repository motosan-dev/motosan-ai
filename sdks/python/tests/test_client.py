"""Unit tests for Client — no network calls."""
import pytest
from unittest.mock import AsyncMock, MagicMock
from motosan_ai import Client, ChatResponse, Usage, StopReason


def _mock_response() -> ChatResponse:
    return ChatResponse(
        content="Paris",
        model="claude-sonnet-4-5",
        usage=Usage(input_tokens=10, output_tokens=5),
        stop_reason=StopReason.END_TURN,
    )


@pytest.fixture
def client_with_mock(monkeypatch):
    """Client with mocked AnthropicProvider."""
    mock_provider = MagicMock()
    mock_provider.chat = AsyncMock(return_value=_mock_response())

    def fake_create_provider(provider, api_key):
        return mock_provider

    import motosan_ai.client as client_module
    monkeypatch.setattr(client_module, "_create_provider", fake_create_provider)
    return Client(provider="anthropic", api_key="test-key"), mock_provider


@pytest.mark.asyncio
async def test_chat_returns_response(client_with_mock):
    client, mock_provider = client_with_mock
    response = await client.chat([{"role": "user", "content": "Hello"}])
    assert response.content == "Paris"
    assert response.model == "claude-sonnet-4-5"
    mock_provider.chat.assert_called_once()


@pytest.mark.asyncio
async def test_chat_accepts_dict_messages(client_with_mock):
    client, _ = client_with_mock
    response = await client.chat([{"role": "user", "content": "Hello"}])
    assert response.content == "Paris"


def test_client_requires_api_key():
    import os
    env_backup = os.environ.pop("ANTHROPIC_API_KEY", None)
    try:
        with pytest.raises(ValueError, match="api_key is required"):
            Client(provider="anthropic", api_key="")
    finally:
        if env_backup:
            os.environ["ANTHROPIC_API_KEY"] = env_backup


def test_client_unknown_provider():
    with pytest.raises(ValueError, match="Unknown provider"):
        Client(provider="unknown", api_key="test")  # type: ignore
