from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai.error import AuthError, NetworkError, ProviderError, RateLimitError
from motosan_ai.providers.chatgpt_codex import ChatGptCodexProvider
from motosan_ai.types import ChatRequest, Message, StopReason

_URL = "https://chatgpt.com/backend-api/codex/responses"


def _sse_text(*payloads: dict) -> str:
    return "\n".join(f"data: {json.dumps(p)}" for p in payloads) + "\n"


def _text_stream() -> str:
    return _sse_text(
        {"type": "response.output_text.delta", "delta": "Hello "},
        {"type": "response.output_text.delta", "delta": "world."},
        {
            "type": "response.completed",
            "response": {"status": "completed", "usage": {"input_tokens": 5, "output_tokens": 2}},
        },
    )


@respx.mock
@pytest.mark.asyncio
async def test_stream_yields_text_then_done():
    respx.post(_URL).mock(
        return_value=httpx.Response(
            200, text=_text_stream(), headers={"content-type": "text/event-stream"}
        )
    )
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    events = [e async for e in p.stream(ChatRequest(messages=[Message.user("hi")]))]
    text = "".join(e.content for e in events if e.event_type == "text" and not e.done)
    assert text == "Hello world."
    assert events[-1].done is True
    assert events[-1].stop_reason == StopReason.end_turn


@respx.mock
@pytest.mark.asyncio
async def test_stream_sends_codex_headers_and_responses_body():
    captured = {}

    def _capture(request):
        captured["headers"] = dict(request.headers)
        captured["body"] = json.loads(request.content)
        return httpx.Response(
            200, text=_text_stream(), headers={"content-type": "text/event-stream"}
        )

    respx.post(_URL).mock(side_effect=_capture)
    async for _ in ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None).stream(
        ChatRequest(messages=[Message.user("hi")])
    ):
        pass

    h = captured["headers"]
    assert h["authorization"] == "Bearer tok"
    assert h["chatgpt-account-id"] == "acct-123"
    assert h["originator"] == "codex_cli_rs"
    assert h["openai-beta"] == "responses=experimental"
    assert h["accept"] == "text/event-stream"

    body = captured["body"]
    assert body["store"] is False
    assert body["stream"] is True
    assert body["model"] == "gpt-5.5"
    assert body["include"] == ["reasoning.encrypted_content"]
    assert body["input"][0]["type"] == "message"
    assert body["input"][0]["content"][0]["type"] == "input_text"


@respx.mock
@pytest.mark.asyncio
async def test_chat_collects_stream_into_response():
    respx.post(_URL).mock(
        return_value=httpx.Response(
            200, text=_text_stream(), headers={"content-type": "text/event-stream"}
        )
    )
    resp = await ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None).chat(
        ChatRequest(messages=[Message.user("hi")])
    )
    assert resp.content == "Hello world."
    assert resp.usage.input_tokens == 5
    assert resp.usage.output_tokens == 2
    assert resp.model == "gpt-5.5"
    assert resp.stop_reason == StopReason.end_turn


@respx.mock
@pytest.mark.asyncio
async def test_chat_surfaces_thinking():
    sse = _sse_text(
        {"type": "response.reasoning_text.delta", "delta": "plan "},
        {"type": "response.reasoning_summary_text.delta", "delta": "ahead"},
        {"type": "response.output_text.delta", "delta": "done"},
        {"type": "response.completed", "response": {"status": "completed"}},
    )
    respx.post(_URL).mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    resp = await ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None).chat(
        ChatRequest(messages=[Message.user("hi")])
    )
    assert resp.content == "done"
    assert resp.thinking == "plan ahead"


@respx.mock
@pytest.mark.asyncio
async def test_chat_tool_call_lifecycle_yields_tool_call():
    sse = _sse_text(
        {
            "type": "response.output_item.added",
            "item": {"type": "function_call", "call_id": "c1", "name": "get_weather"},
        },
        {"type": "response.function_call_arguments.delta", "item_id": "c1", "delta": '{"city":'},
        {"type": "response.function_call_arguments.delta", "item_id": "c1", "delta": '"Paris"}'},
        {
            "type": "response.output_item.done",
            "item": {"type": "function_call", "call_id": "c1", "name": "get_weather"},
        },
        {"type": "response.completed", "response": {"status": "completed"}},
    )
    respx.post(_URL).mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    resp = await ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None).chat(
        ChatRequest(messages=[Message.user("weather?")])
    )
    assert resp.stop_reason == StopReason.tool_use
    assert len(resp.tool_calls) == 1
    assert resp.tool_calls[0].id == "c1"
    assert resp.tool_calls[0].name == "get_weather"
    assert resp.tool_calls[0].input == {"city": "Paris"}


@respx.mock
@pytest.mark.asyncio
async def test_stream_401_raises_auth_error():
    respx.post(_URL).mock(
        return_value=httpx.Response(401, json={"error": {"message": "expired token"}})
    )
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    with pytest.raises(AuthError):
        async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
            pass


@respx.mock
@pytest.mark.asyncio
async def test_stream_429_raises_rate_limit_error():
    respx.post(_URL).mock(
        return_value=httpx.Response(429, json={"error": {"message": "slow down"}})
    )
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    with pytest.raises(RateLimitError):
        async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
            pass


@respx.mock
@pytest.mark.asyncio
async def test_stream_500_raises_provider_error():
    respx.post(_URL).mock(return_value=httpx.Response(500, json={"error": {"message": "boom"}}))
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    with pytest.raises(ProviderError):
        async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
            pass


@respx.mock
@pytest.mark.asyncio
async def test_stream_transport_error_raises_network_error():
    respx.post(_URL).mock(side_effect=httpx.ConnectError("conn refused"))
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    with pytest.raises(NetworkError):
        async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
            pass


@respx.mock
@pytest.mark.asyncio
async def test_chat_502_then_200_is_retried():
    from motosan_ai.retry import with_retry

    route = respx.post(_URL).mock(
        side_effect=[
            httpx.Response(502, text="<html>bad gateway</html>"),
            httpx.Response(200, text=_text_stream(), headers={"content-type": "text/event-stream"}),
        ]
    )
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    resp = await with_retry(
        lambda: p.chat(ChatRequest(messages=[Message.user("hi")])),
        max_retries=2,
        initial_backoff=0.001,
    )
    assert resp.content == "Hello world."
    assert route.call_count == 2


@respx.mock
@pytest.mark.asyncio
async def test_stream_5xx_message_has_status_and_retry_after():
    respx.post(_URL).mock(
        return_value=httpx.Response(503, text="overloaded", headers={"retry-after": "7"})
    )
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    with pytest.raises(ProviderError) as exc_info:
        async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
            pass
    msg = str(exc_info.value)
    assert "HTTP 503: overloaded" in msg
    assert "Retry-After: 7" in msg
