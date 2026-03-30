import json

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message, StopReason, Tool


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


@pytest.fixture
def oauth_provider():
    return AnthropicProvider("sk-ant-oat01-fake", base_url="https://mock.anthropic.com")


# ---------------------------------------------------------------------------
# chat (non-streaming, standard API key)
# ---------------------------------------------------------------------------


@respx.mock
@pytest.mark.asyncio
async def test_anthropic_chat(provider):
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200,
            json={
                "model": "claude-sonnet-4-6",
                "stop_reason": "tool_use",
                "usage": {"input_tokens": 10, "output_tokens": 5},
                "content": [
                    {"type": "text", "text": "checking"},
                    {
                        "type": "tool_use",
                        "id": "toolu_1",
                        "name": "get_weather",
                        "input": {"city": "Taipei"},
                    },
                ],
            },
        )
    )

    req = ChatRequest(messages=[Message.user("weather?")])
    resp = await provider.chat(req)

    assert resp.stop_reason == StopReason.tool_use
    assert resp.content == "checking"
    assert resp.tool_calls[0].id == "toolu_1"
    assert resp.tool_calls[0].name == "get_weather"
    assert resp.tool_calls[0].input == {"city": "Taipei"}
    assert resp.usage.input_tokens == 10
    assert resp.usage.output_tokens == 5


# ---------------------------------------------------------------------------
# stream (SSE parsing)
# ---------------------------------------------------------------------------


def _sse_lines(*events: dict) -> str:
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


@respx.mock
@pytest.mark.asyncio
async def test_anthropic_stream(provider):
    sse = _sse_lines(
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "hel"}},
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "lo"}},
        {"type": "message_stop"},
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )

    req = ChatRequest(messages=[Message.user("hi")])
    events = [e async for e in provider.stream(req)]

    assert [e.content for e in events if not e.done] == ["hel", "lo"]
    assert events[-1].done is True


@respx.mock
@pytest.mark.asyncio
async def test_anthropic_stream_tool_use(provider):
    sse = _sse_lines(
        {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}},
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "Let me check"},
        },
        {"type": "content_block_stop", "index": 0},
        {
            "type": "content_block_start",
            "index": 1,
            "content_block": {"type": "tool_use", "id": "toolu_1", "name": "get_weather"},
        },
        {
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "input_json_delta", "partial_json": '{"city":'},
        },
        {
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "input_json_delta", "partial_json": '"Taipei"}'},
        },
        {"type": "content_block_stop", "index": 1},
        {"type": "message_stop"},
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )

    tools = [
        Tool(
            name="get_weather",
            description="Get weather",
            input_schema={"type": "object", "properties": {"city": {"type": "string"}}},
        )
    ]
    req = ChatRequest(messages=[Message.user("weather in Taipei?")], tools=tools)
    events = [e async for e in provider.stream(req)]

    text_events = [e for e in events if e.event_type == "text" and not e.done]
    assert any(e.content == "Let me check" for e in text_events)

    starts = [e for e in events if e.event_type == "tool_call_start"]
    assert len(starts) == 1
    assert starts[0].tool_call_id == "toolu_1"
    assert starts[0].tool_call_name == "get_weather"

    args_events = [e for e in events if e.event_type == "tool_call_args"]
    assert len(args_events) == 2
    full_args = "".join(e.tool_call_args_delta for e in args_events)
    assert full_args == '{"city":"Taipei"}'
    assert args_events[0].tool_call_id == "toolu_1"

    ends = [e for e in events if e.event_type == "tool_call_end"]
    assert len(ends) == 1
    assert ends[0].tool_call_id == "toolu_1"

    assert events[-1].done is True


# ---------------------------------------------------------------------------
# Auth headers
# ---------------------------------------------------------------------------


@respx.mock
@pytest.mark.asyncio
async def test_anthropic_standard_key_uses_x_api_key(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200,
            json={
                "model": "claude-sonnet-4-6",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "content": [{"type": "text", "text": "hi"}],
            },
        )
    )

    await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    assert route.calls[0].request.headers["x-api-key"] == "test-key"
    assert "authorization" not in route.calls[0].request.headers


@respx.mock
@pytest.mark.asyncio
async def test_anthropic_oauth_uses_bearer(oauth_provider):
    sse = _sse_lines(
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "hi"}},
        {"type": "message_stop"},
    )
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )

    await oauth_provider.chat(ChatRequest(messages=[Message.user("hi")]))
    assert route.calls[0].request.headers["authorization"] == "Bearer sk-ant-oat01-fake"
    assert "oauth-2025-04-20" in route.calls[0].request.headers["anthropic-beta"]
    assert route.calls[0].request.headers["x-app"] == "cli"


# ---------------------------------------------------------------------------
# Error mapping
# ---------------------------------------------------------------------------


@respx.mock
@pytest.mark.asyncio
async def test_anthropic_401_raises_auth_error(provider):
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(401, json={"error": {"message": "invalid key"}})
    )
    from motosan_ai.error import AuthError

    with pytest.raises(AuthError):
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))


@respx.mock
@pytest.mark.asyncio
async def test_anthropic_429_raises_rate_limit(provider):
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(429, json={"error": {"message": "too many requests"}})
    )
    from motosan_ai.error import RateLimitError

    with pytest.raises(RateLimitError):
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))
