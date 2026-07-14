import json

import httpx
import pytest
import respx

from motosan_ai.error import StreamError
from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message, StopReason


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


def _sse(*events: dict) -> str:
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


@respx.mock
@pytest.mark.asyncio
async def test_stream_emits_usage_from_message_start(provider):
    sse = _sse(
        {
            "type": "message_start",
            "message": {
                "usage": {
                    "input_tokens": 100,
                    "output_tokens": 0,
                    "cache_creation_input_tokens": 50,
                    "cache_read_input_tokens": 200,
                }
            },
        },
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "hi"}},
        {"type": "message_stop"},
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )

    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]

    usage_events = [e for e in events if e.event_type == "usage"]
    assert len(usage_events) >= 1
    first = usage_events[0]
    assert first.usage is not None
    assert first.usage.input_tokens == 100
    assert first.usage.cache_creation_input_tokens == 50
    assert first.usage.cache_read_input_tokens == 200


@respx.mock
@pytest.mark.asyncio
async def test_stream_emits_usage_from_message_delta(provider):
    sse = _sse(
        {
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {"input_tokens": 0, "output_tokens": 42},
        },
        {"type": "message_stop"},
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    usage_events = [e for e in events if e.event_type == "usage"]
    assert any(u.usage.output_tokens == 42 for u in usage_events if u.usage is not None)


@respx.mock
@pytest.mark.asyncio
async def test_stream_done_carries_stop_reason(provider):
    sse = _sse(
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "hi"}},
        {"type": "message_delta", "delta": {"stop_reason": "stop_sequence"}},
        {"type": "message_stop"},
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]

    done = next(e for e in events if e.done)
    assert done.stop_reason == StopReason.stop_sequence


@respx.mock
@pytest.mark.asyncio
async def test_stream_done_without_delta_has_no_stop_reason(provider):
    sse = _sse(
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "hi"}},
        {"type": "message_stop"},
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    done = next(e for e in events if e.done)
    assert done.stop_reason is None


@respx.mock
@pytest.mark.asyncio
async def test_stream_raises_on_malformed_chunk(provider):
    # one good text delta (via the file's _sse helper), then a non-empty malformed data: frame
    sse = (
        _sse(
            {
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "hi"},
            }
        )
        + "data: {not valid json\n"
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    seen = []
    with pytest.raises(StreamError, match="malformed SSE chunk"):
        async for ev in provider.stream(ChatRequest(messages=[Message.user("hi")])):
            seen.append(ev)
    assert any(e.content == "hi" for e in seen)  # the good delta was yielded first


@respx.mock
@pytest.mark.asyncio
async def test_stream_raises_on_error_event(provider):
    # message_start + one good text delta, then a mid-stream error frame on HTTP 200
    sse = _sse(
        {"type": "message_start", "message": {"usage": {"input_tokens": 3, "output_tokens": 0}}},
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "par"}},
        {"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}},
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    seen = []
    with pytest.raises(StreamError, match="anthropic stream error: overloaded_error: Overloaded"):
        async for ev in provider.stream(ChatRequest(messages=[Message.user("hi")])):
            seen.append(ev)
    assert any(e.content == "par" for e in seen)  # text before the error was still yielded


@respx.mock
@pytest.mark.asyncio
async def test_stream_error_event_with_missing_fields(provider):
    sse = _sse({"type": "error"})
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    with pytest.raises(StreamError, match="anthropic stream error: unknown_error"):
        async for _ in provider.stream(ChatRequest(messages=[Message.user("hi")])):
            pass
