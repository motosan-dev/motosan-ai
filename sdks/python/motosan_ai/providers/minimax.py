from __future__ import annotations

import json
from collections.abc import AsyncIterator
from typing import Any

import httpx

from motosan_ai.error import (
    AuthError,
    InvalidRequestError,
    NetworkError,
    ProviderError,
    RateLimitError,
)
from motosan_ai.types import (
    ChatRequest,
    ChatResponse,
    Message,
    Role,
    StopReason,
    StreamEvent,
    ToolCall,
    Usage,
)


class MinimaxProvider:
    def __init__(self, api_key: str, model: str | None = None, base_url: str | None = None) -> None:
        self.api_key = api_key
        self.model = model or "MiniMax-Text-01"
        self.base_url = (base_url or "https://api.minimax.chat").rstrip("/")
        self._client = httpx.AsyncClient(timeout=30)

    def _endpoint(self) -> str:
        return f"{self.base_url}/v1/text/chatcompletion_v2"

    @staticmethod
    def _serialize_messages(
        messages: list[Message], system: str | None = None
    ) -> list[dict[str, Any]]:
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

    @staticmethod
    def _raise_for_status(status_code: int, message: str) -> None:
        if status_code == 401:
            raise AuthError(message)
        if status_code == 429:
            raise RateLimitError(message)
        if status_code == 400:
            raise InvalidRequestError(message)
        raise ProviderError(message)

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
            response = await self._client.post(
                self._endpoint(),
                headers={"Authorization": f"Bearer {self.api_key}"},
                json=body,
            )
        except Exception as exc:
            raise NetworkError(str(exc)) from exc

        payload = response.json() if response.content else {}
        if response.status_code >= 400:
            message = (
                (payload.get("error") or {}).get("message")
                or response.text
                or "minimax request failed"
            )
            self._raise_for_status(response.status_code, message)

        choice = (payload.get("choices") or [{}])[0]
        message_obj = choice.get("message") or {}
        content = message_obj.get("content") or ""
        tool_calls: list[ToolCall] = []
        for tc in message_obj.get("tool_calls") or []:
            fn = tc.get("function") or {}
            try:
                parsed_input = json.loads(fn.get("arguments") or "{}")
            except json.JSONDecodeError:
                parsed_input = {}
            tool_calls.append(
                ToolCall(id=tc.get("id", ""), name=fn.get("name", ""), input=parsed_input)
            )

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
            async with self._client.stream(
                "POST",
                self._endpoint(),
                headers={"Authorization": f"Bearer {self.api_key}"},
                json=body,
            ) as response:
                if response.status_code >= 400:
                    text = await response.aread()
                    message = text.decode("utf-8", errors="ignore") or "minimax stream failed"
                    self._raise_for_status(response.status_code, message)

                async for line in response.aiter_lines():
                    if not line.startswith("data:"):
                        continue
                    data = line[5:].strip()
                    if data == "[DONE]":
                        yield StreamEvent(content="", done=True)
                        return
                    try:
                        payload = json.loads(data)
                    except json.JSONDecodeError:
                        continue
                    choice = (payload.get("choices") or [{}])[0]
                    delta = choice.get("delta") or {}
                    text = delta.get("content") or ""
                    if text:
                        yield StreamEvent(content=text, done=False)

                    for tc in delta.get("tool_calls") or []:
                        fn = tc.get("function") or {}
                        tc_id = tc.get("id")
                        tc_name = fn.get("name")
                        tc_args = fn.get("arguments")
                        if tc_id and tc_name:
                            yield StreamEvent(
                                content="",
                                done=False,
                                tool_call_id=tc_id,
                                tool_call_name=tc_name,
                                event_type="tool_call_start",
                            )
                        if tc_args:
                            yield StreamEvent(
                                content="",
                                done=False,
                                tool_call_args_delta=tc_args,
                                event_type="tool_call_args",
                            )

                    finish_reason = choice.get("finish_reason")
                    if finish_reason == "tool_calls":
                        yield StreamEvent(content="", done=False, event_type="tool_call_end")
        except Exception as exc:
            if isinstance(exc, (AuthError, RateLimitError, InvalidRequestError, ProviderError)):
                raise
            raise NetworkError(str(exc)) from exc
