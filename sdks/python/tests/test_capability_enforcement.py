from __future__ import annotations

import pytest

from motosan_ai import Client, Message
from motosan_ai.error import InvalidRequestError
from motosan_ai.types import ChatResponse, StopReason, StreamEvent, Usage

IMG = Message.user_with_image("caption", "abc", "image/png")
PDF = Message.user_with_pdf_base64("caption", "abc")


def _block_dispatch(monkeypatch, provider):
    """Replace the provider's network/CLI entry points with recording fakes.

    Enforcement must fire BEFORE dispatch, so these must never run. If
    enforcement is missing, the fakes answer instantly and the test fails
    deterministically — no network, no CLI process, no hangs.
    """
    calls: list[str] = []

    async def fake_chat(request):
        calls.append("chat")
        return ChatResponse(content="dispatched", usage=Usage(1, 1), stop_reason=StopReason.stop)

    async def fake_stream(request):
        calls.append("stream")
        yield StreamEvent(content="dispatched", done=True)

    monkeypatch.setattr(provider, "chat", fake_chat)
    monkeypatch.setattr(provider, "stream", fake_stream)
    return calls


@pytest.mark.asyncio
async def test_minimax_image_rejected_on_chat(monkeypatch):
    client = Client(provider="minimax", api_key="k")
    calls = _block_dispatch(monkeypatch, client._provider)
    with pytest.raises(InvalidRequestError, match="image"):
        await client.chat([IMG])
    assert calls == []


@pytest.mark.asyncio
async def test_minimax_image_rejected_on_stream(monkeypatch):
    client = Client(provider="minimax", api_key="k")
    calls = _block_dispatch(monkeypatch, client._provider)
    with pytest.raises(InvalidRequestError, match="image"):
        async for _ in client.stream([IMG]):
            pass
    assert calls == []


@pytest.mark.asyncio
async def test_openai_document_rejected_on_chat(monkeypatch):
    client = Client(provider="openai", api_key="k")
    calls = _block_dispatch(monkeypatch, client._provider)
    with pytest.raises(InvalidRequestError, match="document"):
        await client.chat([PDF])
    assert calls == []


@pytest.mark.asyncio
async def test_ollama_native_image_rejected_on_chat(monkeypatch):
    client = Client(provider="ollama", ollama_native=True, model="llama3.2")
    calls = _block_dispatch(monkeypatch, client._provider)
    with pytest.raises(InvalidRequestError, match="image"):
        await client.chat([IMG])
    assert calls == []


@pytest.mark.asyncio
async def test_claude_code_image_rejected_on_chat(monkeypatch):
    client = Client(provider="claude_code")
    calls = _block_dispatch(monkeypatch, client._provider)
    with pytest.raises(InvalidRequestError, match="image"):
        await client.chat([IMG])
    assert calls == []


class _NoCapsProvider:
    """LlmClient-Protocol-shaped provider WITHOUT a capabilities attribute."""

    async def chat(self, request):
        return ChatResponse(content="ok", usage=Usage(1, 1), stop_reason=StopReason.stop)

    async def stream(self, request):
        if False:
            yield None


@pytest.mark.asyncio
async def test_provider_without_capabilities_is_not_validated():
    # The LlmClient Protocol does not require `capabilities`; central
    # validation must be skipped, not crash, for such providers.
    client = Client(provider="anthropic", api_key="k")
    client._provider = _NoCapsProvider()
    response = await client.chat([IMG])
    assert response.content == "ok"
