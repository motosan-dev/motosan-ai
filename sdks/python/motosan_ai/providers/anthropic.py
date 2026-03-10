"""Anthropic Claude provider. Requires: pip install motosan-ai[anthropic]"""
from __future__ import annotations
from typing import AsyncIterator, TYPE_CHECKING

from motosan_ai.types import ChatRequest, ChatResponse, Message, Role, StopReason, StreamEvent, Usage
from motosan_ai.error import AuthError, ProviderError, RateLimitError

if TYPE_CHECKING:
    pass

DEFAULT_MODEL = "claude-sonnet-4-5"
MAX_TOKENS_DEFAULT = 1024


def _check_import() -> None:
    try:
        import anthropic  # noqa: F401
    except ImportError:
        raise ImportError(
            "The 'anthropic' package is required for AnthropicProvider.\n"
            "Install it with: pip install 'motosan-ai[anthropic]'"
        )


def _normalize_messages(req: ChatRequest) -> tuple[list[dict], str | None]:
    """Separate system prompt and convert messages to Anthropic format."""
    system = req.system
    messages = []
    for msg in req.messages:
        role = msg.role if isinstance(msg.role, str) else msg.role.value
        if role == Role.SYSTEM.value:
            system = msg.content  # Anthropic: system is a top-level param
        else:
            messages.append({"role": role, "content": msg.content})
    return messages, system


def _parse_stop_reason(reason: str | None) -> StopReason:
    mapping = {
        "end_turn": StopReason.END_TURN,
        "max_tokens": StopReason.MAX_TOKENS,
        "tool_use": StopReason.TOOL_USE,
        "stop_sequence": StopReason.STOP,
    }
    return mapping.get(reason or "", StopReason.OTHER)


class AnthropicProvider:
    def __init__(self, api_key: str):
        _check_import()
        import anthropic
        self._client = anthropic.AsyncAnthropic(api_key=api_key)

    async def chat(self, req: ChatRequest) -> ChatResponse:
        import anthropic
        messages, system = _normalize_messages(req)
        kwargs: dict = dict(
            model=req.model or DEFAULT_MODEL,
            messages=messages,
            max_tokens=req.max_tokens or MAX_TOKENS_DEFAULT,
        )
        if system:
            kwargs["system"] = system
        if req.temperature is not None:
            kwargs["temperature"] = req.temperature
        if req.provider_options:
            kwargs.update(req.provider_options)

        try:
            resp = await self._client.messages.create(**kwargs)
        except anthropic.AuthenticationError as e:
            raise AuthError(str(e)) from e
        except anthropic.RateLimitError as e:
            raise RateLimitError(str(e)) from e
        except anthropic.APIStatusError as e:
            raise ProviderError(str(e), status=e.status_code) from e

        content = "".join(
            block.text for block in resp.content if hasattr(block, "text")
        )
        return ChatResponse(
            content=content,
            model=resp.model,
            usage=Usage(input_tokens=resp.usage.input_tokens, output_tokens=resp.usage.output_tokens),
            stop_reason=_parse_stop_reason(resp.stop_reason),
        )

    async def stream(self, req: ChatRequest) -> AsyncIterator[StreamEvent]:
        import anthropic
        messages, system = _normalize_messages(req)
        kwargs: dict = dict(
            model=req.model or DEFAULT_MODEL,
            messages=messages,
            max_tokens=req.max_tokens or MAX_TOKENS_DEFAULT,
        )
        if system:
            kwargs["system"] = system
        if req.temperature is not None:
            kwargs["temperature"] = req.temperature

        try:
            async with self._client.messages.stream(**kwargs) as stream:
                async for text in stream.text_stream:
                    yield StreamEvent(content=text, done=False)
            yield StreamEvent(content="", done=True)
        except anthropic.AuthenticationError as e:
            raise AuthError(str(e)) from e
        except anthropic.RateLimitError as e:
            raise RateLimitError(str(e)) from e
        except anthropic.APIStatusError as e:
            raise ProviderError(str(e), status=e.status_code) from e
