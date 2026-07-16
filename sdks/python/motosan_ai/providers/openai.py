from __future__ import annotations

import json
from collections.abc import AsyncIterator
from typing import Any

import httpx

from motosan_ai.error import AuthError, NetworkError, ProviderError, RateLimitError, StreamError
from motosan_ai.provider_base import ProviderCapabilities
from motosan_ai.retry import parse_retry_after_header
from motosan_ai.types import (
    ChatRequest,
    ChatResponse,
    ImageBlock,
    ImageSourceBase64,
    ImageSourceUrl,
    Message,
    Role,
    StopReason,
    StreamEvent,
    TextBlock,
    ToolCall,
    Usage,
)

_DEFAULT_BASE_URL = "https://api.openai.com"

_FINISH_REASON_TO_STOP = {
    "stop": StopReason.stop,
    "length": StopReason.max_tokens,
    "tool_calls": StopReason.tool_use,
}


def _http_error_kwargs(status: int, headers: httpx.Headers | None) -> dict[str, Any]:
    retry_after = None
    request_id = None
    if headers is not None:
        retry_after = parse_retry_after_header(headers.get("retry-after"))
        request_id = headers.get("request-id") or headers.get("x-request-id")
    return {"status_code": status, "retry_after": retry_after, "request_id": request_id}


class OpenAIProvider:
    capabilities: ProviderCapabilities = ProviderCapabilities.with_image()

    def __init__(self, api_key: str, model: str | None = None, base_url: str | None = None) -> None:
        self.api_key = api_key
        self.model = model or "gpt-4o"
        self.base_url = (base_url or _DEFAULT_BASE_URL).rstrip("/")
        self._http = httpx.AsyncClient(timeout=120.0)

    def _endpoint(self) -> str:
        return f"{self.base_url}/v1/chat/completions"

    def _headers(self) -> dict[str, str]:
        return {
            "authorization": f"Bearer {self.api_key}",
            "content-type": "application/json",
        }

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
                if message.content_blocks:
                    blocks: list[dict[str, Any]] = []
                    for block in message.content_blocks:
                        if isinstance(block, TextBlock):
                            blocks.append({"type": "text", "text": block.text})
                        elif isinstance(block, ImageBlock):
                            src = block.source
                            if isinstance(src, ImageSourceBase64):
                                blocks.append(
                                    {
                                        "type": "image_url",
                                        "image_url": {
                                            "url": f"data:{src.media_type};base64,{src.data}"
                                        },
                                    }
                                )
                            elif isinstance(src, ImageSourceUrl):
                                blocks.append({"type": "image_url", "image_url": {"url": src.url}})
                    outgoing.append({"role": "user", "content": blocks})
                else:
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

    def _build_body(self, request: ChatRequest, *, stream: bool = False) -> dict[str, Any]:
        body: dict[str, Any] = {
            "model": request.model or self.model,
            "messages": self._serialize_messages(request.messages, request.system),
        }
        if stream:
            body["stream"] = True
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
            if request.tool_choice is not None:
                choice = request.tool_choice
                if choice.type == "auto":
                    body["tool_choice"] = "auto"
                elif choice.type == "required":
                    body["tool_choice"] = "required"
                elif choice.type == "none":
                    body.pop("tools", None)
                elif choice.type == "tool":
                    body["tool_choice"] = {
                        "type": "function",
                        "function": {"name": choice.name},
                    }
        if request.provider_options:
            body.update(request.provider_options)
        return body

    @staticmethod
    def _map_http_error(
        status: int, message: str, headers: httpx.Headers | None = None
    ) -> Exception:
        metadata = _http_error_kwargs(status, headers)
        if status == 401:
            return AuthError(message, **metadata)
        if status == 429:
            return RateLimitError(message, **metadata)
        return ProviderError(message, **metadata)

    @staticmethod
    def _response_error_message(status: int, headers: httpx.Headers, text: str) -> str:
        message = f"HTTP {status}: {text}"
        retry_after = headers.get("retry-after")
        if retry_after:
            message = f"Retry-After: {retry_after}\n{message}"
        return message

    async def chat(self, request: ChatRequest) -> ChatResponse:
        body = self._build_body(request)
        try:
            resp = await self._http.post(self._endpoint(), headers=self._headers(), json=body)
        except httpx.HTTPError as exc:
            raise NetworkError(str(exc)) from exc

        if not resp.is_success:
            message = self._response_error_message(resp.status_code, resp.headers, resp.text)
            raise self._map_http_error(resp.status_code, message, resp.headers)

        payload = resp.json()
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
            tool_calls.append(
                ToolCall(id=tc.get("id", ""), name=fn.get("name", ""), input=parsed_input)
            )

        finish_reason = choice.get("finish_reason")
        stop_reason = _FINISH_REASON_TO_STOP.get(finish_reason, StopReason.other)

        usage = payload.get("usage") or {}
        return ChatResponse(
            content=content,
            tool_calls=tool_calls,
            model=payload.get("model", self.model),
            usage=Usage(int(usage.get("prompt_tokens", 0)), int(usage.get("completion_tokens", 0))),
            stop_reason=stop_reason,
        )

    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        body = self._build_body(request, stream=True)
        try:
            resp = await self._http.send(
                self._http.build_request(
                    "POST", self._endpoint(), headers=self._headers(), json=body
                ),
                stream=True,
            )
        except httpx.HTTPError as exc:
            raise NetworkError(str(exc)) from exc

        try:
            if not resp.is_success:
                error_body = await resp.aread()
                message = self._response_error_message(
                    resp.status_code, resp.headers, error_body.decode()
                )
                raise self._map_http_error(resp.status_code, message, resp.headers)

            # Per-index tool-call tracking (mirrors TS providers/openai.ts):
            # index -> (id, name); only one tool call is open at a time.
            tool_buffer: dict[int, tuple[str, str]] = {}
            open_tool_index: int | None = None

            async for line in resp.aiter_lines():
                if not line.startswith("data: "):
                    continue
                data = line[6:].strip()
                if not data or data == "[DONE]":
                    if data == "[DONE]":
                        if open_tool_index is not None:
                            yield StreamEvent(
                                content="",
                                done=False,
                                tool_call_id=tool_buffer[open_tool_index][0],
                                event_type="tool_call_end",
                            )
                            open_tool_index = None
                        yield StreamEvent(content="", done=True)
                        return
                    continue
                try:
                    payload = json.loads(data)
                except json.JSONDecodeError as exc:
                    raise StreamError(f"malformed SSE chunk: {exc}") from exc

                for choice in payload.get("choices") or []:
                    delta = choice.get("delta") or {}
                    text = delta.get("content") or ""
                    if text:
                        yield StreamEvent(content=text, done=False)

                    for tc in delta.get("tool_calls") or []:
                        tc_index = tc.get("index")
                        if tc_index is None:
                            continue
                        fn = tc.get("function") or {}
                        tc_id = tc.get("id")
                        tc_name = fn.get("name")
                        tc_args = fn.get("arguments")
                        if tc_id and tc_name:
                            # First fragment for this index: close any open
                            # tool call from a different index, then open this one.
                            if open_tool_index is not None and open_tool_index != tc_index:
                                yield StreamEvent(
                                    content="",
                                    done=False,
                                    tool_call_id=tool_buffer[open_tool_index][0],
                                    event_type="tool_call_end",
                                )
                            tool_buffer[tc_index] = (tc_id, tc_name)
                            open_tool_index = tc_index
                            yield StreamEvent(
                                content="",
                                done=False,
                                tool_call_id=tc_id,
                                tool_call_name=tc_name,
                                event_type="tool_call_start",
                            )
                        if tc_args and tc_index in tool_buffer:
                            yield StreamEvent(
                                content="",
                                done=False,
                                tool_call_id=tool_buffer[tc_index][0],
                                tool_call_args_delta=tc_args,
                                event_type="tool_call_args",
                            )

                    finish_reason = choice.get("finish_reason")
                    if finish_reason:
                        if finish_reason == "tool_calls" and open_tool_index is not None:
                            yield StreamEvent(
                                content="",
                                done=False,
                                tool_call_id=tool_buffer[open_tool_index][0],
                                event_type="tool_call_end",
                            )
                            open_tool_index = None
                        yield StreamEvent(
                            content="",
                            done=True,
                            stop_reason=_FINISH_REASON_TO_STOP.get(finish_reason, StopReason.other),
                        )
                        return
        except StreamError:
            raise
        except (AuthError, RateLimitError, ProviderError, NetworkError):
            raise
        except httpx.HTTPError as exc:
            raise StreamError(f"stream transport error: {exc}") from exc
        finally:
            await resp.aclose()
