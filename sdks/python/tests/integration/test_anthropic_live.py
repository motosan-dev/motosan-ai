"""
Live Anthropic integration tests — hits real API with OAuth token.

Requires ANTHROPIC_API_KEY env var (supports sk-ant-oat01-* OAuth tokens).
Skips automatically if not set.

Run manually:
    ANTHROPIC_API_KEY=... uv run pytest sdks/python/tests/integration/test_anthropic_live.py -v
"""

from __future__ import annotations

import asyncio
import json
import os

import pytest

from motosan_ai import (
    ChatRequest,
    Client,
    Message,
    Provider,
    StreamEventType,
    SystemBlock,
    ThinkingConfig,
    Tool,
)
from motosan_ai.providers.anthropic import AnthropicProvider

API_KEY = os.environ.get("ANTHROPIC_API_KEY", "")
MODEL = "claude-sonnet-4-6"

pytestmark = [
    pytest.mark.skipif(not API_KEY, reason="ANTHROPIC_API_KEY not set"),
    pytest.mark.asyncio,
]

RATE_LIMIT_DELAY = 3


@pytest.fixture
def client():
    return Client(provider=Provider.anthropic, api_key=API_KEY, model=MODEL)


async def _cooldown():
    await asyncio.sleep(RATE_LIMIT_DELAY)


# ---------------------------------------------------------------------------
# 1. chat — basic
# ---------------------------------------------------------------------------


async def test_chat_basic(client):
    resp = await client.chat([Message.user("Reply with exactly one word: PONG")])
    assert "PONG" in resp.content
    assert resp.model
    await _cooldown()


# ---------------------------------------------------------------------------
# 2. stream — basic
# ---------------------------------------------------------------------------


async def test_stream_basic(client):
    chunks: list[str] = []
    async for event in client.stream([Message.user("Reply with exactly: STREAM_OK")]):
        if event.done:
            break
        if event.content:
            chunks.append(event.content)
    assert "STREAM_OK" in "".join(chunks)
    await _cooldown()


# ---------------------------------------------------------------------------
# 3. system prompt
# ---------------------------------------------------------------------------


async def test_system_prompt(client):
    resp = await client.chat(
        [Message.user("What is your name? Reply in one sentence.")],
        system="Your name is TestBot. Always introduce yourself as TestBot.",
    )
    assert "TestBot" in resp.content
    await _cooldown()


# ---------------------------------------------------------------------------
# 4. temperature
# ---------------------------------------------------------------------------


async def test_temperature(client):
    resp = await client.chat(
        [Message.user("Reply with exactly one word: TEMP_OK")],
        temperature=0.0,
    )
    assert "TEMP_OK" in resp.content
    await _cooldown()


# ---------------------------------------------------------------------------
# 5. tool use — single turn
# ---------------------------------------------------------------------------


async def test_tool_use_single_turn(client):
    tools = [
        Tool(
            name="get_weather",
            description="Get current weather for a city",
            input_schema={
                "type": "object",
                "properties": {"city": {"type": "string", "description": "City name"}},
                "required": ["city"],
            },
        )
    ]
    resp = await client.chat(
        [Message.user("What's the weather in Tokyo? Use the tool.")],
        tools=tools,
    )
    assert resp.tool_calls, f"Expected tool_calls, got none. content={resp.content!r}"
    tc = resp.tool_calls[0]
    assert tc.name == "get_weather"
    assert "city" in tc.input
    assert tc.id
    assert resp.stop_reason == "tool_use"
    await _cooldown()


# ---------------------------------------------------------------------------
# 6. tool use — multi-turn
# ---------------------------------------------------------------------------


async def test_tool_use_multi_turn(client):
    tools = [
        Tool(
            name="get_weather",
            description="Get current weather for a city",
            input_schema={
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"],
            },
        )
    ]
    messages = [Message.user("What's the weather in Taipei? Use get_weather tool.")]

    # Turn 1: model calls tool
    resp1 = await client.chat(messages, tools=tools)
    assert resp1.tool_calls, f"Expected tool call, got content={resp1.content!r}"
    tc = resp1.tool_calls[0]

    # Turn 2: provide tool result, get natural language answer
    messages.append(Message.assistant_with_tool_calls(resp1.content, resp1.tool_calls))
    messages.append(
        Message.tool_result(tc.id, json.dumps({"temperature": 28, "condition": "Sunny"}))
    )

    await _cooldown()
    resp2 = await client.chat(messages, tools=tools)
    assert resp2.content
    lower = resp2.content.lower()
    assert "28" in lower or "sunny" in lower or "taipei" in lower, f"got: {resp2.content!r}"
    await _cooldown()


# ---------------------------------------------------------------------------
# 7. stream + tool use
# ---------------------------------------------------------------------------


async def test_stream_tool_use(client):
    tools = [
        Tool(
            name="calculate",
            description="Calculate a math expression and return the result",
            input_schema={
                "type": "object",
                "properties": {"expression": {"type": "string", "description": "Math expression"}},
                "required": ["expression"],
            },
        )
    ]
    events = []
    async for event in client.stream(
        [Message.user("Use the calculate tool to compute 2+2.")],
        tools=tools,
    ):
        events.append(event)
        if event.done:
            break

    event_types = {e.event_type for e in events}
    assert "tool_call_start" in event_types
    assert "tool_call_args" in event_types
    assert "tool_call_end" in event_types

    starts = [e for e in events if e.event_type == "tool_call_start"]
    assert starts[0].tool_call_name == "calculate"
    assert starts[0].tool_call_id

    args_parts = [e.tool_call_args_delta for e in events if e.event_type == "tool_call_args"]
    parsed = json.loads("".join(args_parts))
    assert "expression" in parsed


@pytest.fixture
def anthropic_provider():
    return AnthropicProvider(api_key=API_KEY, model=MODEL)


async def _provider_chat(provider: AnthropicProvider, request: ChatRequest):
    from motosan_ai.retry import with_retry

    return await with_retry(lambda: provider.chat(request), max_retries=3)


async def test_live_vision(anthropic_provider):
    """Requires a real key + live network. Verifies vision roundtrip.

    Uses a 64x64 solid-red PNG — Anthropic's image validator rejects very small
    images, so a 1x1 pixel doesn't work.
    """
    red_square_png = "iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAIAAAAlC+aJAAAAf0lEQVR4nNXOQREAIAzAsFI1868HMYjgsWsU5NwZyiRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4twO/HrNmAGs/GAznAAAAABJRU5ErkJggg=="
    req = ChatRequest(
        messages=[Message.user_with_image("What color is this?", red_square_png, "image/png")],
        max_tokens=64,
    )
    resp = await _provider_chat(anthropic_provider, req)
    assert resp.content
    await _cooldown()


async def test_live_opus_4_8_adaptive_thinking():
    client = Client(provider=Provider.anthropic, api_key=API_KEY, model="claude-opus-4-8")
    req = ChatRequest(
        messages=[
            Message.user("Compute 37 * 43. Think thoroughly, then answer with the final integer.")
        ],
        thinking=ThinkingConfig(budget_tokens=1024),
        max_tokens=2048,
    )

    events = []
    async for event in client.stream_with(req):
        events.append(event)
        if event.done:
            break

    assert any(event.done for event in events)
    assert any(
        event.event_type == StreamEventType.thinking_delta and event.content for event in events
    )
    assert "".join(event.content for event in events if event.event_type == "text").strip()
    await _cooldown()


async def test_live_thinking(anthropic_provider):
    req = ChatRequest(
        messages=[Message.user("What is 13 * 17? Think step by step.")],
        thinking=ThinkingConfig(budget_tokens=1024),
        max_tokens=2048,
        model=MODEL,
    )
    resp = await _provider_chat(anthropic_provider, req)
    assert resp.thinking is not None
    assert "221" in resp.content or "221" in (resp.thinking or "")
    await _cooldown()


@pytest.mark.skipif(
    os.environ.get("ANTHROPIC_LIVE_CACHE") != "1",
    reason="prompt-cache live assertion is opt-in; set ANTHROPIC_LIVE_CACHE=1",
)
async def test_live_prompt_caching_reports_cache_tokens(anthropic_provider):
    big_system = "You are a helpful assistant.\n" * 600
    req = ChatRequest(
        messages=[Message.user("Say hi.")],
        system_blocks=[SystemBlock.cached(big_system)],
        max_tokens=32,
    )
    first = await _provider_chat(anthropic_provider, req)
    await _cooldown()
    second = await _provider_chat(anthropic_provider, req)

    assert (first.usage.cache_creation_input_tokens or 0) > 0
    assert (second.usage.cache_read_input_tokens or 0) > 0
    await _cooldown()
