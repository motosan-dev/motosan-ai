"""
Live ClaudeCodeClient integration tests -- shells out to real ``claude`` CLI.

Requires ``claude`` binary in PATH or CLAUDE_CODE_PATH env var.
Skips automatically if the binary is not found.

Run manually:
    uv run pytest sdks/python/tests/integration/test_claude_code_live.py -v
"""

from __future__ import annotations

import shutil

import pytest

from motosan_ai import ClaudeCodeClient, Message
from motosan_ai.types import ChatRequest

_HAS_CLAUDE = shutil.which("claude") is not None

pytestmark = [
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
