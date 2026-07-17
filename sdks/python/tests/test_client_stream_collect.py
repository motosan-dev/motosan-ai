from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai import Client, Provider
from motosan_ai._stream_collect import collect_stream
from motosan_ai.error import StreamError
from motosan_ai.types import (
    ChatRequest,
    Message,
    StopReason,
    StreamEvent,
    StreamEventType,
    Usage,
)


async def _events_to_iter(events):
    for event in events:
        yield event


@pytest.mark.asyncio
async def test_collect_text_only():
    events = [
        StreamEvent(content="Hello ", done=False),
        StreamEvent(content="world", done=False),
        StreamEvent(content="", done=True, stop_reason=StopReason.end_turn),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.content == "Hello world"
    assert resp.tool_calls == []
    assert resp.stop_reason == StopReason.end_turn


@pytest.mark.asyncio
async def test_collect_with_usage_event():
    events = [
        StreamEvent(content="hi", done=False),
        StreamEvent(
            content="",
            done=False,
            event_type="usage",
            usage=Usage(input_tokens=10, output_tokens=5),
        ),
        StreamEvent(content="", done=True, stop_reason=StopReason.end_turn),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.usage.input_tokens == 10
    assert resp.usage.output_tokens == 5


@pytest.mark.asyncio
async def test_collect_merges_multiple_usage_events():
    events = [
        StreamEvent(
            content="",
            done=False,
            event_type="usage",
            usage=Usage(
                input_tokens=10,
                output_tokens=0,
                cache_creation_input_tokens=2,
                cache_read_input_tokens=3,
            ),
        ),
        StreamEvent(
            content="",
            done=False,
            event_type="usage",
            usage=Usage(input_tokens=0, output_tokens=5),
        ),
        StreamEvent(content="", done=True, stop_reason=StopReason.end_turn),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.usage.input_tokens == 10
    assert resp.usage.output_tokens == 5
    assert resp.usage.cache_creation_input_tokens == 2
    assert resp.usage.cache_read_input_tokens == 3


@pytest.mark.asyncio
async def test_collect_assembles_tool_call_from_start_args_end():
    events = [
        StreamEvent(
            content="",
            done=False,
            event_type="tool_call_start",
            tool_call_id="t1",
            tool_call_name="get_weather",
        ),
        StreamEvent(
            content="",
            done=False,
            event_type="tool_call_args",
            tool_call_id="t1",
            tool_call_args_delta='{"city":',
        ),
        StreamEvent(
            content="",
            done=False,
            event_type="tool_call_args",
            tool_call_id="t1",
            tool_call_args_delta='"Taipei"}',
        ),
        StreamEvent(content="", done=False, event_type="tool_call_end", tool_call_id="t1"),
        StreamEvent(content="", done=True, stop_reason=StopReason.tool_use),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert len(resp.tool_calls) == 1
    tc = resp.tool_calls[0]
    assert tc.id == "t1"
    assert tc.name == "get_weather"
    assert tc.input == {"city": "Taipei"}
    assert resp.stop_reason == StopReason.tool_use


@pytest.mark.asyncio
async def test_collect_handles_malformed_tool_args_as_empty_dict():
    events = [
        StreamEvent(
            content="",
            done=False,
            event_type="tool_call_start",
            tool_call_id="t1",
            tool_call_name="x",
        ),
        StreamEvent(
            content="",
            done=False,
            event_type="tool_call_args",
            tool_call_id="t1",
            tool_call_args_delta="not json",
        ),
        StreamEvent(content="", done=False, event_type="tool_call_end", tool_call_id="t1"),
        StreamEvent(content="", done=True),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.tool_calls[0].input == {}


@pytest.mark.asyncio
async def test_collect_default_stop_reason_when_done_lacks_one():
    events = [
        StreamEvent(content="hi", done=False),
        StreamEvent(content="", done=True),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.stop_reason == StopReason.end_turn


@pytest.mark.asyncio
async def test_collect_thinking_content_concatenated():
    events = [
        StreamEvent(
            content="reasoning step 1",
            done=False,
            event_type=StreamEventType.thinking_delta,
        ),
        StreamEvent(content=" step 2", done=False, event_type=StreamEventType.thinking_delta),
        StreamEvent(content="answer", done=False),
        StreamEvent(content="", done=True),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.thinking == "reasoning step 1 step 2"
    assert resp.content == "answer"


@pytest.mark.asyncio
async def test_collect_thinking_done_takes_priority():
    # Mirrors Rust stream.rs: ThinkingDone carries the authoritative full
    # text and wins over the delta accumulator at assembly.
    events = [
        StreamEvent(content="a", done=False, event_type=StreamEventType.thinking_delta),
        StreamEvent(content="b", done=False, event_type=StreamEventType.thinking_delta),
        StreamEvent(
            content="ab-final",
            done=False,
            event_type=StreamEventType.thinking_done,
        ),
        StreamEvent(content="answer", done=False),
        StreamEvent(content="", done=True, stop_reason=StopReason.end_turn),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.thinking == "ab-final"
    assert resp.content == "answer"


@pytest.mark.asyncio
async def test_collect_thinking_deltas_only_concatenated():
    # No thinking_done (e.g. chatgpt_codex): concatenated deltas are the
    # fallback.
    events = [
        StreamEvent(content="a", done=False, event_type=StreamEventType.thinking_delta),
        StreamEvent(content="b", done=False, event_type=StreamEventType.thinking_delta),
        StreamEvent(content="", done=True, stop_reason=StopReason.end_turn),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.thinking == "ab"


@pytest.mark.asyncio
async def test_collect_empty_thinking_done_yields_none():
    # Explicit empty thinking block -> treat as none (Rust parity,
    # stream.rs assembly match arm Some(_) => None).
    events = [
        StreamEvent(content="", done=False, event_type=StreamEventType.thinking_done),
        StreamEvent(content="", done=True, stop_reason=StopReason.end_turn),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.thinking is None


@pytest.mark.asyncio
async def test_collect_ignores_legacy_thinking_string():
    # BREAKING (M4/F3): the ad-hoc "thinking" event_type is gone from the
    # stream vocabulary; collect_stream must not accumulate it.
    events = [
        StreamEvent(content="legacy", done=False, event_type="thinking"),
        StreamEvent(content="", done=True, stop_reason=StopReason.end_turn),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.thinking is None


def _sse(*events):
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


@respx.mock
@pytest.mark.asyncio
async def test_client_stream_collect_returns_assembled_response(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    sse = _sse(
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "Hello "},
        },
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "world."},
        },
        {"type": "message_stop"},
    )
    respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    client = Client(provider=Provider.anthropic, model="claude-sonnet-4-6")
    resp = await client.stream_collect([Message.user("hi")])
    assert resp.content == "Hello world."
    assert resp.model == "claude-sonnet-4-6"


@respx.mock
@pytest.mark.asyncio
async def test_client_stream_collect_with_uses_request_model(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    sse = _sse({"type": "message_stop"})
    respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    client = Client(provider=Provider.anthropic, model="default-model")
    req = ChatRequest.builder().message(Message.user("hi")).model("override-model").build()
    resp = await client.stream_collect_with(req)
    assert resp.model == "override-model"


@respx.mock
@pytest.mark.asyncio
async def test_client_stream_collect_with_passes_thinking(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    sse = _sse({"type": "message_stop"})
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    client = Client(provider=Provider.anthropic)
    req = ChatRequest.builder().message(Message.user("hi")).thinking(2048).build()
    await client.stream_collect_with(req)
    body = json.loads(route.calls[0].request.content)
    assert body["thinking"] == {
        "type": "enabled",
        "budget_tokens": 2048,
        "display": "summarized",
    }


def test_collect_stream_exported_from_top_level():
    import motosan_ai

    assert callable(motosan_ai.collect_stream)


@pytest.mark.asyncio
async def test_collect_stream_propagates_mid_stream_raise():
    async def _raising_stream():
        yield StreamEvent(content="partial", done=False)
        raise StreamError("boom")

    with pytest.raises(StreamError, match="boom"):
        await collect_stream(_raising_stream())


@pytest.mark.asyncio
async def test_collect_stream_captures_session_id():
    async def gen():
        yield StreamEvent(content="", done=False, session_id="sid-1")
        yield StreamEvent(content="hi", done=False)
        yield StreamEvent(content="", done=True)

    resp = await collect_stream(gen())
    assert resp.session_id == "sid-1"
    assert resp.content == "hi"


@pytest.mark.asyncio
async def test_collect_stream_session_id_none_when_absent():
    async def gen():
        yield StreamEvent(content="hi", done=False)
        yield StreamEvent(content="", done=True)

    resp = await collect_stream(gen())
    assert resp.session_id is None


@pytest.mark.asyncio
async def test_collect_infers_tool_use_when_done_lacks_stop_reason():
    events = [
        StreamEvent(
            content="",
            done=False,
            event_type="tool_call_start",
            tool_call_id="t1",
            tool_call_name="get_weather",
        ),
        StreamEvent(
            content="",
            done=False,
            event_type="tool_call_args",
            tool_call_id="t1",
            tool_call_args_delta='{"city":"Taipei"}',
        ),
        StreamEvent(content="", done=False, event_type="tool_call_end", tool_call_id="t1"),
        StreamEvent(content="", done=True),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.stop_reason == StopReason.tool_use
