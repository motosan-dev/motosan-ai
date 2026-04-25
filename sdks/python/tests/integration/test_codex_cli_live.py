"""Live integration tests for CodexCliClient.

Set MOTOSAN_RUN_CODEX_LIVE=1 to run. Skips when `codex` binary is not
on PATH (or `CODEX_PATH` not pointing at one), or when a preflight turn
shows the local Codex auth/model is not usable.
"""

from __future__ import annotations

import os
import shutil

import pytest

from motosan_ai.error import ProviderError
from motosan_ai.providers.codex_cli import CodexCliClient, SandboxMode
from motosan_ai.types import ChatRequest, Message

_BINARY = os.environ.get("CODEX_PATH") or shutil.which("codex")
_RUN_LIVE = os.environ.get("MOTOSAN_RUN_CODEX_LIVE") == "1"
_CODEX_MODEL = os.environ.get("MOTOSAN_CODEX_MODEL", "gpt-5.1-codex")

pytestmark = [
    pytest.mark.skipif(not _RUN_LIVE, reason="set MOTOSAN_RUN_CODEX_LIVE=1 to run"),
    pytest.mark.skipif(_BINARY is None, reason="codex binary not on PATH"),
    pytest.mark.asyncio,
]


@pytest.fixture(scope="session")
async def live_codex_client() -> CodexCliClient:
    """Preflight Codex auth/model before running live tests.

    Codex has no stable `auth status` command across CLI versions. A tiny
    read-only turn is the most reliable auth+model probe; skip with the
    upstream message if the configured account/model cannot run.
    """
    client = CodexCliClient().sandbox(SandboxMode.read_only).model(_CODEX_MODEL)
    try:
        await client.chat(ChatRequest(messages=[Message.user("Reply with exactly: OK")]))
    except ProviderError as exc:
        pytest.skip(
            "codex live preflight failed; check auth/version/model "
            f"(MOTOSAN_CODEX_MODEL={_CODEX_MODEL!r}): {exc}"
        )
    return client


async def test_live_chat_basic(live_codex_client: CodexCliClient):
    resp = await live_codex_client.chat(
        ChatRequest(messages=[Message.user("Reply with exactly: PONG")])
    )
    assert "PONG" in resp.content


async def test_live_stream_emits_text_then_done(live_codex_client: CodexCliClient):
    events = []
    async for event in live_codex_client.stream(
        ChatRequest(messages=[Message.user("Reply with: STREAM_OK")])
    ):
        events.append(event)

    text_content = "".join(e.content for e in events if e.event_type == "text" and not e.done)
    assert "STREAM_OK" in text_content
    assert events[-1].done is True


async def test_live_stream_emits_usage_event(live_codex_client: CodexCliClient):
    events = []
    async for event in live_codex_client.stream(
        ChatRequest(messages=[Message.user("Count to 3.")])
    ):
        events.append(event)

    usage_events = [e for e in events if e.event_type == "usage"]
    assert len(usage_events) >= 1
    assert usage_events[0].usage is not None
    assert usage_events[0].usage.input_tokens > 0
    assert usage_events[0].usage.output_tokens > 0
