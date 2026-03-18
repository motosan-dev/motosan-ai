from __future__ import annotations

import asyncio
import logging
import os
from enum import StrEnum
from typing import Any, AsyncIterator, Iterable

from motosan_ai.error import ConfigError, RateLimitError
from motosan_ai.providers import AnthropicProvider, MinimaxProvider, OpenAIProvider
from motosan_ai.think_stripper import ThinkStripper
from motosan_ai.types import ChatRequest, ChatResponse, Message, StreamEvent, Tool

logger = logging.getLogger(__name__)


class Provider(StrEnum):
    anthropic = "anthropic"
    openai = "openai"
    minimax = "minimax"
    ollama = "ollama"


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
        *,
        ollama_native: bool = False,
        ollama_think: bool = False,
        ollama_keep_alive: str | None = None,
        ollama_num_ctx: int | None = None,
        max_retries: int = 3,
    ) -> None:
        provider_value = Provider(provider)
        self.provider = provider_value
        self.model = model
        self._max_retries = max_retries

        if provider_value == Provider.ollama:
            self.api_key = api_key or ""
            if ollama_native:
                from motosan_ai.providers.ollama import OllamaProvider as NativeOllamaProvider

                self._provider = NativeOllamaProvider(
                    model=model or "llama3.2",
                    base_url=base_url or "http://localhost:11434",
                    think=ollama_think,
                    keep_alive=ollama_keep_alive,
                    num_ctx=ollama_num_ctx,
                )
            else:
                self._provider = OpenAIProvider(
                    api_key=self.api_key,
                    model=model or "llama3.2",
                    base_url=base_url or "http://localhost:11434/v1",
                )
        else:
            self.api_key = api_key or self._load_api_key(provider_value)
            if not self.api_key:
                raise ConfigError(f"Missing API key for provider: {provider_value.value}")

            if provider_value == Provider.anthropic:
                self._provider = AnthropicProvider(api_key=self.api_key, model=model)
            elif provider_value == Provider.openai:
                self._provider = OpenAIProvider(api_key=self.api_key, model=model, base_url=base_url)
            else:
                self._provider = MinimaxProvider(api_key=self.api_key, model=model, base_url=base_url)

    @classmethod
    def anthropic(
        cls,
        api_key: str | None = None,
        model: str | None = None,
        max_retries: int = 3,
    ) -> "Client":
        return cls(provider=Provider.anthropic, api_key=api_key, model=model, max_retries=max_retries)

    @classmethod
    def openai(
        cls,
        api_key: str | None = None,
        model: str | None = None,
        max_retries: int = 3,
    ) -> "Client":
        return cls(provider=Provider.openai, api_key=api_key, model=model, max_retries=max_retries)

    @classmethod
    def minimax(
        cls,
        api_key: str | None = None,
        model: str | None = None,
        base_url: str | None = None,
        max_retries: int = 3,
    ) -> "Client":
        return cls(provider=Provider.minimax, api_key=api_key, model=model, base_url=base_url, max_retries=max_retries)

    @classmethod
    def ollama(
        cls,
        model: str | None = None,
        base_url: str | None = None,
        *,
        native: bool = False,
        think: bool = False,
        keep_alive: str | None = None,
        num_ctx: int | None = None,
        max_retries: int = 3,
    ) -> "Client":
        return cls(
            provider=Provider.ollama,
            model=model,
            base_url=base_url,
            ollama_native=native,
            ollama_think=think,
            ollama_keep_alive=keep_alive,
            ollama_num_ctx=num_ctx,
            max_retries=max_retries,
        )

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
        if self._max_retries > 0:
            from motosan_ai.retry import with_retry
            return await with_retry(
                lambda: self._provider.chat(request),
                max_retries=self._max_retries,
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
        last_error: RateLimitError | None = None
        max_attempts = self._max_retries + 1 if self._max_retries > 0 else 1
        for attempt in range(max_attempts):
            try:
                stripper = ThinkStripper()
                async for event in self._provider.stream(request):
                    if event.event_type == "text" and event.content:
                        clean = stripper.feed(event.content)
                        if clean:
                            yield StreamEvent(content=clean, done=False)
                    else:
                        if event.done:
                            remaining = stripper.flush()
                            if remaining:
                                yield StreamEvent(content=remaining, done=False)
                        yield event
                return  # stream completed successfully
            except RateLimitError as e:
                last_error = e
                if attempt >= self._max_retries:
                    break
                from motosan_ai.retry import _parse_retry_after, DEFAULT_MAX_BACKOFF
                retry_after = _parse_retry_after(str(e))
                wait = min(retry_after if retry_after is not None else 1.0 * (2 ** attempt), DEFAULT_MAX_BACKOFF)
                logger.warning(
                    "Rate limited stream (attempt %d/%d), retrying in %.1fs",
                    attempt + 1, self._max_retries, wait,
                )
                await asyncio.sleep(wait)
        raise last_error  # type: ignore[misc]

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
