import json

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message, ThinkingConfig


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


def _thinking_response() -> httpx.Response:
    return httpx.Response(
        200,
        json={
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 20},
            "content": [
                {"type": "thinking", "thinking": "Let me reason about this..."},
                {"type": "text", "text": "Answer: 42"},
            ],
        },
    )


@respx.mock
@pytest.mark.asyncio
async def test_thinking_serialized_in_body(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=_thinking_response()
    )
    req = ChatRequest(messages=[Message.user("solve")], thinking=ThinkingConfig(budget_tokens=4096))
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["thinking"] == {"type": "enabled", "budget_tokens": 4096}


@respx.mock
@pytest.mark.asyncio
async def test_thinking_forces_temperature_to_one(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=_thinking_response()
    )
    req = ChatRequest(
        messages=[Message.user("solve")],
        temperature=0.2,
        thinking=ThinkingConfig(budget_tokens=1024),
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["temperature"] == 1.0


@respx.mock
@pytest.mark.asyncio
async def test_thinking_blocks_parsed_into_response(provider):
    respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_thinking_response())
    req = ChatRequest(messages=[Message.user("solve")], thinking=ThinkingConfig(budget_tokens=1024))
    resp = await provider.chat(req)

    assert resp.thinking == "Let me reason about this..."
    assert resp.content == "Answer: 42"


def _sse_lines(*events: dict) -> str:
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


@respx.mock
@pytest.mark.asyncio
async def test_stream_emits_thinking_deltas_as_thinking_event(provider):
    """Regression: `thinking_delta` events were silently dropped in the stream path,
    so OAuth chat() (which collects from stream) never populated ChatResponse.thinking.
    """
    sse = _sse_lines(
        {
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "thinking", "thinking": ""},
        },
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "thinking_delta", "thinking": "Let me "},
        },
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "thinking_delta", "thinking": "reason..."},
        },
        {"type": "content_block_stop", "index": 0},
        {
            "type": "content_block_start",
            "index": 1,
            "content_block": {"type": "text", "text": ""},
        },
        {
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "text_delta", "text": "42"},
        },
        {"type": "message_stop"},
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    req = ChatRequest(messages=[Message.user("q")], thinking=ThinkingConfig(budget_tokens=1024))
    events = [e async for e in provider.stream(req)]

    thinking_events = [e for e in events if e.event_type == "thinking" and not e.done]
    assert [e.content for e in thinking_events] == ["Let me ", "reason..."]

    text_events = [e for e in events if e.event_type == "text" and not e.done]
    assert [e.content for e in text_events] == ["42"]


@respx.mock
@pytest.mark.asyncio
async def test_oauth_chat_collects_thinking_from_stream():
    """OAuth path runs chat() through stream+collect; must populate ChatResponse.thinking."""
    oauth_provider = AnthropicProvider("sk-ant-oat01-fake", base_url="https://mock.anthropic.com")
    sse = _sse_lines(
        {
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "thinking", "thinking": ""},
        },
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "thinking_delta", "thinking": "trace"},
        },
        {"type": "content_block_stop", "index": 0},
        {
            "type": "content_block_start",
            "index": 1,
            "content_block": {"type": "text", "text": ""},
        },
        {
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "text_delta", "text": "done"},
        },
        {"type": "message_stop"},
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    req = ChatRequest(messages=[Message.user("q")], thinking=ThinkingConfig(budget_tokens=1024))
    resp = await oauth_provider.chat(req)
    assert resp.thinking == "trace"
    assert resp.content == "done"
