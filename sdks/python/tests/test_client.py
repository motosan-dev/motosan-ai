import os

import pytest

from motosan_ai import ChatResponse, Client, Message, Provider, StopReason, Usage


class FakeProvider:
    def __init__(self):
        self.last_request = None

    async def chat(self, request):
        self.last_request = request
        return ChatResponse(content="ok", usage=Usage(1, 1), stop_reason=StopReason.stop)

    async def stream(self, request):
        self.last_request = request
        if False:
            yield None


@pytest.mark.asyncio
async def test_client_accepts_dict_messages(monkeypatch):
    monkeypatch.setenv("OPENAI_API_KEY", "env-openai")
    client = Client(Provider.openai)
    fake = FakeProvider()
    client._provider = fake

    response = await client.chat([{"role": "user", "content": "hello"}])
    assert response.content == "ok"
    assert fake.last_request.messages[0] == Message.user("hello")


def test_client_env_fallback(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "env-anthropic")
    client = Client(Provider.anthropic)
    assert client.api_key == "env-anthropic"


def test_chat_sync(monkeypatch):
    monkeypatch.setenv("MINIMAX_API_KEY", "env-mini")
    client = Client(Provider.minimax)
    fake = FakeProvider()
    client._provider = fake
    response = client.chat_sync([Message.user("hi")])
    assert response.content == "ok"


def test_missing_key_raises(monkeypatch):
    monkeypatch.delenv("OPENAI_API_KEY", raising=False)
    with pytest.raises(Exception):
        Client(Provider.openai)
