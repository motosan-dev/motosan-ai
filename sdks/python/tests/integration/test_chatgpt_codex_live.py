"""Live integration test for ChatGptCodexProvider.

Skip unless ``MOTOSAN_RUN_CHATGPT_CODEX_LIVE=1`` and both
``CHATGPT_CODEX_ACCESS_TOKEN`` and ``CHATGPT_CODEX_ACCOUNT_ID`` are set in the
environment (the provider takes a pre-obtained token; there is no OAuth flow).
Optionally override the model via ``CHATGPT_CODEX_MODEL`` (default ``gpt-5.5``).
"""

from __future__ import annotations

import os

import pytest

from motosan_ai.providers.chatgpt_codex import ChatGptCodexProvider
from motosan_ai.types import ChatRequest, Message

_RUN = os.environ.get("MOTOSAN_RUN_CHATGPT_CODEX_LIVE") == "1"
_TOKEN = os.environ.get("CHATGPT_CODEX_ACCESS_TOKEN")
_ACCOUNT = os.environ.get("CHATGPT_CODEX_ACCOUNT_ID")
_MODEL = os.environ.get("CHATGPT_CODEX_MODEL", "gpt-5.5")

pytestmark = [
    pytest.mark.skipif(not _RUN, reason="set MOTOSAN_RUN_CHATGPT_CODEX_LIVE=1 to run"),
    pytest.mark.skipif(_TOKEN is None, reason="CHATGPT_CODEX_ACCESS_TOKEN not set"),
    pytest.mark.skipif(_ACCOUNT is None, reason="CHATGPT_CODEX_ACCOUNT_ID not set"),
    pytest.mark.asyncio,
]


@pytest.fixture
def provider() -> ChatGptCodexProvider:
    assert _TOKEN is not None and _ACCOUNT is not None
    return ChatGptCodexProvider(access_token=_TOKEN, account_id=_ACCOUNT, model=_MODEL)


async def test_live_chat_basic(provider: ChatGptCodexProvider):
    resp = await provider.chat(ChatRequest(messages=[Message.user("Reply with exactly: PONG")]))
    assert "PONG" in resp.content


async def test_live_stream_emits_text_then_done(provider: ChatGptCodexProvider):
    events = []
    async for event in provider.stream(
        ChatRequest(messages=[Message.user("Reply with: STREAM_OK")])
    ):
        events.append(event)
    text = "".join(e.content for e in events if e.event_type == "text" and not e.done)
    assert "STREAM_OK" in text
    assert events[-1].done is True
