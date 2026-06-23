from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai import Client, Provider
from motosan_ai.error import ProviderError, StreamError
from motosan_ai.types import ChatRequest, Message, StreamEvent


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
    assert body["thinking"] == {
        "type": "enabled",
        "budget_tokens": 1024,
        "display": "summarized",
    }


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


class _CountingProvider:
    """Provider stub that records how many times stream() was invoked."""

    def __init__(self, make_gen):
        self._make_gen = make_gen
        self.stream_calls = 0

    async def stream(self, request):
        self.stream_calls += 1
        async for event in self._make_gen():
            yield event


@pytest.mark.asyncio
async def test_stream_with_does_not_retry_stream_error(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")

    async def gen():
        # Long enough to clear ThinkStripper's tail-hold so it flushes mid-stream
        # (short deltas stay buffered until a done event, which never arrives here).
        yield StreamEvent(content="hello world", done=False)
        raise StreamError("boom")

    client = Client(provider=Provider.anthropic, max_retries=3)
    provider = _CountingProvider(gen)
    client._provider = provider  # documented injection seam
    req = ChatRequest(messages=[Message.user("hi")])
    seen = []
    with pytest.raises(StreamError, match="boom"):
        async for ev in client.stream_with(req):
            seen.append(ev)
    assert provider.stream_calls == 1  # never retried
    # the pre-raise delta was emitted once (ThinkStripper holds a 6-char tail)
    assert "".join(e.content for e in seen) == "hello"


@pytest.mark.asyncio
async def test_stream_with_does_not_retry_provider_error_after_yield(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")

    async def gen():
        yield StreamEvent(content="partial", done=False)
        raise ProviderError("503 server error")  # retryable BY CLASS, but mid-stream

    client = Client(provider=Provider.anthropic, max_retries=3)
    provider = _CountingProvider(gen)
    client._provider = provider  # documented injection seam
    req = ChatRequest(messages=[Message.user("hi")])
    with pytest.raises(ProviderError):
        async for _ in client.stream_with(req):
            pass
    assert provider.stream_calls == 1  # mid-stream retryable error is NOT replayed
