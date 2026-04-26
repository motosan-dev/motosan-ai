from __future__ import annotations

import warnings

import httpx
import respx

from motosan_ai import Client, Provider
from motosan_ai.types import Message


@respx.mock
def test_chat_sync_emits_deprecation_warning(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200,
            json={
                "model": "claude-sonnet-4-6",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "content": [{"type": "text", "text": "ok"}],
            },
        )
    )
    client = Client(provider=Provider.anthropic)
    with warnings.catch_warnings(record=True) as recorded:
        warnings.simplefilter("always")
        resp = client.chat_sync([Message.user("hi")])
    assert resp.content == "ok"

    deprecations = [w for w in recorded if issubclass(w.category, DeprecationWarning)]
    assert len(deprecations) == 1
    assert "chat_sync" in str(deprecations[0].message)
    assert "asyncio.run" in str(deprecations[0].message)
