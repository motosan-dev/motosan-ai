import json

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message, StreamEventType, ThinkingConfig


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
    assert body["thinking"] == {
        "type": "enabled",
        "budget_tokens": 4096,
        "display": "summarized",
    }


@respx.mock
@pytest.mark.asyncio
async def test_opus_4_8_thinking_uses_adaptive_shape(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200,
            json={
                "model": "claude-opus-4-8",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "content": [{"type": "text", "text": "ok"}],
            },
        )
    )
    req = ChatRequest(
        messages=[Message.user("solve")],
        model="claude-opus-4-8",
        thinking=ThinkingConfig(budget_tokens=4096),
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["thinking"] == {"type": "adaptive", "display": "summarized"}
    assert body["output_config"] == {"effort": "high"}
    assert "temperature" not in body


@respx.mock
@pytest.mark.asyncio
async def test_oauth_opus_4_8_adaptive_thinking_skips_interleaved_beta():
    oauth_provider = AnthropicProvider("sk-ant-oat01-fake", base_url="https://mock.anthropic.com")
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200,
            text=_sse_lines({"type": "message_stop"}),
            headers={"content-type": "text/event-stream"},
        )
    )
    req = ChatRequest(
        messages=[Message.user("solve")],
        model="claude-opus-4-8",
        thinking=ThinkingConfig(budget_tokens=4096),
    )
    events = [event async for event in oauth_provider.stream(req)]

    assert events[-1].done
    beta = route.calls[0].request.headers["anthropic-beta"]
    assert "oauth-2025-04-20" in beta
    assert "fine-grained-tool-streaming-2025-05-14" in beta
    assert "interleaved-thinking-2025-05-14" not in beta


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
async def test_stream_emits_typed_thinking_events(provider):
    """M4/F3: thinking blocks stream as thinking_delta events, then exactly one
    thinking_done event carrying the full concatenated text fires on
    content_block_stop — before any final-answer text events (mirrors the Rust
    Anthropic adapter).
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

    deltas = [e for e in events if e.event_type == StreamEventType.thinking_delta]
    assert [e.content for e in deltas] == ["Let me ", "reason..."]

    dones = [e for e in events if e.event_type == StreamEventType.thinking_done]
    assert [e.content for e in dones] == ["Let me reason..."]
    assert all(not e.done for e in deltas + dones)

    idx_done = next(
        i for i, e in enumerate(events) if e.event_type == StreamEventType.thinking_done
    )
    idx_first_text = next(
        i for i, e in enumerate(events) if e.event_type == StreamEventType.text and e.content
    )
    assert idx_done < idx_first_text
    assert [e.content for e in events if e.event_type == StreamEventType.text and e.content] == [
        "42"
    ]


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
