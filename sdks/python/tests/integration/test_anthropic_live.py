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

from motosan_ai import Client, Message, Provider, Tool

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
    messages.append(Message.tool_result(tc.id, json.dumps({"temperature": 28, "condition": "Sunny"})))

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
