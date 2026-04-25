"""Live integration test for GeminiCodeAssistProvider.

Skip unless `MOTOSAN_RUN_CODE_ASSIST_LIVE=1`, a usable cached Google OAuth
token exists at the default cache path, and ``GOOGLE_PROJECT_ID`` is set.
"""

from __future__ import annotations

import os

import pytest

from motosan_ai.oauth import DEFAULT_CACHE_PATH, ensure_fresh_token, google_gemini_config
from motosan_ai.providers.gemini_code_assist import GeminiCodeAssistProvider
from motosan_ai.types import ChatRequest, Message

_RUN = os.environ.get("MOTOSAN_RUN_CODE_ASSIST_LIVE") == "1"
_PROJECT = os.environ.get("GOOGLE_PROJECT_ID")
_TOKEN_PRESENT = DEFAULT_CACHE_PATH.exists()

pytestmark = [
    pytest.mark.skipif(not _RUN, reason="set MOTOSAN_RUN_CODE_ASSIST_LIVE=1 to run"),
    pytest.mark.skipif(_PROJECT is None, reason="GOOGLE_PROJECT_ID not set"),
    pytest.mark.skipif(
        not _TOKEN_PRESENT, reason=f"no cached token at {DEFAULT_CACHE_PATH}; run login first"
    ),
    pytest.mark.asyncio,
]


@pytest.fixture
async def provider() -> GeminiCodeAssistProvider:
    token = await ensure_fresh_token(google_gemini_config())
    assert _PROJECT is not None
    return GeminiCodeAssistProvider(access_token=token.access_token, project_id=_PROJECT)


async def test_live_chat_basic(provider: GeminiCodeAssistProvider):
    resp = await provider.chat(ChatRequest(messages=[Message.user("Reply with exactly: PONG")]))
    assert "PONG" in resp.content


async def test_live_stream_emits_text_then_done(provider: GeminiCodeAssistProvider):
    events = []
    async for event in provider.stream(
        ChatRequest(messages=[Message.user("Reply with: STREAM_OK")])
    ):
        events.append(event)
    text = "".join(e.content for e in events if e.event_type == "text" and not e.done)
    assert "STREAM_OK" in text
    assert events[-1].done is True
