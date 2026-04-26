from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai import Client, Provider
from motosan_ai.types import ChatRequest, Message


def _sse_lines(*events: dict) -> str:
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


@respx.mock
@pytest.mark.asyncio
async def test_stream_with_passes_thinking_to_provider(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    sse = _sse_lines(
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "hi"}},
        {"type": "message_stop"},
    )
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    client = Client(provider=Provider.anthropic)
    req = ChatRequest.builder().message(Message.user("hi")).thinking(1024).build()
    events = [event async for event in client.stream_with(req)]
    assert any(event.content == "hi" for event in events)
    body = json.loads(route.calls[0].request.content)
    assert body["thinking"] == {"type": "enabled", "budget_tokens": 1024}


@respx.mock
@pytest.mark.asyncio
async def test_stream_with_falls_back_to_client_model(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    sse = _sse_lines({"type": "message_stop"})
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    client = Client(provider=Provider.anthropic, model="claude-haiku-4-5-20251001")
    req = ChatRequest.builder().message(Message.user("hi")).build()
    [event async for event in client.stream_with(req)]
    body = json.loads(route.calls[0].request.content)
    assert body["model"] == "claude-haiku-4-5-20251001"


@respx.mock
@pytest.mark.asyncio
async def test_stream_kwargs_path_unchanged_regression(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    sse = _sse_lines(
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "hi"}},
        {"type": "message_stop"},
    )
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    client = Client(provider=Provider.anthropic)
    events = [event async for event in client.stream([Message.user("hi")], system="terse")]
    assert any(event.content == "hi" for event in events)
    body = json.loads(route.calls[0].request.content)
    assert body["system"] == "terse"
