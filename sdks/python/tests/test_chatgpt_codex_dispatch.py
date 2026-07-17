from __future__ import annotations

import pytest

from motosan_ai import ChatGptCodexProvider, Client, Provider
from motosan_ai.error import ConfigError


def test_provider_enum_has_openai_chatgpt():
    assert Provider.openai_chatgpt == "openai_chatgpt"


def test_client_chatgpt_codex_classmethod():
    c = Client.chatgpt_codex(access_token="tok", account_id="acct-123", model="gpt-5.5")
    assert c.provider == Provider.openai_chatgpt
    assert isinstance(c._provider, ChatGptCodexProvider)
    assert c._provider.access_token == "tok"
    assert c._provider.account_id == "acct-123"
    assert c._provider.model == "gpt-5.5"
    # No api key required for this provider.
    assert c.api_key == ""


def test_client_chatgpt_codex_requires_access_token():
    with pytest.raises(ConfigError, match="access_token"):
        Client.chatgpt_codex(access_token=None, account_id="acct-123")


def test_client_chatgpt_codex_requires_account_id():
    with pytest.raises(ConfigError, match="account_id"):
        Client.chatgpt_codex(access_token="tok", account_id=None)


def test_chatgpt_codex_provider_exported_at_top_level():
    import motosan_ai

    assert "ChatGptCodexProvider" in motosan_ai.__all__
    assert motosan_ai.ChatGptCodexProvider is ChatGptCodexProvider


def test_client_chatgpt_codex_threads_reasoning_effort_default():
    c = Client.chatgpt_codex(
        access_token="tok",
        account_id="acct-123",
        model="gpt-5.5",
        reasoning_effort="high",
    )
    assert c._provider._reasoning_effort == "high"


def test_client_chatgpt_codex_reasoning_effort_defaults_none():
    c = Client.chatgpt_codex(access_token="tok", account_id="acct-123")
    assert c._provider._reasoning_effort is None


def test_client_chatgpt_codex_accepts_token_source_without_access_token():
    async def source() -> str:
        return "tok"

    c = Client.chatgpt_codex(account_id="acct-123", token_source=source)
    assert isinstance(c._provider, ChatGptCodexProvider)
    assert c._provider.token_source is source
    assert c._provider.access_token is None


def test_client_chatgpt_codex_token_source_still_requires_account_id():
    async def source() -> str:
        return "tok"

    with pytest.raises(ConfigError, match="account_id"):
        Client.chatgpt_codex(token_source=source)


def test_client_chatgpt_codex_error_mentions_token_source():
    with pytest.raises(ConfigError, match="access_token or token_source"):
        Client.chatgpt_codex(account_id="acct-123")
