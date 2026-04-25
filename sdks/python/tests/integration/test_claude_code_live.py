"""
Live ClaudeCodeClient integration tests -- shells out to real ``claude`` CLI.

Set MOTOSAN_RUN_CLAUDE_CODE_LIVE=1 to run. Requires ``claude`` binary in PATH or CLAUDE_CODE_PATH env var.
Skips automatically if the binary is not found.

Run manually:
    uv run pytest sdks/python/tests/integration/test_claude_code_live.py -v
"""

from __future__ import annotations

import os
import shutil

import pytest

from motosan_ai import ClaudeCodeClient, Message
from motosan_ai.types import ChatRequest

_HAS_CLAUDE = shutil.which(os.environ.get("CLAUDE_CODE_PATH", "claude")) is not None
_RUN_LIVE = os.environ.get("MOTOSAN_RUN_CLAUDE_CODE_LIVE") == "1"

pytestmark = [
    pytest.mark.skipif(not _RUN_LIVE, reason="set MOTOSAN_RUN_CLAUDE_CODE_LIVE=1 to run"),
    pytest.mark.skipif(not _HAS_CLAUDE, reason="claude CLI not found in PATH"),
    pytest.mark.asyncio,
]


@pytest.fixture
def client():
    return ClaudeCodeClient()


async def test_chat_roundtrip(client):
    request = ChatRequest(messages=[Message.user("Reply with only the word 'pong'.")])
    resp = await client.chat(request)
    assert "pong" in resp.content.lower()
    assert resp.tool_calls == []


async def test_stream_roundtrip(client):
    request = ChatRequest(messages=[Message.user("Reply with only the word 'pong'.")])
    chunks: list[str] = []
    async for event in client.stream(request):
        if event.done:
            break
        if event.content:
            chunks.append(event.content)
    text = "".join(chunks)
    assert "pong" in text.lower()


async def test_live_system_prompt():
    client = ClaudeCodeClient().system_prompt("Your name is TestBot. Always identify yourself.")
    resp = await client.chat(ChatRequest(messages=[Message.user("Hi, who are you?")]))
    assert "TestBot" in resp.content, f"system_prompt not applied: {resp.content!r}"


async def test_live_session_id_roundtrip():
    import uuid

    sid = str(uuid.uuid4())
    client = ClaudeCodeClient().session_id(sid)
    resp1 = await client.chat(ChatRequest(messages=[Message.user("Remember the number 7788.")]))
    assert resp1.content

    client2 = ClaudeCodeClient().resume(sid)
    resp2 = await client2.chat(
        ChatRequest(messages=[Message.user("What number did I ask you to remember?")])
    )
    assert "7788" in resp2.content, f"session not resumed: {resp2.content!r}"


async def test_live_stream_emits_usage_event():
    client = ClaudeCodeClient()
    events = []
    async for event in client.stream(ChatRequest(messages=[Message.user("Count to 3.")])):
        events.append(event)

    usage_events = [e for e in events if e.event_type == "usage"]
    assert len(usage_events) == 1, f"expected exactly 1 usage event, got {len(usage_events)}"
    assert usage_events[0].usage is not None
    assert usage_events[0].usage.input_tokens > 0
    assert usage_events[0].usage.output_tokens > 0

    done_events = [e for e in events if e.done]
    assert len(done_events) == 1
    assert events.index(usage_events[0]) < events.index(done_events[0])
