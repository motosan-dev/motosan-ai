from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai import Client, Provider
from motosan_ai.error import ProviderError
from motosan_ai.retry import RetryEvent, RetryPolicy
from motosan_ai.types import ChatRequest, Message, StreamEvent

_OK_JSON = {
    "model": "claude-sonnet-4-6",
    "stop_reason": "end_turn",
    "usage": {"input_tokens": 1, "output_tokens": 1},
    "content": [{"type": "text", "text": "ok"}],
}

_FAST = {"max_retries": 1, "base_delay": 0.001, "max_delay": 0.002}


def _sse_lines(*events: dict) -> str:
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


def test_legacy_max_retries_builds_default_policy(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")

    client = Client(provider=Provider.anthropic, max_retries=2)

    assert client._retry_policy.max_retries == 2


def test_explicit_retry_policy_wins_over_max_retries(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")

    client = Client(
        provider=Provider.anthropic, max_retries=5, retry_policy=RetryPolicy(max_retries=0)
    )

    assert client._retry_policy.max_retries == 0


def test_classmethod_constructor_accepts_retry_policy(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")

    client = Client.anthropic(retry_policy=RetryPolicy(max_retries=7))

    assert client._retry_policy.max_retries == 7


@respx.mock
@pytest.mark.asyncio
async def test_chat_retry_policy_retries_once_then_succeeds(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        side_effect=[
            httpx.Response(500, json={"error": {"message": "overloaded"}}),
            httpx.Response(200, json=_OK_JSON),
        ]
    )
    client = Client(provider=Provider.anthropic, retry_policy=RetryPolicy(**_FAST))

    resp = await client.chat([Message.user("hi")])

    assert resp.content == "ok"
    assert route.call_count == 2


@respx.mock
@pytest.mark.asyncio
async def test_on_retry_fires_through_client_chat_path(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    respx.post("https://api.anthropic.com/v1/messages").mock(
        side_effect=[
            httpx.Response(429, json={"error": {"message": "slow down"}}),
            httpx.Response(200, json=_OK_JSON),
        ]
    )
    events: list[RetryEvent] = []
    policy = RetryPolicy(on_retry=events.append, **_FAST)
    client = Client(provider=Provider.anthropic, retry_policy=policy)

    resp = await client.chat([Message.user("hi")])

    assert resp.content == "ok"
    assert len(events) == 1
    assert events[0].attempt == 1
    assert events[0].cause == "status:429"
    assert 0.0 <= events[0].delay <= 0.002


@respx.mock
@pytest.mark.asyncio
async def test_stream_retry_policy_retries_before_first_event(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    sse = _sse_lines(
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "hi"}},
        {"type": "message_stop"},
    )
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        side_effect=[
            httpx.Response(500, json={"error": {"message": "overloaded"}}),
            httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"}),
        ]
    )
    observed: list[RetryEvent] = []
    policy = RetryPolicy(on_retry=observed.append, **_FAST)
    client = Client(provider=Provider.anthropic, retry_policy=policy)

    out = [ev async for ev in client.stream([Message.user("hi")])]

    assert any(ev.content == "hi" for ev in out)
    assert route.call_count == 2
    assert len(observed) == 1
    assert observed[0].attempt == 1
    assert observed[0].cause == "status:500"


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
async def test_stream_policy_refuses_retry_after_first_event(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")

    async def gen():
        yield StreamEvent(content="partial", done=False)
        raise ProviderError("HTTP 503: overloaded", status_code=503)

    client = Client(
        provider=Provider.anthropic,
        retry_policy=RetryPolicy(max_retries=3, base_delay=0.001),
    )
    provider = _CountingProvider(gen)
    client._provider = provider
    req = ChatRequest(messages=[Message.user("hi")])

    with pytest.raises(ProviderError):
        async for _ in client.stream_with(req):
            pass
    assert provider.stream_calls == 1
