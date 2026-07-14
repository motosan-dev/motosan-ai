from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai.error import StreamError
from motosan_ai.providers.chatgpt_codex import (
    ChatGptCodexProvider,
    _ChatGptCodexAdapterState,
    _parse_sse_event,
)
from motosan_ai.types import ChatRequest, Message, StopReason

_URL = "https://chatgpt.com/backend-api/codex/responses"

# Real text-delta frames mirroring the Rust TEXT_FRAMES fixture, plus a complete
# response.completed frame (usage + status).
TEXT_FRAMES = [
    {"type": "response.created", "response": {"id": "resp_1", "status": "in_progress"}},
    {"type": "response.output_text.delta", "delta": "Hi", "item_id": "msg_1"},
    {"type": "response.output_text.delta", "delta": " there", "item_id": "msg_1"},
    {"type": "response.output_text.delta", "delta": ",", "item_id": "msg_1"},
    {"type": "response.output_text.delta", "delta": " friend", "item_id": "msg_1"},
    {"type": "response.output_text.done", "item_id": "msg_1", "text": "Hi there, friend"},
    {
        "type": "response.completed",
        "response": {
            "id": "resp_1",
            "status": "completed",
            "usage": {"input_tokens": 12, "output_tokens": 5},
        },
    },
]


def _sse_text(*payloads: dict) -> str:
    return "\n".join(f"data: {json.dumps(p)}" for p in payloads) + "\n"


def _drive(frames: list[dict]) -> list:
    state = _ChatGptCodexAdapterState()
    out = []
    for frame in frames:
        out.extend(_parse_sse_event(json.dumps(frame), state))
    return out


def test_empty_and_done_sentinel_return_no_events():
    state = _ChatGptCodexAdapterState()
    assert _parse_sse_event("", state) == []
    assert _parse_sse_event("[DONE]", state) == []


def test_malformed_json_skipped():
    assert _parse_sse_event("not json {", _ChatGptCodexAdapterState()) == []


def test_unknown_event_type_ignored():
    assert (
        _parse_sse_event(json.dumps({"type": "response.in_progress"}), _ChatGptCodexAdapterState())
        == []
    )


def test_empty_text_delta_emits_nothing():
    state = _ChatGptCodexAdapterState()
    assert (
        _parse_sse_event(json.dumps({"type": "response.output_text.delta", "delta": ""}), state)
        == []
    )


def test_adapter_emits_text_and_done():
    events = _drive(TEXT_FRAMES)
    text = "".join(e.content for e in events if e.event_type == "text")
    assert text == "Hi there, friend"
    done = next(e for e in events if e.done)
    assert done.stop_reason == StopReason.end_turn


def test_adapter_emits_usage_from_response_completed():
    events = _drive(TEXT_FRAMES)
    usage = next(e.usage for e in events if e.event_type == "usage")
    assert usage is not None
    assert usage.input_tokens == 12
    assert usage.output_tokens == 5
    assert usage.cache_read_input_tokens is None


def test_adapter_surfaces_cached_tokens_as_is():
    state = _ChatGptCodexAdapterState()
    frame = {
        "type": "response.completed",
        "response": {
            "status": "completed",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 5,
                "input_tokens_details": {"cached_tokens": 30},
            },
        },
    }
    events = _parse_sse_event(json.dumps(frame), state)
    usage = next(e.usage for e in events if e.event_type == "usage")
    assert usage is not None
    # Surfaced as-is, NOT subtracted (input_tokens already counts the full prompt).
    assert usage.input_tokens == 100
    assert usage.cache_read_input_tokens == 30


def test_adapter_maps_reasoning_delta_to_thinking():
    events = _drive(
        [
            {"type": "response.reasoning_text.delta", "delta": "think "},
            {"type": "response.reasoning_summary_text.delta", "delta": "more"},
        ]
    )
    thinking = "".join(e.content for e in events if e.event_type == "thinking")
    assert thinking == "think more"


def test_adapter_handles_function_call_lifecycle():
    # Real wire: the item carries both an item id ("fc_...") and a call_id
    # ("call_..."); argument fragments are keyed by the ITEM id. All emitted
    # events must use the call_id.
    events = _drive(
        [
            {
                "type": "response.output_item.added",
                "item": {
                    "type": "function_call",
                    "id": "fc_42",
                    "call_id": "call_42",
                    "name": "get_weather",
                },
            },
            {
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_42",
                "delta": '{"city":',
            },
            {
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_42",
                "delta": '"Paris"}',
            },
            {
                "type": "response.output_item.done",
                "item": {
                    "type": "function_call",
                    "id": "fc_42",
                    "call_id": "call_42",
                    "name": "get_weather",
                },
            },
            {
                "type": "response.completed",
                "response": {
                    "status": "completed",
                    "usage": {"input_tokens": 3, "output_tokens": 7},
                },
            },
        ]
    )

    start = next(e for e in events if e.event_type == "tool_call_start")
    assert start.tool_call_id == "call_42"
    assert start.tool_call_name == "get_weather"

    arg_events = [e for e in events if e.event_type == "tool_call_args"]
    assert [e.tool_call_id for e in arg_events] == ["call_42", "call_42"]
    assert "".join(e.tool_call_args_delta or "" for e in arg_events) == '{"city":"Paris"}'

    end = next(e for e in events if e.event_type == "tool_call_end")
    assert end.tool_call_id == "call_42"

    done = next(e for e in events if e.done)
    assert done.stop_reason == StopReason.tool_use


def test_args_delta_for_unknown_item_id_passes_through():
    state = _ChatGptCodexAdapterState()
    events = _parse_sse_event(
        json.dumps(
            {
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_orphan",
                "delta": "{}",
            }
        ),
        state,
    )
    assert len(events) == 1
    assert events[0].event_type == "tool_call_args"
    assert events[0].tool_call_id == "fc_orphan"
    assert events[0].tool_call_args_delta == "{}"


def test_adapter_maps_incomplete_to_max_tokens():
    events = _drive(
        [
            {
                "type": "response.completed",
                "response": {
                    "status": "incomplete",
                    "usage": {"input_tokens": 1, "output_tokens": 1},
                },
            },
        ]
    )
    done = next(e for e in events if e.done)
    assert done.stop_reason == StopReason.max_tokens


def test_adapter_surfaces_top_level_error_sets_state():
    state = _ChatGptCodexAdapterState()
    events = _parse_sse_event(
        json.dumps({"type": "error", "message": "rate limited", "code": "rate_limit_exceeded"}),
        state,
    )
    assert events == []
    assert state.error == "rate limited"


def test_response_failed_reads_nested_error_message():
    state = _ChatGptCodexAdapterState()
    _parse_sse_event(
        json.dumps({"type": "response.failed", "response": {"error": {"message": "boom"}}}),
        state,
    )
    assert state.error == "boom"


def test_error_without_message_uses_fallback():
    state = _ChatGptCodexAdapterState()
    _parse_sse_event(json.dumps({"type": "error"}), state)
    assert state.error == "ChatGPT-backend stream error"


@respx.mock
@pytest.mark.asyncio
async def test_stream_raises_stream_error_on_error_frame():
    sse = _sse_text({"type": "error", "message": "rate limited"})
    respx.post(_URL).mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    with pytest.raises(StreamError, match="rate limited"):
        async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
            pass
