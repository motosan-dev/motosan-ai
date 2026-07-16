from __future__ import annotations

import json
import time
from collections.abc import AsyncIterator
from dataclasses import dataclass, field
from itertools import count
from typing import Any

import httpx

from motosan_ai._stream_collect import collect_stream
from motosan_ai.error import AuthError, NetworkError, ProviderError, RateLimitError, StreamError
from motosan_ai.provider_base import BaseProvider, ProviderCapabilities
from motosan_ai.providers.gemini import build_gemini_body
from motosan_ai.retry import parse_retry_after_header
from motosan_ai.types import ChatRequest, ChatResponse, StopReason, StreamEvent, Usage

_DEFAULT_BASE_URL = "https://cloudcode-pa.googleapis.com"
_DEFAULT_MODEL = "gemini-2.5-flash"
_USER_AGENT = "google-cloud-sdk vscode_cloudshelleditor/0.1"
_X_GOOG_API_CLIENT = "gl-node/22.17.0"
_CLIENT_METADATA = (
    '{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}'
)
_REQUEST_COUNTER = count()
_TOOL_CALL_COUNTER = count()


def _http_error_kwargs(status: int, headers: httpx.Headers | None) -> dict[str, Any]:
    retry_after = None
    request_id = None
    if headers is not None:
        retry_after = parse_retry_after_header(headers.get("retry-after"))
        request_id = headers.get("request-id") or headers.get("x-request-id")
    return {"status_code": status, "retry_after": retry_after, "request_id": request_id}


def _gen_request_id() -> str:
    ts_ms = int(time.time() * 1000)
    return f"motosan-{ts_ms}-{next(_REQUEST_COUNTER):09d}"


def _gen_tool_call_id(name: str) -> str:
    ts_ms = int(time.time() * 1000)
    return f"{name}_{ts_ms}_{next(_TOOL_CALL_COUNTER)}"


@dataclass
class _CodeAssistAdapterState:
    seen_tool_ids: set[str] = field(default_factory=set)


_FINISH_REASON_MAP = {"MAX_TOKENS": StopReason.max_tokens}


def _parse_sse_event(data: str, state: _CodeAssistAdapterState) -> list[StreamEvent]:
    s = data.strip()
    if not s or s == "[DONE]":
        return []
    try:
        chunk = json.loads(s)
    except json.JSONDecodeError as exc:
        raise StreamError(f"malformed SSE chunk: {exc}") from exc

    response_data = chunk.get("response")
    if not isinstance(response_data, dict):
        return []

    candidates = response_data.get("candidates") or []
    if not candidates:
        return []
    candidate = candidates[0]
    if not isinstance(candidate, dict):
        return []

    parts = (candidate.get("content") or {}).get("parts") or []
    finish_reason = candidate.get("finishReason")
    out: list[StreamEvent] = []
    has_tool_calls = False

    for part in parts:
        if not isinstance(part, dict):
            continue
        text = part.get("text")
        if isinstance(text, str) and text:
            out.append(StreamEvent(content=text, done=False))
        fc = part.get("functionCall")
        if isinstance(fc, dict):
            has_tool_calls = True
            name = fc.get("name") or ""
            args = fc.get("args") or {}
            provided_id = fc.get("id")
            if (
                isinstance(provided_id, str)
                and provided_id
                and provided_id not in state.seen_tool_ids
            ):
                tool_id = provided_id
            else:
                tool_id = _gen_tool_call_id(name)
            state.seen_tool_ids.add(tool_id)
            out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="tool_call_start",
                    tool_call_id=tool_id,
                    tool_call_name=name,
                )
            )
            out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="tool_call_args",
                    tool_call_id=tool_id,
                    tool_call_args_delta=json.dumps(args),
                )
            )
            out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="tool_call_end",
                    tool_call_id=tool_id,
                )
            )

    usage_meta = response_data.get("usageMetadata")
    if isinstance(usage_meta, dict):
        prompt = int(usage_meta.get("promptTokenCount") or 0)
        cached = int(usage_meta.get("cachedContentTokenCount") or 0)
        output = int(usage_meta.get("candidatesTokenCount") or 0)
        out.append(
            StreamEvent(
                content="",
                done=False,
                event_type="usage",
                usage=Usage(
                    input_tokens=max(prompt - cached, 0),
                    output_tokens=output,
                    cache_read_input_tokens=cached if cached > 0 else None,
                ),
            )
        )

    if finish_reason:
        if has_tool_calls:
            stop = StopReason.tool_use
        elif finish_reason == "STOP":
            stop = StopReason.end_turn
        else:
            stop = _FINISH_REASON_MAP.get(finish_reason, StopReason.other)
        out.append(StreamEvent(content="", done=True, stop_reason=stop))

    return out


class GeminiCodeAssistProvider(BaseProvider):
    capabilities: ProviderCapabilities = ProviderCapabilities.with_image()

    def __init__(
        self,
        access_token: str,
        project_id: str,
        model: str | None = None,
        base_url: str | None = None,
    ) -> None:
        self.access_token = access_token
        self.project_id = project_id
        self.model = model or _DEFAULT_MODEL
        self.base_url = (base_url or _DEFAULT_BASE_URL).rstrip("/")
        self._http = httpx.AsyncClient(timeout=120.0)

    def _stream_url(self) -> str:
        return f"{self.base_url}/v1internal:streamGenerateContent?alt=sse"

    def _headers(self) -> dict[str, str]:
        return {
            "authorization": f"Bearer {self.access_token}",
            "user-agent": _USER_AGENT,
            "x-goog-api-client": _X_GOOG_API_CLIENT,
            "client-metadata": _CLIENT_METADATA,
            "content-type": "application/json",
        }

    def _build_envelope(self, request: ChatRequest) -> dict[str, Any]:
        inner = build_gemini_body(request)
        return {
            "project": self.project_id,
            "model": request.model or self.model,
            "request": inner,
            "userAgent": "motosan-ai",
            "requestId": _gen_request_id(),
        }

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

    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        self.validate_request(request)
        body = self._build_envelope(request)
        try:
            resp = await self._http.send(
                self._http.build_request(
                    "POST", self._stream_url(), headers=self._headers(), json=body
                ),
                stream=True,
            )
        except httpx.HTTPError as exc:
            raise NetworkError(str(exc)) from exc

        try:
            if not resp.is_success:
                error_body = await resp.aread()
                raise self._map_http_error(resp.status_code, error_body.decode(), resp.headers)

            state = _CodeAssistAdapterState()
            try:
                async for line in resp.aiter_lines():
                    if not line.startswith("data: "):
                        continue
                    data = line[len("data: ") :]
                    for event in _parse_sse_event(data, state):
                        yield event
                        if event.done:
                            return
            except (StreamError, AuthError, RateLimitError, ProviderError, NetworkError):
                raise
            except httpx.HTTPError as exc:
                raise StreamError(f"stream transport error: {exc}") from exc
        finally:
            await resp.aclose()

    async def chat(self, request: ChatRequest) -> ChatResponse:
        response = await collect_stream(self.stream(request))
        return ChatResponse(
            content=response.content,
            tool_calls=response.tool_calls,
            model=request.model or self.model,
            usage=response.usage,
            stop_reason=response.stop_reason,
            thinking=response.thinking,
        )
