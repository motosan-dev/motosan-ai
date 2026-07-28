from __future__ import annotations

import json
from collections.abc import AsyncIterator, Awaitable, Callable
from dataclasses import dataclass, field
from typing import Any

import httpx

from motosan_ai._stream_collect import collect_model_stream, collect_stream
from motosan_ai.error import (
    AuthError,
    ConfigError,
    IncompleteStreamError,
    NetworkError,
    ProviderError,
    RateLimitError,
    StreamError,
    StreamReadTimeoutError,
)
from motosan_ai.provider_base import BaseProvider, ProviderCapabilities
from motosan_ai.providers.responses import (
    ModelStreamState,
    build_model_request_body,
    parse_model_sse_event,
)
from motosan_ai.retry import parse_retry_after_header
from motosan_ai.types import (
    ChatRequest,
    ChatResponse,
    ModelChatRequest,
    ModelChatResponse,
    ModelStreamDelta,
    ModelStreamDone,
    Role,
    StopReason,
    StreamEvent,
    StreamEventType,
    Usage,
)

_DEFAULT_BASE_URL = "https://chatgpt.com/backend-api/codex/responses"
_DEFAULT_MODEL = "gpt-5.5"
_ORIGINATOR = "codex_cli_rs"


def _http_error_kwargs(status: int, headers: httpx.Headers | None) -> dict[str, Any]:
    retry_after = None
    request_id = None
    if headers is not None:
        retry_after = parse_retry_after_header(headers.get("retry-after"))
        request_id = headers.get("request-id") or headers.get("x-request-id")
    return {"status_code": status, "retry_after": retry_after, "request_id": request_id}


@dataclass
class _ChatGptCodexAdapterState:
    seen_tool_ids: set[str] = field(default_factory=set)
    item_to_call_id: dict[str, str] = field(default_factory=dict)
    saw_tool_call: bool = False
    error: str | None = None


def _parse_sse_event(data: str, state: _ChatGptCodexAdapterState) -> list[StreamEvent]:
    """Map one decoded Responses SSE ``data`` payload to zero or more StreamEvents.

    Pure (apart from mutating ``state``). Port of Rust
    ``ChatGptCodexStreamAdapter::handle_event``. On a fatal ``error`` /
    ``response.failed`` frame this sets ``state.error`` and returns ``[]``; the
    caller (``stream()``) raises ``StreamError`` after draining.
    """
    s = data.strip()
    if not s or s == "[DONE]":
        return []
    try:
        chunk = json.loads(s)
    except json.JSONDecodeError:
        return []
    if not isinstance(chunk, dict):
        return []

    event_type = chunk.get("type")
    out: list[StreamEvent] = []

    if event_type == "response.output_text.delta":
        delta = chunk.get("delta")
        if isinstance(delta, str) and delta:
            out.append(StreamEvent(content=delta, done=False))

    elif event_type in (
        "response.reasoning_text.delta",
        "response.reasoning_summary_text.delta",
    ):
        delta = chunk.get("delta")
        if isinstance(delta, str) and delta:
            out.append(
                StreamEvent(
                    content=delta,
                    done=False,
                    event_type=StreamEventType.thinking_delta,
                )
            )

    elif event_type == "response.output_item.added":
        item = chunk.get("item")
        if isinstance(item, dict) and item.get("type") == "function_call":
            call_id = item.get("call_id") or ""
            name = item.get("name") or ""
            if call_id:
                state.saw_tool_call = True
                state.seen_tool_ids.add(call_id)
                item_id = item.get("id")
                if isinstance(item_id, str) and item_id:
                    state.item_to_call_id[item_id] = call_id
                out.append(
                    StreamEvent(
                        content="",
                        done=False,
                        event_type="tool_call_start",
                        tool_call_id=call_id,
                        tool_call_name=name,
                    )
                )

    elif event_type == "response.function_call_arguments.delta":
        item_id = chunk.get("item_id") or ""
        delta = chunk.get("delta")
        if item_id and isinstance(delta, str):
            out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="tool_call_args",
                    # Wire fragments are keyed by the item's "fc_..." id; translate
                    # to the "call_..." call_id announced in output_item.added so
                    # consumers can correlate. Unknown ids pass through unchanged.
                    tool_call_id=state.item_to_call_id.get(item_id, item_id),
                    tool_call_args_delta=delta,
                )
            )

    elif event_type == "response.output_item.done":
        item = chunk.get("item")
        if isinstance(item, dict) and item.get("type") == "function_call":
            call_id = item.get("call_id") or ""
            if call_id:
                out.append(
                    StreamEvent(
                        content="",
                        done=False,
                        event_type="tool_call_end",
                        tool_call_id=call_id,
                    )
                )

    elif event_type == "response.completed":
        response = chunk.get("response")
        response = response if isinstance(response, dict) else {}
        usage = response.get("usage")
        if isinstance(usage, dict):
            input_tokens = int(usage.get("input_tokens") or 0)
            output_tokens = int(usage.get("output_tokens") or 0)
            details = usage.get("input_tokens_details")
            cached = 0
            if isinstance(details, dict):
                cached = int(details.get("cached_tokens") or 0)
            out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="usage",
                    usage=Usage(
                        input_tokens=input_tokens,
                        output_tokens=output_tokens,
                        cache_creation_input_tokens=None,
                        cache_read_input_tokens=cached if cached > 0 else None,
                    ),
                )
            )
        status = response.get("status") or "completed"
        if state.saw_tool_call:
            stop = StopReason.tool_use
        elif status == "incomplete":
            stop = StopReason.max_tokens
        else:
            stop = StopReason.end_turn
        out.append(StreamEvent(content="", done=True, stop_reason=stop))

    elif event_type in ("error", "response.failed"):
        msg = chunk.get("message")
        if not isinstance(msg, str) or not msg:
            response = chunk.get("response")
            if isinstance(response, dict):
                err = response.get("error")
                if isinstance(err, dict) and isinstance(err.get("message"), str):
                    msg = err["message"]
        if not isinstance(msg, str) or not msg:
            err = chunk.get("error")
            if isinstance(err, dict) and isinstance(err.get("message"), str):
                msg = err["message"]
        if not isinstance(msg, str) or not msg:
            msg = "ChatGPT-backend stream error"
        state.error = msg

    return out


class ChatGptCodexProvider(BaseProvider):
    capabilities: ProviderCapabilities = ProviderCapabilities.with_freeform_tools()

    def __init__(
        self,
        access_token: str | None = None,
        account_id: str | None = None,
        model: str | None = None,
        base_url: str | None = None,
        *,
        token_source: Callable[[], Awaitable[str]] | None = None,
        connect_timeout: float = 10.0,
        read_idle_timeout: float = 120.0,
    ) -> None:
        if access_token is None and token_source is None:
            raise ConfigError("chatgpt_codex requires access_token or token_source")
        if not account_id:
            raise ConfigError("chatgpt_codex requires account_id")
        self.access_token = access_token
        self.token_source = token_source
        self.account_id = account_id
        self.model = model or _DEFAULT_MODEL
        self.base_url = base_url or _DEFAULT_BASE_URL
        self._reasoning_effort: str | None = None
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

    def reasoning_effort(self, effort: str | None) -> ChatGptCodexProvider:
        """Set the default reasoning effort emitted as ``reasoning.effort``.

        Used when a request does not carry a per-request
        ``provider_options["reasoning_effort"]``. A per-request value always
        wins. Pass ``None`` to leave the ``reasoning`` object off the body when
        no per-request effort is supplied. The string is passed through
        verbatim — the backend validates the value. Mirrors Rust
        ``ChatGptCodexProvider::with_reasoning_effort``.
        """
        self._reasoning_effort = effort
        return self

    def _stream_url(self) -> str:
        return self.base_url

    async def _bearer(self) -> str:
        """Resolve the bearer token for the current request attempt (F5).

        When ``token_source`` is set it is awaited on every call. The retry
        loops live in ``Client`` (``_dispatch_chat`` / ``stream_with``) and
        re-enter ``stream()`` once per attempt, so each attempt fetches a
        fresh token.
        """
        if self.token_source is not None:
            return await self.token_source()
        if self.access_token is None:  # pragma: no cover — guarded in __init__
            raise ConfigError("chatgpt_codex requires access_token or token_source")
        return self.access_token

    def _headers(self, bearer: str) -> dict[str, str]:
        return {
            "authorization": f"Bearer {bearer}",
            "chatgpt-account-id": self.account_id,
            "originator": _ORIGINATOR,
            "openai-beta": "responses=experimental",
            "accept": "text/event-stream",
            "content-type": "application/json",
        }

    def _build_responses_body(self, request: ChatRequest) -> dict[str, Any]:
        model = request.model or self.model

        instructions_parts: list[str] = []
        if request.system_blocks is not None:
            for block in request.system_blocks:
                trimmed = block.text.strip()
                if trimmed:
                    instructions_parts.append(trimmed)
        elif request.system is not None:
            trimmed = request.system.strip()
            if trimmed:
                instructions_parts.append(trimmed)

        input_items: list[dict[str, Any]] = []
        for message in request.messages:
            if message.role == Role.system:
                trimmed = message.content.strip()
                if trimmed:
                    instructions_parts.append(trimmed)
            elif message.role == Role.user:
                input_items.append(
                    {
                        "type": "message",
                        "role": "user",
                        "content": [{"type": "input_text", "text": message.content}],
                    }
                )
            elif message.role == Role.assistant:
                if message.content:
                    input_items.append(
                        {
                            "type": "message",
                            "role": "assistant",
                            "content": [{"type": "output_text", "text": message.content}],
                        }
                    )
                for tool_call in message.tool_calls:
                    input_items.append(
                        {
                            "type": "function_call",
                            "call_id": tool_call.id,
                            "name": tool_call.name,
                            "arguments": json.dumps(tool_call.input),
                        }
                    )
            elif message.role == Role.tool:
                if message.tool_call_id is not None:
                    input_items.append(
                        {
                            "type": "function_call_output",
                            "call_id": message.tool_call_id,
                            "output": message.content,
                        }
                    )

        instructions = (
            "\n\n".join(instructions_parts)
            if instructions_parts
            else "You are a helpful assistant."
        )

        body: dict[str, Any] = {
            "model": model,
            "store": False,
            "stream": True,
            "instructions": instructions,
            "input": input_items,
            "include": ["reasoning.encrypted_content"],
            "tool_choice": "auto",
            "parallel_tool_calls": True,
        }

        if request.tools is not None:
            mapped = [
                {
                    "type": "function",
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema,
                    "strict": None,
                }
                for tool in request.tools
            ]
            if mapped:
                body["tools"] = mapped

        # Reasoning effort: a per-request provider_options value wins; else the
        # provider-level default; else the `reasoning` object is omitted.
        effort: str | None = None
        if request.provider_options is not None:
            candidate = request.provider_options.get("reasoning_effort")
            if isinstance(candidate, str):
                effort = candidate
        if effort is None:
            effort = self._reasoning_effort
        if effort is not None:
            body["reasoning"] = {"effort": effort, "summary": "auto"}

        if request.temperature is not None:
            body["temperature"] = request.temperature

        return body

    def build_model_responses_body(self, request: ModelChatRequest) -> dict[str, Any]:
        """Encode a native ModelChatRequest into the ChatGPT-backend body.

        The four hard overrides below are applied AFTER the shared codec, and
        the codec already shallow-merged ``provider_options`` — so these beat
        both the caller's ``provider_options`` AND an explicit
        ``request.tool_choice``. That is deliberate Rust parity.
        """
        body = build_model_request_body(
            request,
            self.model,
            stream=True,
            default_instructions="You are a helpful assistant.",
        )
        body["store"] = False
        body["include"] = ["reasoning.encrypted_content"]
        body["tool_choice"] = "auto"
        body["parallel_tool_calls"] = True

        # Effort: per-request provider_options wins, then the provider default,
        # else the `reasoning` object is omitted entirely.
        effort: str | None = None
        if request.provider_options is not None:
            candidate = request.provider_options.get("reasoning_effort")
            if isinstance(candidate, str):
                effort = candidate
        if effort is None:
            effort = self._reasoning_effort
        if effort is not None:
            body["reasoning"] = {"effort": effort, "summary": "auto"}
            # The codec's shallow merge will have injected the raw key onto the
            # body. It must never reach the wire.
            body.pop("reasoning_effort", None)

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

    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        self.validate_request(request)
        body = self._build_responses_body(request)
        # F5: resolved at the top of the attempt. Client retry loops re-invoke
        # stream() per attempt (chat() delegates here too), so a token_source
        # is consulted once per attempt. Token-source failures propagate
        # verbatim — they are auth plumbing, not transport errors.
        bearer = await self._bearer()
        try:
            resp = await self._http.send(
                self._http.build_request(
                    "POST", self._stream_url(), headers=self._headers(bearer), json=body
                ),
                stream=True,
            )
        except httpx.HTTPError as exc:
            raise NetworkError(str(exc)) from exc

        try:
            if not resp.is_success:
                error_body = await resp.aread()
                message = f"HTTP {resp.status_code}: {error_body.decode()}"
                retry_after = resp.headers.get("retry-after")
                if retry_after:
                    message = f"Retry-After: {retry_after}\n{message}"
                raise self._map_http_error(resp.status_code, message, resp.headers)

            state = _ChatGptCodexAdapterState()
            try:
                async for line in resp.aiter_lines():
                    if not line.startswith("data: "):
                        continue
                    data = line[len("data: ") :]
                    for event in _parse_sse_event(data, state):
                        yield event
                        if event.done:
                            return
                    if state.error is not None:
                        raise StreamError(state.error)

                raise IncompleteStreamError(
                    "incomplete stream: chatgpt_codex ended without a terminal event"
                )
            except (StreamError, AuthError, RateLimitError, ProviderError, NetworkError):
                raise
            except httpx.ReadTimeout as exc:
                raise StreamReadTimeoutError(
                    f"stream read timed out after {self._read_idle_timeout}s"
                ) from exc
            except httpx.HTTPError as exc:
                raise StreamError(f"stream transport error: {exc}") from exc
        except httpx.ReadTimeout as exc:
            raise StreamReadTimeoutError(
                f"stream read timed out after {self._read_idle_timeout}s"
            ) from exc
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

    async def model_stream(self, request: ModelChatRequest) -> AsyncIterator[ModelStreamDelta]:
        self.validate_model_request(request)
        body = self.build_model_responses_body(request)
        bearer = await self._bearer()
        try:
            resp = await self._http.send(
                self._http.build_request(
                    "POST", self._stream_url(), headers=self._headers(bearer), json=body
                ),
                stream=True,
            )
        except httpx.HTTPError as exc:
            raise NetworkError(str(exc)) from exc

        try:
            if not resp.is_success:
                error_body = await resp.aread()
                message = f"HTTP {resp.status_code}: {error_body.decode()}"
                retry_after = resp.headers.get("retry-after")
                if retry_after:
                    message = f"Retry-After: {retry_after}\n{message}"
                raise self._map_http_error(resp.status_code, message, resp.headers)

            state = ModelStreamState()
            try:
                async for line in resp.aiter_lines():
                    if not line.startswith("data: "):
                        continue
                    for delta in parse_model_sse_event(line[len("data: ") :], state):
                        yield delta
                        if isinstance(delta, ModelStreamDone):
                            return
                    if state.error is not None:
                        raise StreamError(state.error)

                # Provider string is `chatgpt-codex` (hyphen) on the native
                # path; the legacy adapter above uses `chatgpt_codex`.
                raise IncompleteStreamError(
                    "incomplete stream: chatgpt-codex ended without a terminal event"
                )
            except (StreamError, AuthError, RateLimitError, ProviderError, NetworkError):
                raise
            except httpx.ReadTimeout as exc:
                raise StreamReadTimeoutError(
                    f"stream read timed out after {self._read_idle_timeout}s"
                ) from exc
            except httpx.HTTPError as exc:
                raise StreamError(f"stream transport error: {exc}") from exc
        except httpx.ReadTimeout as exc:
            raise StreamReadTimeoutError(
                f"stream read timed out after {self._read_idle_timeout}s"
            ) from exc
        finally:
            await resp.aclose()

    async def model_chat(self, request: ModelChatRequest) -> ModelChatResponse:
        """Native chat = native stream + collect: there is no blocking endpoint."""
        response = await collect_model_stream(self.model_stream(request))
        if not response.model:
            response.model = request.model or self.model
        return response
