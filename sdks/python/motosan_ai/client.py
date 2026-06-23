from __future__ import annotations

import asyncio
import logging
import os
from collections.abc import AsyncIterator, Iterable
from dataclasses import replace
from enum import StrEnum
from typing import Any

from motosan_ai.error import ConfigError, NetworkError, ProviderError, RateLimitError
from motosan_ai.providers import (
    AnthropicProvider,
    CodexCliClient,
    GeminiCliClient,
    GeminiCodeAssistProvider,
    GeminiProvider,
    MinimaxProvider,
    OpenAIProvider,
)
from motosan_ai.think_stripper import ThinkStripper
from motosan_ai.types import ChatRequest, ChatResponse, Message, StreamEvent, Tool

logger = logging.getLogger(__name__)


class Provider(StrEnum):
    anthropic = "anthropic"
    openai = "openai"
    minimax = "minimax"
    ollama = "ollama"
    gemini = "gemini"
    codex_cli = "codex_cli"
    gemini_cli = "gemini_cli"
    gemini_code_assist = "gemini_code_assist"


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
        binary_path: str | None = None,
        access_token: str | None = None,
        project_id: str | None = None,
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

        if provider_value == Provider.gemini_code_assist:
            if not access_token:
                raise ConfigError("gemini_code_assist requires access_token")
            if not project_id:
                raise ConfigError("gemini_code_assist requires project_id")
            self.api_key = ""
            self._provider = GeminiCodeAssistProvider(
                access_token=access_token,
                project_id=project_id,
                model=model,
                base_url=base_url,
            )
        elif provider_value == Provider.codex_cli:
            self.api_key = ""
            self._provider = CodexCliClient(binary_path=binary_path)
        elif provider_value == Provider.gemini_cli:
            self.api_key = ""
            self._provider = GeminiCliClient(binary_path=binary_path)
        elif provider_value == Provider.ollama:
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
                self._provider = OpenAIProvider(
                    api_key=self.api_key, model=model, base_url=base_url
                )
            elif provider_value == Provider.gemini:
                self._provider = GeminiProvider(
                    api_key=self.api_key, model=model, base_url=base_url
                )
            else:
                self._provider = MinimaxProvider(
                    api_key=self.api_key, model=model, base_url=base_url
                )

    @classmethod
    def anthropic(
        cls,
        api_key: str | None = None,
        model: str | None = None,
        max_retries: int = 3,
    ) -> Client:
        return cls(
            provider=Provider.anthropic, api_key=api_key, model=model, max_retries=max_retries
        )

    @classmethod
    def openai(
        cls,
        api_key: str | None = None,
        model: str | None = None,
        max_retries: int = 3,
    ) -> Client:
        return cls(provider=Provider.openai, api_key=api_key, model=model, max_retries=max_retries)

    @classmethod
    def gemini(
        cls,
        api_key: str | None = None,
        model: str | None = None,
        base_url: str | None = None,
        max_retries: int = 3,
    ) -> Client:
        return cls(
            provider=Provider.gemini,
            api_key=api_key,
            model=model,
            base_url=base_url,
            max_retries=max_retries,
        )

    @classmethod
    def gemini_code_assist(
        cls,
        access_token: str | None = None,
        project_id: str | None = None,
        model: str | None = None,
        base_url: str | None = None,
        max_retries: int = 3,
    ) -> Client:
        return cls(
            provider=Provider.gemini_code_assist,
            access_token=access_token,
            project_id=project_id,
            model=model,
            base_url=base_url,
            max_retries=max_retries,
        )

    @classmethod
    def minimax(
        cls,
        api_key: str | None = None,
        model: str | None = None,
        base_url: str | None = None,
        max_retries: int = 3,
    ) -> Client:
        return cls(
            provider=Provider.minimax,
            api_key=api_key,
            model=model,
            base_url=base_url,
            max_retries=max_retries,
        )

    @classmethod
    def codex_cli(
        cls,
        binary_path: str | None = None,
        model: str | None = None,
        max_retries: int = 3,
    ) -> Client:
        return cls(
            provider=Provider.codex_cli,
            binary_path=binary_path,
            model=model,
            max_retries=max_retries,
        )

    @classmethod
    def gemini_cli(
        cls,
        binary_path: str | None = None,
        model: str | None = None,
        max_retries: int = 3,
    ) -> Client:
        return cls(
            provider=Provider.gemini_cli,
            binary_path=binary_path,
            model=model,
            max_retries=max_retries,
        )

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
    ) -> Client:
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
            Provider.gemini: "GEMINI_API_KEY",
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

    async def chat_with(self, request: ChatRequest) -> ChatResponse:
        """Send a fully-built ChatRequest.

        Use this when you need fields that ``chat()`` kwargs do not expose,
        such as tool_choice, thinking, mcp_servers, system_blocks, or
        stop_sequences. If ``request.model`` is None, ``self.model`` is used.
        """
        if request.model is None and self.model is not None:
            request = replace(request, model=self.model)

        if self._max_retries > 0:
            from motosan_ai.retry import with_retry

            return await with_retry(
                lambda: self._provider.chat(request),
                max_retries=self._max_retries,
            )
        return await self._provider.chat(request)

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
        return await self.chat_with(request)

    async def stream_with(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        """Stream a fully-built ChatRequest.

        If ``request.model`` is None, ``self.model`` is used. Retry semantics
        are identical to the previous ``stream()`` implementation.
        """
        if request.model is None and self.model is not None:
            request = replace(request, model=self.model)

        last_error: RateLimitError | None = None
        max_attempts = self._max_retries + 1 if self._max_retries > 0 else 1
        for attempt in range(max_attempts):
            yielded = False
            try:
                stripper = ThinkStripper()
                async for event in self._provider.stream(request):
                    if event.event_type == "text" and event.content:
                        clean = stripper.feed(event.content)
                        if clean:
                            yielded = True
                            yield StreamEvent(content=clean, done=False)
                    else:
                        if event.done:
                            remaining = stripper.flush()
                            if remaining:
                                yielded = True
                                yield StreamEvent(content=remaining, done=False)
                        yielded = True
                        yield event
                return
            except (RateLimitError, NetworkError, ProviderError) as e:
                from motosan_ai.retry import (
                    DEFAULT_INITIAL_BACKOFF,
                    DEFAULT_MAX_BACKOFF,
                    _is_retryable,
                    _parse_retry_after,
                )

                # F1: once the stream has emitted any event, a mid-stream error
                # must propagate verbatim — retrying would replay a partially
                # consumed request and double-emit. Only connection-time
                # (pre-first-yield) failures are retried.
                if yielded or not _is_retryable(e):
                    raise
                last_error = e
                if attempt >= self._max_retries:
                    break
                retry_after = _parse_retry_after(str(e))
                wait = min(
                    retry_after
                    if retry_after is not None
                    else DEFAULT_INITIAL_BACKOFF * (2**attempt),
                    DEFAULT_MAX_BACKOFF,
                )
                logger.warning(
                    "Retryable stream error (attempt %d/%d), retrying in %.1fs: %s",
                    attempt + 1,
                    self._max_retries,
                    wait,
                    type(e).__name__,
                )
                await asyncio.sleep(wait)
        raise last_error  # type: ignore[misc]

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
        async for event in self.stream_with(request):
            yield event

    async def stream_collect(
        self,
        messages: Iterable[Message | dict[str, Any]],
        *,
        tools: list[Tool] | None = None,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
        provider_options: dict[str, Any] | None = None,
    ) -> ChatResponse:
        """Stream a chat request and assemble the full ChatResponse."""
        from motosan_ai._stream_collect import collect_stream

        response = await collect_stream(
            self.stream(
                messages,
                tools=tools,
                system=system,
                temperature=temperature,
                max_tokens=max_tokens,
                provider_options=provider_options,
            )
        )
        if not response.model and self.model is not None:
            response = replace(response, model=self.model)
        return response

    async def stream_collect_with(self, request: ChatRequest) -> ChatResponse:
        """Stream a fully-built ChatRequest and assemble the full ChatResponse."""
        from motosan_ai._stream_collect import collect_stream

        model_hint = request.model or self.model or ""
        response = await collect_stream(self.stream_with(request))
        if not response.model:
            response = replace(response, model=model_hint)
        return response
