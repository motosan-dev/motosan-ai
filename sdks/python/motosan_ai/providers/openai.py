from __future__ import annotations

import json
from typing import Any, AsyncIterator

from motosan_ai.error import AuthError, ConfigError, NetworkError, ProviderError, RateLimitError
from motosan_ai.types import ChatRequest, ChatResponse, Message, Role, StopReason, StreamEvent, ToolCall, Usage


class OpenAIProvider:
    def __init__(self, api_key: str, model: str | None = None) -> None:
        self.api_key = api_key
        self.model = model or "gpt-4o"
        try:
            from openai import AsyncOpenAI  # type: ignore
        except Exception as exc:  # pragma: no cover
            raise ConfigError("openai package is required for OpenAIProvider") from exc
        self._client = AsyncOpenAI(api_key=api_key)

    @staticmethod
    def _serialize_messages(messages: list[Message], system: str | None = None) -> list[dict[str, Any]]:
        outgoing: list[dict[str, Any]] = []
        if system:
            outgoing.append({"role": "system", "content": system})
        for message in messages:
            if message.role == Role.system:
                outgoing.append({"role": "system", "content": message.content})
            elif message.role == Role.user:
                outgoing.append({"role": "user", "content": message.content})
            elif message.role == Role.assistant:
                if message.tool_calls:
                    outgoing.append(
                        {
                            "role": "assistant",
                            "content": message.content,
                            "tool_calls": [
                                {
                                    "id": tc.id,
                                    "type": "function",
                                    "function": {
                                        "name": tc.name,
                                        "arguments": json.dumps(tc.input, ensure_ascii=False),
                                    },
                                }
                                for tc in message.tool_calls
                            ],
                        }
                    )
                else:
                    outgoing.append({"role": "assistant", "content": message.content})
            elif message.role == Role.tool and message.tool_call_id:
                outgoing.append(
                    {
                        "role": "tool",
                        "tool_call_id": message.tool_call_id,
                        "content": message.content,
                    }
                )
        return outgoing

    async def chat(self, request: ChatRequest) -> ChatResponse:
        body: dict[str, Any] = {
            "model": request.model or self.model,
            "messages": self._serialize_messages(request.messages, request.system),
        }
        if request.temperature is not None:
            body["temperature"] = request.temperature
        if request.max_tokens is not None:
            body["max_tokens"] = request.max_tokens
        if request.tools:
            body["tools"] = [
                {
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description or "",
                        "parameters": t.input_schema or {"type": "object", "properties": {}},
                    },
                }
                for t in request.tools
            ]
        if request.provider_options:
            body.update(request.provider_options)

        try:
            resp = await self._client.chat.completions.create(**body)
        except Exception as exc:
            name = exc.__class__.__name__.lower()
            if "authentication" in name:
                raise AuthError(str(exc)) from exc
            if "ratelimit" in name or "rate_limit" in name:
                raise RateLimitError(str(exc)) from exc
            raise NetworkError(str(exc)) from exc

        payload = resp if isinstance(resp, dict) else resp.model_dump()
        choice = (payload.get("choices") or [{}])[0]
        message = choice.get("message") or {}
        content = message.get("content") or ""

        tool_calls: list[ToolCall] = []
        for tc in message.get("tool_calls") or []:
            fn = tc.get("function") or {}
            try:
                parsed_input = json.loads(fn.get("arguments") or "{}")
            except json.JSONDecodeError:
                parsed_input = {}
            tool_calls.append(ToolCall(id=tc.get("id", ""), name=fn.get("name", ""), input=parsed_input))

        finish_reason = choice.get("finish_reason")
        stop_reason = {
            "stop": StopReason.stop,
            "length": StopReason.max_tokens,
            "tool_calls": StopReason.tool_use,
        }.get(finish_reason, StopReason.other)

        usage = payload.get("usage") or {}
        return ChatResponse(
            content=content,
            tool_calls=tool_calls,
            model=payload.get("model", self.model),
            usage=Usage(int(usage.get("prompt_tokens", 0)), int(usage.get("completion_tokens", 0))),
            stop_reason=stop_reason,
        )

    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        body: dict[str, Any] = {
            "model": request.model or self.model,
            "messages": self._serialize_messages(request.messages, request.system),
            "stream": True,
        }
        if request.provider_options:
            body.update(request.provider_options)

        try:
            stream = await self._client.chat.completions.create(**body)
        except Exception as exc:
            raise ProviderError(str(exc)) from exc

        async for event in stream:
            payload = event if isinstance(event, dict) else event.model_dump()
            for choice in payload.get("choices") or []:
                delta = choice.get("delta") or {}
                text = delta.get("content") or ""
                if text:
                    yield StreamEvent(content=text, done=False)
                if choice.get("finish_reason"):
                    yield StreamEvent(content="", done=True)
                    return
