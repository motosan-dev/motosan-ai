import json

import httpx
import pytest
import respx

from motosan_ai.error import StreamError
from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.types import ChatRequest, Message, StopReason


@pytest.fixture
def provider():
    return GeminiProvider(api_key="test-key")


def _stream_url():
    return "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"


def _sse(*events: dict) -> str:
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


@respx.mock
@pytest.mark.asyncio
async def test_stream_text_deltas(provider):
    sse = _sse(
        {"candidates": [{"content": {"parts": [{"text": "Hel"}]}}]},
        {"candidates": [{"content": {"parts": [{"text": "lo"}]}}]},
        {
            "candidates": [{"content": {"parts": [{"text": "!"}]}, "finishReason": "STOP"}],
            "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 3},
        },
    )
    respx.post(_stream_url()).mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    assert [e.content for e in events if e.event_type == "text" and not e.done] == [
        "Hel",
        "lo",
        "!",
    ]
    assert next(e for e in events if e.done).stop_reason == StopReason.end_turn


@respx.mock
@pytest.mark.asyncio
async def test_stream_function_call_emits_start_args_end(provider):
    sse = _sse(
        {
            "candidates": [
                {
                    "content": {
                        "parts": [
                            {"functionCall": {"name": "get_weather", "args": {"city": "Taipei"}}}
                        ]
                    },
                    "finishReason": "STOP",
                }
            ]
        }
    )
    respx.post(_stream_url()).mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("w?")]))]
    starts = [e for e in events if e.event_type == "tool_call_start"]
    args = [e for e in events if e.event_type == "tool_call_args"]
    ends = [e for e in events if e.event_type == "tool_call_end"]
    assert len(starts) == 1
    assert starts[0].tool_call_name == "get_weather"
    assert json.loads(args[0].tool_call_args_delta) == {"city": "Taipei"}
    assert len(ends) == 1
    assert next(e for e in events if e.done).stop_reason == StopReason.tool_use


@respx.mock
@pytest.mark.asyncio
async def test_stream_emits_usage_events(provider):
    sse = _sse(
        {
            "candidates": [{"content": {"parts": [{"text": "hi"}]}, "finishReason": "STOP"}],
            "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 2},
        }
    )
    respx.post(_stream_url()).mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    usage_events = [e for e in events if e.event_type == "usage"]
    assert len(usage_events) == 1
    assert usage_events[0].usage.input_tokens == 5
    assert usage_events[0].usage.output_tokens == 2


@respx.mock
@pytest.mark.asyncio
async def test_stream_ignores_empty_data_and_done_marker(provider):
    sse = (
        "data: \n"
        + _sse({"candidates": [{"content": {"parts": [{"text": "ok"}]}, "finishReason": "STOP"}]})
        + "data: [DONE]\n"
    )
    respx.post(_stream_url()).mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    assert [e.content for e in events if e.event_type == "text" and not e.done] == ["ok"]


@respx.mock
@pytest.mark.asyncio
async def test_stream_raises_on_malformed_chunk(provider):
    sse = (
        _sse({"candidates": [{"content": {"parts": [{"text": "hi"}]}}]}) + "data: {not valid json\n"
    )
    respx.post(_stream_url()).mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    seen = []
    with pytest.raises(StreamError, match="malformed SSE chunk"):
        async for ev in provider.stream(ChatRequest(messages=[Message.user("hi")])):
            seen.append(ev)
    assert any(e.content == "hi" for e in seen)
