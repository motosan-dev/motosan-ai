from __future__ import annotations

import json
from collections.abc import AsyncIterator
from typing import Any

import httpx

from motosan_ai.error import (
    AuthError,
    IncompleteStreamError,
    InvalidRequestError,
    NetworkError,
    ProviderError,
    RateLimitError,
    StreamError,
    StreamReadTimeoutError,
)
from motosan_ai.provider_base import ProviderCapabilities
from motosan_ai.retry import parse_retry_after_header
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


def _http_error_kwargs(status: int, headers: httpx.Headers | None) -> dict[str, Any]:
    retry_after = None
    request_id = None
    if headers is not None:
        retry_after = parse_retry_after_header(headers.get("retry-after"))
        request_id = headers.get("request-id") or headers.get("x-request-id")
    return {"status_code": status, "retry_after": retry_after, "request_id": request_id}


class MinimaxProvider:
    capabilities: ProviderCapabilities = ProviderCapabilities.text_only()

    def __init__(
        self,
        api_key: str,
        model: str | None = None,
        base_url: str | None = None,
        *,
        connect_timeout: float = 10.0,
        read_idle_timeout: float = 120.0,
    ) -> None:
        self.api_key = api_key
        self.model = model or "MiniMax-Text-01"
        self.base_url = (base_url or "https://api.minimax.chat").rstrip("/")
        self._read_idle_timeout = read_idle_timeout
        self._client = httpx.AsyncClient(
            timeout=httpx.Timeout(
                connect=connect_timeout,
                read=read_idle_timeout,
                write=read_idle_timeout,
                pool=connect_timeout,
            )
        )

    async def aclose(self) -> None:
        """Close the underlying HTTP connection pool."""
        await self._client.aclose()

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
    def _apply_tool_choice(body: dict[str, Any], request: ChatRequest) -> None:
        if request.tool_choice is None:
            return
        choice = request.tool_choice
        if choice.type == "auto":
            body["tool_choice"] = "auto"
        elif choice.type == "required":
            body["tool_choice"] = "required"
        elif choice.type == "none":
            body.pop("tools", None)
        elif choice.type == "tool":
            body["tool_choice"] = {"type": "function", "function": {"name": choice.name}}

    @staticmethod
    def _raise_for_status(
        status_code: int, message: str, headers: httpx.Headers | None = None
    ) -> None:
        metadata = _http_error_kwargs(status_code, headers)
        if status_code == 401:
            raise AuthError(message, **metadata)
        if status_code == 429:
            raise RateLimitError(message, **metadata)
        if status_code == 400:
            raise InvalidRequestError(message, **metadata)
        raise ProviderError(message, **metadata)

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
        self._apply_tool_choice(body, request)
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

        if response.status_code >= 400:
            try:
                error_payload = response.json() if response.content else {}
                detail = str((error_payload.get("error") or {}).get("message") or "")
            except (json.JSONDecodeError, TypeError, AttributeError):
                detail = ""
            if not detail:
                detail = response.text or "minimax request failed"
            message = f"HTTP {response.status_code}: {detail}"
            retry_after = response.headers.get("retry-after")
            if retry_after:
                message = f"Retry-After: {retry_after}\n{message}"
            self._raise_for_status(response.status_code, message, response.headers)

        payload = response.json() if response.content else {}

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
        self._apply_tool_choice(body, request)
        if request.provider_options:
            body.update(request.provider_options)

        # Q2: an httpx fault BEFORE the first yield is a connection-time fault
        # (NetworkError, retryable). The SAME fault AFTER the first yield, or a
        # malformed frame, is mid-stream -> StreamError (non-retryable).
        yielded = False
        try:
            async with self._client.stream(
                "POST",
                self._endpoint(),
                headers={"Authorization": f"Bearer {self.api_key}"},
                json=body,
            ) as response:
                if response.status_code >= 400:
                    text = await response.aread()
                    detail = text.decode("utf-8", errors="ignore") or "minimax stream failed"
                    message = f"HTTP {response.status_code}: {detail}"
                    retry_after = response.headers.get("retry-after")
                    if retry_after:
                        message = f"Retry-After: {retry_after}\n{message}"
                    self._raise_for_status(response.status_code, message, response.headers)

                # M3 (amended): any finish_reason chunk marks the stream
                # semantically complete (either-suffices terminal rule).
                saw_finish_reason = False

                async for line in response.aiter_lines():
                    if not line.startswith("data:"):
                        continue
                    data = line[5:].strip()
                    if not data or data == "[DONE]":
                        if data == "[DONE]":
                            yielded = True
                            yield StreamEvent(content="", done=True)
                            return
                        continue
                    try:
                        payload = json.loads(data)
                    except json.JSONDecodeError as exc:
                        raise StreamError(f"malformed SSE chunk: {exc}") from exc
                    choice = (payload.get("choices") or [{}])[0]
                    delta = choice.get("delta") or {}
                    text = delta.get("content") or ""
                    if text:
                        yielded = True
                        yield StreamEvent(content=text, done=False)

                    for tc in delta.get("tool_calls") or []:
                        fn = tc.get("function") or {}
                        tc_id = tc.get("id")
                        tc_name = fn.get("name")
                        tc_args = fn.get("arguments")
                        if tc_id and tc_name:
                            yielded = True
                            yield StreamEvent(
                                content="",
                                done=False,
                                tool_call_id=tc_id,
                                tool_call_name=tc_name,
                                event_type="tool_call_start",
                            )
                        if tc_args:
                            yielded = True
                            yield StreamEvent(
                                content="",
                                done=False,
                                tool_call_args_delta=tc_args,
                                event_type="tool_call_args",
                            )

                    finish_reason = choice.get("finish_reason")
                    if finish_reason:
                        saw_finish_reason = True
                        if finish_reason == "tool_calls":
                            yielded = True
                            yield StreamEvent(content="", done=False, event_type="tool_call_end")

                if saw_finish_reason:
                    # Semantic terminal seen: emit the same no-stop_reason done
                    # event as the [DONE] branch; collect_stream fills it.
                    yielded = True
                    yield StreamEvent(content="", done=True)
                    return

                raise IncompleteStreamError(
                    "incomplete stream: minimax ended without a terminal event"
                )
        except (AuthError, RateLimitError, InvalidRequestError, ProviderError, StreamError):
            raise
        except httpx.ReadTimeout as exc:
            raise StreamReadTimeoutError(
                f"stream read timed out after {self._read_idle_timeout}s"
            ) from exc
        except httpx.HTTPError as exc:
            if yielded:
                raise StreamError(f"stream transport error: {exc}") from exc
            raise NetworkError(str(exc)) from exc
