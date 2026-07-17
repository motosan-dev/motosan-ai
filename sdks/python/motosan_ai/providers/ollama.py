from __future__ import annotations

import json
import uuid
from collections.abc import AsyncIterator
from typing import Any

import httpx

from motosan_ai.error import (
    IncompleteStreamError,
    NetworkError,
    ProviderError,
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


class OllamaProvider:
    capabilities: ProviderCapabilities = ProviderCapabilities.text_only()

    def __init__(
        self,
        model: str = "llama3.2",
        base_url: str = "http://localhost:11434",
        think: bool = False,
        keep_alive: str | None = None,
        num_ctx: int | None = None,
        *,
        connect_timeout: float = 10.0,
        read_idle_timeout: float = 120.0,
    ) -> None:
        self.model = model
        self.base_url = base_url.rstrip("/")
        self.think = think
        self.keep_alive = keep_alive
        self.num_ctx = num_ctx
        self._read_idle_timeout = read_idle_timeout
        self._http = httpx.AsyncClient(
            timeout=httpx.Timeout(
                connect=connect_timeout,
                read=read_idle_timeout,
                write=read_idle_timeout,
                pool=connect_timeout,
            )
        )

    async def aclose(self) -> None:
        """Close the underlying HTTP connection pool."""
        await self._http.aclose()

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
                msg: dict[str, Any] = {"role": "assistant", "content": message.content}
                if message.tool_calls:
                    msg["tool_calls"] = [
                        {
                            "function": {
                                "name": tc.name,
                                "arguments": tc.input,
                            },
                        }
                        for tc in message.tool_calls
                    ]
                outgoing.append(msg)
            elif message.role == Role.tool and message.tool_call_id:
                outgoing.append({"role": "tool", "content": message.content})
        return outgoing

    def _build_body(self, request: ChatRequest, *, stream: bool = False) -> dict[str, Any]:
        body: dict[str, Any] = {
            "model": request.model or self.model,
            "messages": self._serialize_messages(request.messages, request.system),
            "stream": stream,
        }
        if self.think:
            body["think"] = True
        if self.keep_alive is not None:
            body["keep_alive"] = self.keep_alive
        options: dict[str, Any] = {}
        if request.temperature is not None:
            options["temperature"] = request.temperature
        if self.num_ctx is not None:
            options["num_ctx"] = self.num_ctx
        if options:
            body["options"] = options
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
        return body

    async def chat(self, request: ChatRequest) -> ChatResponse:
        body = self._build_body(request, stream=False)
        try:
            resp = await self._http.post(f"{self.base_url}/api/chat", json=body)
        except Exception as exc:
            raise NetworkError(str(exc)) from exc

        if resp.status_code >= 400:
            raise ProviderError(
                f"Ollama error {resp.status_code}: {resp.text}",
                **_http_error_kwargs(resp.status_code, resp.headers),
            )

        data = resp.json()
        msg = data.get("message") or {}
        content = msg.get("content") or ""

        tool_calls: list[ToolCall] = []
        for tc in msg.get("tool_calls") or []:
            fn = tc.get("function") or {}
            raw_args = fn.get("arguments", {})
            if isinstance(raw_args, str):
                try:
                    parsed_input = json.loads(raw_args)
                except json.JSONDecodeError:
                    parsed_input = {}
            else:
                parsed_input = raw_args
            tool_calls.append(
                ToolCall(
                    id=str(uuid.uuid4()),
                    name=fn.get("name", ""),
                    input=parsed_input,
                )
            )

        stop_reason = StopReason.tool_use if tool_calls else StopReason.stop

        usage_data = data.get("usage") or {}
        prompt_tokens = int(usage_data.get("prompt_tokens", data.get("prompt_eval_count", 0)))
        completion_tokens = int(usage_data.get("completion_tokens", data.get("eval_count", 0)))

        return ChatResponse(
            content=content,
            tool_calls=tool_calls,
            model=data.get("model", self.model),
            usage=Usage(prompt_tokens, completion_tokens),
            stop_reason=stop_reason,
        )

    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        body = self._build_body(request, stream=True)
        # Q2: an httpx fault BEFORE the first yield is connection-time
        # (NetworkError, retryable); after the first yield it is mid-stream
        # (StreamError, non-retryable). A malformed NDJSON line -> StreamError.
        yielded = False
        try:
            async with self._http.stream("POST", f"{self.base_url}/api/chat", json=body) as resp:
                if resp.status_code >= 400:
                    text = await resp.aread()
                    raise ProviderError(
                        f"Ollama error {resp.status_code}: {text.decode('utf-8', errors='ignore')}",
                        **_http_error_kwargs(resp.status_code, resp.headers),
                    )

                async for line in resp.aiter_lines():
                    if not line:
                        continue
                    try:
                        chunk = json.loads(line)
                    except json.JSONDecodeError as exc:
                        raise StreamError(f"malformed NDJSON chunk: {exc}") from exc
                    if chunk.get("done"):
                        yielded = True
                        yield StreamEvent(content="", done=True)
                        return
                    msg = chunk.get("message", {})
                    content = msg.get("content", "")
                    thinking = msg.get("thinking", "")
                    text = content or thinking
                    if text:
                        yielded = True
                        yield StreamEvent(content=text, done=False)

                    for tc in msg.get("tool_calls") or []:
                        fn = tc.get("function") or {}
                        raw_args = fn.get("arguments", {})
                        if isinstance(raw_args, str):
                            args_str = raw_args
                        else:
                            args_str = json.dumps(raw_args, ensure_ascii=False)
                        tc_id = str(uuid.uuid4())
                        yielded = True
                        yield StreamEvent(
                            content="",
                            done=False,
                            tool_call_id=tc_id,
                            tool_call_name=fn.get("name", ""),
                            event_type="tool_call_start",
                        )
                        yield StreamEvent(
                            content="",
                            done=False,
                            tool_call_id=tc_id,
                            tool_call_args_delta=args_str,
                            event_type="tool_call_args",
                        )
                        yield StreamEvent(
                            content="",
                            done=False,
                            tool_call_id=tc_id,
                            event_type="tool_call_end",
                        )

                raise IncompleteStreamError(
                    "incomplete stream: ollama ended without a terminal event"
                )
        except (ProviderError, StreamError):
            raise
        except httpx.ReadTimeout as exc:
            raise StreamReadTimeoutError(
                f"stream read timed out after {self._read_idle_timeout}s"
            ) from exc
        except httpx.HTTPError as exc:
            if yielded:
                raise StreamError(f"stream transport error: {exc}") from exc
            raise NetworkError(str(exc)) from exc
