from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai.error import StreamError
from motosan_ai.providers.gemini_code_assist import (
    GeminiCodeAssistProvider,
    _CodeAssistAdapterState,
    _parse_sse_event,
)
from motosan_ai.types import ChatRequest, Message, StopReason


def _sse_text(*payloads: dict) -> str:
    return "\n".join(f"data: {json.dumps(p)}" for p in payloads) + "\n"


def test_empty_data_returns_no_events():
    state = _CodeAssistAdapterState()
    assert _parse_sse_event("", state) == []
    assert _parse_sse_event("[DONE]", state) == []


def test_text_part_emits_text_event():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {"response": {"candidates": [{"content": {"parts": [{"text": "hello"}]}}]}}
    )
    events = _parse_sse_event(payload, state)
    assert len(events) == 1
    assert events[0].content == "hello"
    assert events[0].done is False


def test_function_call_emits_start_args_end_in_order():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {
            "response": {
                "candidates": [
                    {
                        "content": {
                            "parts": [
                                {
                                    "functionCall": {
                                        "name": "get_weather",
                                        "args": {"city": "Taipei"},
                                    }
                                }
                            ]
                        }
                    }
                ]
            }
        }
    )
    events = _parse_sse_event(payload, state)
    assert [e.event_type for e in events] == ["tool_call_start", "tool_call_args", "tool_call_end"]
    assert events[0].tool_call_name == "get_weather"
    assert events[0].tool_call_id
    assert json.loads(events[1].tool_call_args_delta or "{}") == {"city": "Taipei"}


def test_function_call_uses_api_id_when_present_and_unique():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {
            "response": {
                "candidates": [
                    {
                        "content": {
                            "parts": [{"functionCall": {"id": "api-123", "name": "x", "args": {}}}]
                        }
                    }
                ]
            }
        }
    )
    events = _parse_sse_event(payload, state)
    assert next(e for e in events if e.event_type == "tool_call_start").tool_call_id == "api-123"


def test_function_call_regenerates_id_on_duplicate_seen():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {
            "response": {
                "candidates": [
                    {
                        "content": {
                            "parts": [{"functionCall": {"id": "dup", "name": "x", "args": {}}}]
                        }
                    }
                ]
            }
        }
    )
    id1 = next(
        e for e in _parse_sse_event(payload, state) if e.event_type == "tool_call_start"
    ).tool_call_id
    id2 = next(
        e for e in _parse_sse_event(payload, state) if e.event_type == "tool_call_start"
    ).tool_call_id
    assert id1 == "dup"
    assert id2 != "dup"
    assert (id2 or "").startswith("x_")


def test_usage_with_cached_subtracts_from_input_tokens():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {
            "response": {
                "candidates": [{"content": {"parts": [{"text": "ok"}]}}],
                "usageMetadata": {
                    "promptTokenCount": 100,
                    "cachedContentTokenCount": 30,
                    "candidatesTokenCount": 20,
                },
            }
        }
    )
    u = next(e.usage for e in _parse_sse_event(payload, state) if e.event_type == "usage")
    assert u is not None
    assert u.input_tokens == 70
    assert u.output_tokens == 20
    assert u.cache_read_input_tokens == 30


def test_usage_without_cached_returns_full_input_tokens():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {
            "response": {
                "candidates": [{"content": {"parts": [{"text": "ok"}]}}],
                "usageMetadata": {"promptTokenCount": 100, "candidatesTokenCount": 20},
            }
        }
    )
    u = next(e.usage for e in _parse_sse_event(payload, state) if e.event_type == "usage")
    assert u is not None
    assert u.input_tokens == 100
    assert u.cache_read_input_tokens is None


def test_finish_reason_stop_with_tool_call_emits_tool_use():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {
            "response": {
                "candidates": [
                    {
                        "content": {"parts": [{"functionCall": {"name": "x", "args": {}}}]},
                        "finishReason": "STOP",
                    }
                ]
            }
        }
    )
    done = next(e for e in _parse_sse_event(payload, state) if e.done)
    assert done.stop_reason == StopReason.tool_use


def test_finish_reason_stop_without_tool_call_emits_end_turn():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {
            "response": {
                "candidates": [{"content": {"parts": [{"text": "hi"}]}, "finishReason": "STOP"}]
            }
        }
    )
    done = next(e for e in _parse_sse_event(payload, state) if e.done)
    assert done.stop_reason == StopReason.end_turn


def test_finish_reason_max_tokens():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {
            "response": {
                "candidates": [
                    {"content": {"parts": [{"text": "trun"}]}, "finishReason": "MAX_TOKENS"}
                ]
            }
        }
    )
    done = next(e for e in _parse_sse_event(payload, state) if e.done)
    assert done.stop_reason == StopReason.max_tokens


def test_chunk_without_response_wrapper_skipped():
    state = _CodeAssistAdapterState()
    payload = json.dumps({"candidates": [{"content": {"parts": [{"text": "x"}]}}]})
    assert _parse_sse_event(payload, state) == []


def test_malformed_json_raises_stream_error():
    with pytest.raises(StreamError, match="malformed SSE chunk"):
        _parse_sse_event("not json {", _CodeAssistAdapterState())


@respx.mock
@pytest.mark.asyncio
async def test_stream_yields_text_then_done():
    sse = _sse_text(
        {
            "response": {
                "candidates": [{"content": {"parts": [{"text": "hi"}]}, "finishReason": "STOP"}]
            }
        }
    )
    respx.post("https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    p = GeminiCodeAssistProvider("ya29.fake", "myproj")
    events = [e async for e in p.stream(ChatRequest(messages=[Message.user("hi")]))]
    assert "".join(e.content for e in events if e.event_type == "text" and not e.done) == "hi"
    assert events[-1].done is True
    assert events[-1].stop_reason == StopReason.end_turn


@respx.mock
@pytest.mark.asyncio
async def test_chat_collects_stream_into_response():
    sse = _sse_text(
        {"response": {"candidates": [{"content": {"parts": [{"text": "Hello "}]}}]}},
        {
            "response": {
                "candidates": [{"content": {"parts": [{"text": "world."}]}}],
                "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 2},
            }
        },
        {"response": {"candidates": [{"content": {"parts": []}, "finishReason": "STOP"}]}},
    )
    respx.post("https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    resp = await GeminiCodeAssistProvider("ya29.fake", "myproj").chat(
        ChatRequest(messages=[Message.user("hi")])
    )
    assert resp.content == "Hello world."
    assert resp.usage.input_tokens == 5
    assert resp.usage.output_tokens == 2
    assert resp.stop_reason == StopReason.end_turn


@respx.mock
@pytest.mark.asyncio
async def test_stream_401_raises_auth_error():
    from motosan_ai.error import AuthError

    respx.post("https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse").mock(
        return_value=httpx.Response(401, json={"error": {"message": "expired token"}})
    )
    p = GeminiCodeAssistProvider("ya29.fake", "myproj")
    with pytest.raises(AuthError):
        async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
            pass


@respx.mock
@pytest.mark.asyncio
async def test_stream_sends_envelope_in_body():
    captured = {}

    def _capture(request):
        captured["body"] = json.loads(request.content)
        return httpx.Response(
            200,
            text=_sse_text(
                {
                    "response": {
                        "candidates": [
                            {"content": {"parts": [{"text": "x"}]}, "finishReason": "STOP"}
                        ]
                    }
                }
            ),
            headers={"content-type": "text/event-stream"},
        )

    respx.post("https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse").mock(
        side_effect=_capture
    )
    async for _ in GeminiCodeAssistProvider("ya29.fake", "myproj").stream(
        ChatRequest(messages=[Message.user("hi")])
    ):
        pass
    assert captured["body"]["project"] == "myproj"
    assert captured["body"]["userAgent"] == "motosan-ai"
    assert "contents" in captured["body"]["request"]


@respx.mock
@pytest.mark.asyncio
async def test_stream_raises_on_malformed_chunk():
    sse = (
        _sse_text({"response": {"candidates": [{"content": {"parts": [{"text": "hi"}]}}]}})
        + "data: {not valid json\n"
    )
    respx.post("https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    p = GeminiCodeAssistProvider("ya29.fake", "myproj")
    seen = []
    with pytest.raises(StreamError, match="malformed SSE chunk"):
        async for ev in p.stream(ChatRequest(messages=[Message.user("hi")])):
            seen.append(ev)
    assert any(e.content == "hi" for e in seen)
