from __future__ import annotations

import httpx
import pytest
import respx

from motosan_ai import Client, Provider
from motosan_ai.error import AuthError, ConfigError, RateLimitError
from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.providers.minimax import MinimaxProvider
from motosan_ai.providers.openai import OpenAIProvider


@pytest.mark.parametrize(
    "provider_enum,env_var,expected_class",
    [
        (Provider.anthropic, "ANTHROPIC_API_KEY", AnthropicProvider),
        (Provider.openai, "OPENAI_API_KEY", OpenAIProvider),
        (Provider.gemini, "GEMINI_API_KEY", GeminiProvider),
        (Provider.minimax, "MINIMAX_API_KEY", MinimaxProvider),
    ],
    ids=["anthropic", "openai", "gemini", "minimax"],
)
def test_client_dispatch_resolves_correct_provider_class(
    provider_enum, env_var, expected_class, monkeypatch
):
    monkeypatch.setenv(env_var, "test-key-from-env")
    client = Client(provider=provider_enum)
    assert isinstance(client._provider, expected_class)
    assert client.api_key == "test-key-from-env"


@pytest.mark.parametrize(
    "provider_enum,env_var",
    [
        (Provider.anthropic, "ANTHROPIC_API_KEY"),
        (Provider.openai, "OPENAI_API_KEY"),
        (Provider.gemini, "GEMINI_API_KEY"),
        (Provider.minimax, "MINIMAX_API_KEY"),
    ],
)
def test_missing_env_var_raises_config_error(provider_enum, env_var, monkeypatch):
    monkeypatch.delenv(env_var, raising=False)
    with pytest.raises(ConfigError):
        Client(provider=provider_enum)


def test_explicit_api_key_overrides_env(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "env-key")
    client = Client(provider=Provider.anthropic, api_key="explicit-key")
    assert client.api_key == "explicit-key"


@respx.mock
@pytest.mark.asyncio
async def test_client_retries_on_429_then_succeeds(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        side_effect=[
            httpx.Response(429, json={"error": {"message": "slow down"}}),
            httpx.Response(
                200,
                json={
                    "model": "claude-sonnet-4-6",
                    "stop_reason": "end_turn",
                    "usage": {"input_tokens": 1, "output_tokens": 1},
                    "content": [{"type": "text", "text": "ok"}],
                },
            ),
        ]
    )
    client = Client(provider=Provider.anthropic, max_retries=2)
    resp = await client.chat([{"role": "user", "content": "hi"}])
    assert resp.content == "ok"
    assert route.call_count == 2


@respx.mock
@pytest.mark.asyncio
async def test_client_gives_up_after_max_retries(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(429, json={"error": {"message": "slow"}})
    )
    client = Client(provider=Provider.anthropic, max_retries=1)
    with pytest.raises(RateLimitError):
        await client.chat([{"role": "user", "content": "hi"}])
    assert route.call_count == 2


@respx.mock
@pytest.mark.asyncio
async def test_client_does_not_retry_on_4xx_non_429(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(401, json={"error": {"message": "bad"}})
    )
    client = Client(provider=Provider.anthropic, max_retries=3)
    with pytest.raises(AuthError):
        await client.chat([{"role": "user", "content": "hi"}])
    assert route.call_count == 1
