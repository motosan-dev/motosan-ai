from __future__ import annotations

import asyncio
import os
from enum import StrEnum
from typing import Any, AsyncIterator, Iterable

from motosan_ai.error import ConfigError
from motosan_ai.providers import AnthropicProvider, MinimaxProvider, OpenAIProvider
from motosan_ai.types import ChatRequest, ChatResponse, Message, StreamEvent, Tool


class Provider(StrEnum):
    anthropic = "anthropic"
    openai = "openai"
    minimax = "minimax"


def _normalize_message(item: Message | dict[str, Any]) -> Message:
    if isinstance(item, Message):
        return item
    role = item.get("role")
    content = item.get("content", "")
    tool_call_id = item.get("tool_call_id")
    if role == "user":
        return Message.user(content)
    if role == "assistant":
        return Message.assistant(content)
    if role == "system":
        return Message.system(content)
    if role == "tool":
        return Message.tool_result(tool_call_id or "", content)
    raise ValueError(f"Unsupported message role: {role}")


class Client:
    def __init__(
        self,
        provider: Provider | str,
        api_key: str | None = None,
        model: str | None = None,
        base_url: str | None = None,
    ) -> None:
        provider_value = Provider(provider)
        self.provider = provider_value
        self.api_key = api_key or self._load_api_key(provider_value)
        self.model = model
        if not self.api_key:
            raise ConfigError(f"Missing API key for provider: {provider_value.value}")

        if provider_value == Provider.anthropic:
            self._provider = AnthropicProvider(api_key=self.api_key, model=model)
        elif provider_value == Provider.openai:
            self._provider = OpenAIProvider(api_key=self.api_key, model=model)
        else:
            self._provider = MinimaxProvider(api_key=self.api_key, model=model, base_url=base_url)

    @classmethod
    def anthropic(cls, api_key: str | None = None, model: str | None = None) -> "Client":
        return cls(provider=Provider.anthropic, api_key=api_key, model=model)

    @classmethod
    def openai(cls, api_key: str | None = None, model: str | None = None) -> "Client":
        return cls(provider=Provider.openai, api_key=api_key, model=model)

    @classmethod
    def minimax(
        cls,
        api_key: str | None = None,
        model: str | None = None,
        base_url: str | None = None,
    ) -> "Client":
        return cls(provider=Provider.minimax, api_key=api_key, model=model, base_url=base_url)

    @staticmethod
    def _load_api_key(provider: Provider) -> str | None:
        env_map = {
            Provider.anthropic: "ANTHROPIC_API_KEY",
            Provider.openai: "OPENAI_API_KEY",
            Provider.minimax: "MINIMAX_API_KEY",
        }
        return os.getenv(env_map[provider])

    def _build_request(
        self,
        messages: Iterable[Message | dict[str, Any]],
        *,
        tools: list[Tool] | None = None,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
        provider_options: dict[str, Any] | None = None,
    ) -> ChatRequest:
        normalized = [_normalize_message(m) for m in messages]
        return ChatRequest(
            messages=normalized,
            model=self.model,
            tools=tools,
            system=system,
            temperature=temperature,
            max_tokens=max_tokens,
            provider_options=provider_options,
        )

    async def chat(
        self,
        messages: Iterable[Message | dict[str, Any]],
        *,
        tools: list[Tool] | None = None,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
        provider_options: dict[str, Any] | None = None,
    ) -> ChatResponse:
        request = self._build_request(
            messages,
            tools=tools,
            system=system,
            temperature=temperature,
            max_tokens=max_tokens,
            provider_options=provider_options,
        )
        return await self._provider.chat(request)

    async def stream(
        self,
        messages: Iterable[Message | dict[str, Any]],
        *,
        tools: list[Tool] | None = None,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
        provider_options: dict[str, Any] | None = None,
    ) -> AsyncIterator[StreamEvent]:
        request = self._build_request(
            messages,
            tools=tools,
            system=system,
            temperature=temperature,
            max_tokens=max_tokens,
            provider_options=provider_options,
        )
        async for event in self._provider.stream(request):
            yield event

    def chat_sync(
        self,
        messages: Iterable[Message | dict[str, Any]],
        *,
        tools: list[Tool] | None = None,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
        provider_options: dict[str, Any] | None = None,
    ) -> ChatResponse:
        return asyncio.run(
            self.chat(
                messages,
                tools=tools,
                system=system,
                temperature=temperature,
                max_tokens=max_tokens,
                provider_options=provider_options,
            )
        )
