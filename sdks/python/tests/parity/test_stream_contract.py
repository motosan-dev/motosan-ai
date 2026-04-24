from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai.types import ChatRequest, Message


def _anthropic_sse() -> str:
    events = [
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "hello"},
        },
        {"type": "message_stop"},
    ]
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


def _openai_sse() -> str:
    events = [
        {
            "id": "chatcmpl-1",
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": {"content": "hello"}}],
        },
        {
            "id": "chatcmpl-1",
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
        },
    ]
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\ndata: [DONE]\n"


def _gemini_sse() -> str:
    events = [{"candidates": [{"content": {"parts": [{"text": "hello"}]}, "finishReason": "STOP"}]}]
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


def _minimax_sse() -> str:
    events = [
        {"id": "1", "choices": [{"index": 0, "delta": {"content": "hello"}}]},
        {"id": "1", "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]},
    ]
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\ndata: [DONE]\n"


_SSE_BY_PROVIDER = {
    "anthropic": _anthropic_sse,
    "openai": _openai_sse,
    "gemini": _gemini_sse,
    "minimax": _minimax_sse,
}


@respx.mock
@pytest.mark.asyncio
async def test_text_stream_contract(provider_under_test):
    respx.post(provider_under_test.stream_endpoint).mock(
        return_value=httpx.Response(
            200,
            text=_SSE_BY_PROVIDER[provider_under_test.name](),
            headers={"content-type": "text/event-stream"},
        )
    )
    events = [
        event
        async for event in provider_under_test.provider.stream(
            ChatRequest(messages=[Message.user("hi")])
        )
    ]
    text_events = [event for event in events if event.event_type == "text" and not event.done]
    done_events = [event for event in events if event.done]
    assert len(text_events) >= 1
    assert "".join(event.content for event in text_events) == "hello"
    assert len(done_events) == 1
    assert events[-1].done
