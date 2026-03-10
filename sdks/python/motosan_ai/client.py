from __future__ import annotations
import asyncio
from typing import AsyncIterator, Literal

from motosan_ai.types import ChatRequest, ChatResponse, Message, StreamEvent
from motosan_ai.providers.base import ProviderProtocol

ProviderName = Literal["anthropic", "openai", "minimax"]


def _create_provider(provider: ProviderName, api_key: str) -> ProviderProtocol:
    if provider == "anthropic":
        from motosan_ai.providers.anthropic import AnthropicProvider
        return AnthropicProvider(api_key=api_key)
    elif provider == "openai":
        from motosan_ai.providers.openai import OpenAIProvider
        return OpenAIProvider(api_key=api_key)
    elif provider == "minimax":
        from motosan_ai.providers.minimax import MinimaxProvider
        return MinimaxProvider(api_key=api_key)
    else:
        raise ValueError(f"Unknown provider: {provider!r}. Choose from: anthropic, openai, minimax")


class Client:
    """
    Unified AI client. Works with Anthropic, OpenAI, and MiniMax.

    Usage::

        client = Client(provider="anthropic")
        response = await client.chat([{"role": "user", "content": "Hello"}])
        print(response.content)

    Switch provider by changing one line::

        client = Client(provider="openai")   # same code, different model
    """

    def __init__(
        self,
        provider: ProviderName,
        api_key: str | None = None,
        *,
        model: str | None = None,
    ):
        import os
        _key_env = {
            "anthropic": "ANTHROPIC_API_KEY",
            "openai": "OPENAI_API_KEY",
            "minimax": "MINIMAX_API_KEY",
        }
        resolved_key = api_key or os.environ.get(_key_env.get(provider, ""), "")
        if not resolved_key:
            raise ValueError(
                f"api_key is required. Pass it explicitly or set {_key_env.get(provider)}"
            )
        self._provider = _create_provider(provider, resolved_key)
        self._default_model = model

    async def chat(
        self,
        messages: list[Message | dict],
        *,
        model: str | None = None,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
        provider_options: dict | None = None,
    ) -> ChatResponse:
        """Send a chat request and return the full response."""
        req = self._build_request(
            messages, model=model, system=system,
            temperature=temperature, max_tokens=max_tokens,
            provider_options=provider_options,
        )
        return await self._provider.chat(req)

    async def stream(
        self,
        messages: list[Message | dict],
        *,
        model: str | None = None,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
    ) -> AsyncIterator[StreamEvent]:
        """Stream a response token by token."""
        req = self._build_request(messages, model=model, system=system,
                                  temperature=temperature, max_tokens=max_tokens)
        async for event in self._provider.stream(req):
            yield event

    def chat_sync(self, messages: list[Message | dict], **kwargs) -> ChatResponse:
        """Synchronous wrapper for chat(). Useful in non-async contexts."""
        return asyncio.run(self.chat(messages, **kwargs))

    def _build_request(self, messages: list[Message | dict], **kwargs) -> ChatRequest:
        normalized: list[Message] = []
        for m in messages:
            if isinstance(m, dict):
                normalized.append(Message(role=m["role"], content=m["content"]))
            else:
                normalized.append(m)
        return ChatRequest(
            messages=normalized,
            model=kwargs.get("model") or self._default_model,
            system=kwargs.get("system"),
            temperature=kwargs.get("temperature"),
            max_tokens=kwargs.get("max_tokens"),
            provider_options=kwargs.get("provider_options"),
        )
