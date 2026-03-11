from __future__ import annotations

from typing import Any, AsyncIterator

from motosan_ai.error import AuthError, ConfigError, NetworkError, ProviderError, RateLimitError
from motosan_ai.types import ChatRequest, ChatResponse, Message, Role, StopReason, StreamEvent, ToolCall, Usage


class AnthropicProvider:
    def __init__(self, api_key: str, model: str | None = None) -> None:
        self.api_key = api_key
        self.model = model or "claude-sonnet-4-5"
        try:
            from anthropic import AsyncAnthropic  # type: ignore
        except Exception as exc:  # pragma: no cover
            raise ConfigError("anthropic package is required for AnthropicProvider") from exc
        self._client = AsyncAnthropic(api_key=api_key)

    @staticmethod
    def _serialize_messages(messages: list[Message]) -> tuple[list[dict[str, Any]], str | None]:
        outgoing: list[dict[str, Any]] = []
        system_parts: list[str] = []

        for message in messages:
            if message.role == Role.system:
                if message.content.strip():
                    system_parts.append(message.content)
                continue

            if message.role == Role.user:
                outgoing.append({"role": "user", "content": message.content})
                continue

            if message.role == Role.assistant:
                if message.tool_calls:
                    blocks: list[dict[str, Any]] = []
                    if message.content:
                        blocks.append({"type": "text", "text": message.content})
                    for tc in message.tool_calls:
                        blocks.append(
                            {
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.name,
                                "input": tc.input,
                            }
                        )
                    outgoing.append({"role": "assistant", "content": blocks})
                else:
                    outgoing.append({"role": "assistant", "content": message.content})
                continue

            if message.role == Role.tool and message.tool_call_id:
                outgoing.append(
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "tool_result",
                                "tool_use_id": message.tool_call_id,
                                "content": message.content,
                            }
                        ],
                    }
                )

        system = "\n\n".join(system_parts) if system_parts else None
        return outgoing, system

    async def chat(self, request: ChatRequest) -> ChatResponse:
        messages, extracted_system = self._serialize_messages(request.messages)
        system = request.system or extracted_system
        body: dict[str, Any] = {
            "model": request.model or self.model,
            "messages": messages,
            "max_tokens": request.max_tokens or 1024,
        }
        if system:
            body["system"] = system
        if request.temperature is not None:
            body["temperature"] = request.temperature
        if request.tools:
            body["tools"] = [
                {
                    "name": t.name,
                    "description": t.description or "",
                    "input_schema": t.input_schema or {"type": "object", "properties": {}},
                }
                for t in request.tools
            ]
        if request.provider_options:
            body.update(request.provider_options)

        try:
            resp = await self._client.messages.create(**body)
        except Exception as exc:
            name = exc.__class__.__name__.lower()
            if "authentication" in name:
                raise AuthError(str(exc)) from exc
            if "ratelimit" in name or "rate_limit" in name:
                raise RateLimitError(str(exc)) from exc
            raise NetworkError(str(exc)) from exc

        payload = resp if isinstance(resp, dict) else resp.model_dump()
        content_blocks = payload.get("content", [])
        text = "".join(block.get("text", "") for block in content_blocks if block.get("type") == "text")
        tool_calls = [
            ToolCall(
                id=block.get("id", ""),
                name=block.get("name", ""),
                input=block.get("input", {}) or {},
            )
            for block in content_blocks
            if block.get("type") == "tool_use"
        ]
        stop_reason_map = {
            "end_turn": StopReason.end_turn,
            "max_tokens": StopReason.max_tokens,
            "tool_use": StopReason.tool_use,
            "stop": StopReason.stop,
        }
        usage = payload.get("usage", {})
        return ChatResponse(
            content=text,
            tool_calls=tool_calls,
            model=payload.get("model", self.model),
            usage=Usage(int(usage.get("input_tokens", 0)), int(usage.get("output_tokens", 0))),
            stop_reason=stop_reason_map.get(payload.get("stop_reason"), StopReason.other),
        )

    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        messages, extracted_system = self._serialize_messages(request.messages)
        body: dict[str, Any] = {
            "model": request.model or self.model,
            "messages": messages,
            "stream": True,
            "max_tokens": request.max_tokens or 1024,
        }
        system = request.system or extracted_system
        if system:
            body["system"] = system
        if request.provider_options:
            body.update(request.provider_options)

        try:
            events = await self._client.messages.create(**body)
        except Exception as exc:
            raise ProviderError(str(exc)) from exc

        async for event in events:
            payload = event if isinstance(event, dict) else event.model_dump()
            if payload.get("type") == "content_block_delta":
                text = (payload.get("delta") or {}).get("text", "")
                if text:
                    yield StreamEvent(content=text, done=False)
            elif payload.get("type") == "message_stop":
                yield StreamEvent(content="", done=True)
                return
