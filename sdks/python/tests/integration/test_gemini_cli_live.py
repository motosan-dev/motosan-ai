"""Live integration tests for GeminiCliClient.

Skip unless `gemini` is on PATH (or `GEMINI_CLI_PATH` points at it) and
`MOTOSAN_RUN_GEMINI_CLI_LIVE=1` is set.
"""

from __future__ import annotations

import os
import shutil

import pytest

from motosan_ai.providers.gemini_cli import ApprovalMode, GeminiCliClient
from motosan_ai.types import ChatRequest, Message

_BINARY = os.environ.get("GEMINI_CLI_PATH") or shutil.which("gemini")
_RUN = os.environ.get("MOTOSAN_RUN_GEMINI_CLI_LIVE") == "1"

pytestmark = [
    pytest.mark.skipif(_BINARY is None, reason="gemini binary not on PATH"),
    pytest.mark.skipif(not _RUN, reason="set MOTOSAN_RUN_GEMINI_CLI_LIVE=1 to run"),
    pytest.mark.asyncio,
]


async def test_live_chat_basic():
    client = GeminiCliClient().approval_mode(ApprovalMode.plan)
    resp = await client.chat(ChatRequest(messages=[Message.user("Reply with exactly: PONG")]))
    assert "PONG" in resp.content


async def test_live_stream_emits_text_then_done():
    client = GeminiCliClient().approval_mode(ApprovalMode.plan)
    events = []
    async for event in client.stream(ChatRequest(messages=[Message.user("Reply with: STREAM_OK")])):
        events.append(event)
    text_content = "".join(e.content for e in events if e.event_type == "text" and not e.done)
    assert "STREAM_OK" in text_content
    assert events[-1].done is True


async def test_live_stream_emits_usage_event():
    client = GeminiCliClient().approval_mode(ApprovalMode.plan)
    events = []
    async for event in client.stream(ChatRequest(messages=[Message.user("Count to 3.")])):
        events.append(event)
    usage_events = [e for e in events if e.event_type == "usage"]
    assert len(usage_events) >= 1
    assert usage_events[0].usage is not None
    assert usage_events[0].usage.input_tokens > 0
    assert usage_events[0].usage.output_tokens > 0
