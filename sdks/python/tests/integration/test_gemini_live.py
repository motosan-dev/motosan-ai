"""Live integration tests for GeminiProvider.

Requires GEMINI_API_KEY environment variable. Skipped otherwise.
"""

import os

import pytest

from motosan_ai import Client, Message, Tool


@pytest.fixture
def client():
    key = os.getenv("GEMINI_API_KEY")
    if not key:
        pytest.skip("GEMINI_API_KEY not set")
    return Client.gemini(api_key=key)


@pytest.mark.asyncio
async def test_live_simple_chat(client):
    resp = await client.chat([Message.user("Say exactly: pong")], max_tokens=32)
    assert resp.content
    assert "pong" in resp.content.lower()


@pytest.mark.asyncio
async def test_live_vision(client):
    # 64x64 solid-red PNG (187 bytes) — Gemini and Anthropic both reject 1x1 pixels.
    red_square_png = "iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAIAAAAlC+aJAAAAf0lEQVR4nNXOQREAIAzAsFI1868HMYjgsWsU5NwZyiRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4iRO4twO/HrNmAGs/GAznAAAAABJRU5ErkJggg=="
    resp = await client.chat(
        [
            Message.user_with_image(
                "What do you see? Reply in 5 words.", red_square_png, "image/png"
            )
        ],
        max_tokens=64,
    )
    assert resp.content


@pytest.mark.asyncio
async def test_live_tool_use(client):
    tools = [
        Tool(
            name="get_weather",
            description="Get weather for a city",
            input_schema={
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"],
            },
        )
    ]
    resp = await client.chat(
        [Message.user("What's the weather in Taipei?")], tools=tools, max_tokens=256
    )
    assert resp.content or resp.tool_calls


@pytest.mark.asyncio
async def test_live_streaming(client):
    chunks = []
    async for ev in client.stream([Message.user("Count from 1 to 5.")], max_tokens=64):
        if ev.event_type == "text" and not ev.done:
            chunks.append(ev.content)
    full = "".join(chunks)
    assert any(str(n) in full for n in range(1, 6))
