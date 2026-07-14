from __future__ import annotations

import json
from collections.abc import AsyncIterator
from typing import Any

import httpx

from motosan_ai._stream_collect import collect_stream
from motosan_ai.error import AuthError, NetworkError, ProviderError, RateLimitError, StreamError
from motosan_ai.provider_base import BaseProvider, ProviderCapabilities
from motosan_ai.types import (
    ChatRequest,
    ChatResponse,
    Message,
    Role,
    StopReason,
    StreamEvent,
    ToolCall,
    Usage,
    content_block_to_dict,
    mcp_server_config_to_dict,
    mcp_tool_config_to_dict,
    system_block_to_dict,
)

_DEFAULT_BASE_URL = "https://api.anthropic.com"
_ANTHROPIC_VERSION = "2023-06-01"
_CLAUDE_CODE_PREFIX = "You are Claude Code, Anthropic's official CLI for Claude."
_OAUTH_BETAS = [
    "claude-code-20250219",
    "oauth-2025-04-20",
    "fine-grained-tool-streaming-2025-05-14",
]
_INTERLEAVED_THINKING_BETA = "interleaved-thinking-2025-05-14"
_MCP_BETA = "mcp-client-2025-11-20"
_ADAPTIVE_THINKING_MODELS = {"claude-opus-4-8", "claude-opus-4-7", "claude-opus-4-6"}
_STOP_REASON_MAP = {
    "end_turn": StopReason.end_turn,
    "max_tokens": StopReason.max_tokens,
    "tool_use": StopReason.tool_use,
    "stop": StopReason.stop,
    "stop_sequence": StopReason.stop_sequence,
}


def _with_cache_control(block: dict[str, Any]) -> dict[str, Any]:
    return {**block, "cache_control": {"type": "ephemeral"}}


def _model_uses_adaptive_thinking(model: str) -> bool:
    return model in _ADAPTIVE_THINKING_MODELS


def _apply_thinking_config(body: dict[str, Any], model: str, budget_tokens: int) -> None:
    if _model_uses_adaptive_thinking(model):
        # Pi marks Opus 4.8/4.7/4.6 as forceAdaptiveThinking: use Anthropic's
        # adaptive shape instead of the older budget-token shape.
        body["thinking"] = {"type": "adaptive", "display": "summarized"}
        body["output_config"] = {"effort": "high"}
    else:
        body["thinking"] = {
            "type": "enabled",
            "budget_tokens": budget_tokens,
            "display": "summarized",
        }


def _usage_from_dict(usage: dict[str, Any] | None) -> Usage:
    usage = usage or {}
    return Usage(
        input_tokens=int(usage.get("input_tokens", 0) or 0),
        output_tokens=int(usage.get("output_tokens", 0) or 0),
        cache_creation_input_tokens=usage.get("cache_creation_input_tokens"),
        cache_read_input_tokens=usage.get("cache_read_input_tokens"),
    )


class AnthropicProvider(BaseProvider):
    capabilities: ProviderCapabilities = ProviderCapabilities.full()

    def __init__(
        self,
        api_key: str,
        model: str | None = None,
        base_url: str | None = None,
    ) -> None:
        self.api_key = api_key
        self.model = model or "claude-sonnet-4-6"
        self.base_url = (base_url or _DEFAULT_BASE_URL).rstrip("/")
        self._is_oauth = api_key.startswith("sk-ant-oat01-")
        self._http = httpx.AsyncClient(timeout=120.0)

    def _endpoint(self) -> str:
        return f"{self.base_url}/v1/messages"

    def _headers(self, *, has_mcp: bool = False, adaptive_thinking: bool = False) -> dict[str, str]:
        headers: dict[str, str] = {
            "anthropic-version": _ANTHROPIC_VERSION,
            "content-type": "application/json",
        }
        betas: list[str] = []
        if self._is_oauth:
            headers["authorization"] = f"Bearer {self.api_key}"
            headers["user-agent"] = "claude-code/1.0.33"
            headers["x-app"] = "cli"
            betas.extend(_OAUTH_BETAS)
            if not adaptive_thinking:
                betas.append(_INTERLEAVED_THINKING_BETA)
        else:
            headers["x-api-key"] = self.api_key
        if has_mcp:
            betas.append(_MCP_BETA)
        if betas:
            headers["anthropic-beta"] = ",".join(betas)
        return headers

    @staticmethod
    def _serialize_messages(
        messages: list[Message],
        *,
        oauth: bool = False,
    ) -> tuple[list[dict[str, Any]], str | None]:
        outgoing: list[dict[str, Any]] = []
        system_parts: list[str] = []

        for message in messages:
            if message.role == Role.system:
                if message.content.strip():
                    system_parts.append(message.content)
                continue

            if message.role == Role.user:
                if message.content_blocks:
                    blocks = [content_block_to_dict(block) for block in message.content_blocks]
                    if message.cache and blocks:
                        blocks[-1] = _with_cache_control(blocks[-1])
                    outgoing.append({"role": "user", "content": blocks})
                elif message.cache:
                    outgoing.append(
                        {
                            "role": "user",
                            "content": [
                                _with_cache_control({"type": "text", "text": message.content})
                            ],
                        }
                    )
                elif oauth:
                    outgoing.append(
                        {"role": "user", "content": [{"type": "text", "text": message.content}]}
                    )
                else:
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
                    if message.cache and blocks:
                        blocks[-1] = _with_cache_control(blocks[-1])
                    outgoing.append({"role": "assistant", "content": blocks})
                elif message.cache:
                    outgoing.append(
                        {
                            "role": "assistant",
                            "content": [
                                _with_cache_control({"type": "text", "text": message.content})
                            ],
                        }
                    )
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

    def _build_body(self, request: ChatRequest, *, stream: bool = False) -> dict[str, Any]:
        messages, extracted_system = self._serialize_messages(
            request.messages,
            oauth=self._is_oauth,
        )

        body: dict[str, Any] = {
            "model": request.model or self.model,
            "messages": messages,
            "max_tokens": request.max_tokens or 4096,
        }
        if stream:
            body["stream"] = True

        plain_system = request.system or extracted_system
        if self._is_oauth:
            oauth_blocks: list[dict[str, Any]] = [
                _with_cache_control({"type": "text", "text": _CLAUDE_CODE_PREFIX}),
            ]
            if request.system_blocks:
                oauth_blocks.extend(system_block_to_dict(block) for block in request.system_blocks)
            elif plain_system:
                block = {"type": "text", "text": plain_system}
                if request.system_cache:
                    block = _with_cache_control(block)
                oauth_blocks.append(block)
            body["system"] = oauth_blocks
        elif request.system_blocks:
            body["system"] = [system_block_to_dict(block) for block in request.system_blocks]
        elif plain_system and request.system_cache:
            body["system"] = [_with_cache_control({"type": "text", "text": plain_system})]
        elif plain_system:
            body["system"] = plain_system

        if request.thinking is not None:
            model = str(body["model"])
            if not _model_uses_adaptive_thinking(model):
                body["temperature"] = 1.0
            _apply_thinking_config(body, model, request.thinking.budget_tokens)
        elif request.temperature is not None:
            body["temperature"] = request.temperature

        tool_blocks: list[dict[str, Any]] = []
        if request.tools:
            for tool in request.tools:
                obj: dict[str, Any] = {
                    "name": tool.name,
                    "description": tool.description or "",
                    "input_schema": tool.input_schema or {"type": "object", "properties": {}},
                }
                if tool.cache:
                    obj["cache_control"] = {"type": "ephemeral"}
                tool_blocks.append(obj)
        if request.mcp_tool_configs:
            tool_blocks.extend(mcp_tool_config_to_dict(cfg) for cfg in request.mcp_tool_configs)
        if tool_blocks:
            body["tools"] = tool_blocks

        if request.tool_choice is not None:
            choice = request.tool_choice
            if choice.type == "auto":
                body["tool_choice"] = {"type": "auto"}
            elif choice.type == "required":
                body["tool_choice"] = {"type": "any"}
            elif choice.type == "none":
                body.pop("tools", None)
            elif choice.type == "tool":
                body["tool_choice"] = {"type": "tool", "name": choice.name}

        if request.stop_sequences:
            body["stop_sequences"] = list(request.stop_sequences)
        if request.mcp_servers:
            body["mcp_servers"] = [
                mcp_server_config_to_dict(server) for server in request.mcp_servers
            ]
        if request.provider_options:
            body.update(request.provider_options)
        return body

    def _response_error_message(self, status: int, headers: httpx.Headers, text: str) -> str:
        try:
            payload = json.loads(text)
            message = str((payload.get("error") or {}).get("message") or "")
        except (json.JSONDecodeError, TypeError, AttributeError):
            message = text[:500]
        if not message:
            message = f"HTTP {status} error"
        if self.api_key:
            message = message.replace(self.api_key, "[REDACTED]")
        message = f"HTTP {status}: {message}"
        retry_after = headers.get("retry-after")
        if retry_after:
            message = f"Retry-After: {retry_after}\n{message}"
        return message

    @staticmethod
    def _map_http_error(status: int, message: str) -> Exception:
        if status == 401:
            return AuthError(message)
        if status == 429:
            return RateLimitError(message)
        return ProviderError(message)

    @staticmethod
    def _has_mcp(request: ChatRequest) -> bool:
        return bool(request.mcp_servers) or bool(request.mcp_tool_configs)

    async def chat(self, request: ChatRequest) -> ChatResponse:
        self.validate_request(request)
        # OAuth tokens require streaming — collect stream into a single response.
        if self._is_oauth:
            response = await collect_stream(self.stream(request))
            stop_reason = response.stop_reason
            if response.tool_calls:
                stop_reason = StopReason.tool_use
            return ChatResponse(
                content=response.content,
                model=request.model or self.model,
                usage=response.usage,
                stop_reason=stop_reason,
                tool_calls=response.tool_calls,
                thinking=response.thinking,
            )

        body = self._build_body(request)
        adaptive_thinking = (body.get("thinking") or {}).get("type") == "adaptive"
        try:
            resp = await self._http.post(
                self._endpoint(),
                headers=self._headers(
                    has_mcp=self._has_mcp(request), adaptive_thinking=adaptive_thinking
                ),
                json=body,
            )
        except httpx.HTTPError as exc:
            raise NetworkError(str(exc)) from exc

        if not resp.is_success:
            message = self._response_error_message(resp.status_code, resp.headers, resp.text)
            raise self._map_http_error(resp.status_code, message)

        payload = resp.json()
        return self._parse_response(payload)

    def _parse_response(self, payload: dict[str, Any]) -> ChatResponse:
        content_blocks = payload.get("content", [])
        text = "".join(
            block.get("text", "") for block in content_blocks if block.get("type") == "text"
        )
        thinking_parts = [
            block.get("thinking", "") for block in content_blocks if block.get("type") == "thinking"
        ]
        thinking = "".join(thinking_parts) if thinking_parts else None
        tool_calls = [
            ToolCall(
                id=block.get("id", ""),
                name=block.get("name", ""),
                input=block.get("input", {}) or {},
            )
            for block in content_blocks
            if block.get("type") == "tool_use"
        ]
        return ChatResponse(
            content=text,
            tool_calls=tool_calls,
            model=payload.get("model", self.model),
            usage=_usage_from_dict(payload.get("usage", {})),
            stop_reason=_STOP_REASON_MAP.get(payload.get("stop_reason"), StopReason.other),
            thinking=thinking,
        )

    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        self.validate_request(request)
        body = self._build_body(request, stream=True)
        adaptive_thinking = (body.get("thinking") or {}).get("type") == "adaptive"
        try:
            resp = await self._http.send(
                self._http.build_request(
                    "POST",
                    self._endpoint(),
                    headers=self._headers(
                        has_mcp=self._has_mcp(request), adaptive_thinking=adaptive_thinking
                    ),
                    json=body,
                ),
                stream=True,
            )
        except httpx.HTTPError as exc:
            raise NetworkError(str(exc)) from exc

        current_tool_id: str | None = None
        current_stop_reason: StopReason | None = None

        try:
            if not resp.is_success:
                error_body = await resp.aread()
                message = self._response_error_message(
                    resp.status_code, resp.headers, error_body.decode()
                )
                raise self._map_http_error(resp.status_code, message)

            async for line in resp.aiter_lines():
                if not line.startswith("data: "):
                    continue
                data = line[6:].strip()
                if not data or data == "[DONE]":
                    if data == "[DONE]":
                        yield StreamEvent(content="", done=True, stop_reason=current_stop_reason)
                        return
                    continue

                try:
                    payload = json.loads(data)
                except json.JSONDecodeError as exc:
                    raise StreamError(f"malformed SSE chunk: {exc}") from exc

                event_type = payload.get("type")

                if event_type == "error":
                    error = payload.get("error") or {}
                    error_type = error.get("type") or "unknown_error"
                    error_message = error.get("message") or ""
                    raise StreamError(f"anthropic stream error: {error_type}: {error_message}")

                if event_type == "message_start":
                    usage = (payload.get("message") or {}).get("usage")
                    if usage:
                        yield StreamEvent(
                            content="",
                            done=False,
                            event_type="usage",
                            usage=_usage_from_dict(usage),
                        )
                    continue

                if event_type == "message_delta":
                    delta = payload.get("delta") or {}
                    reason = delta.get("stop_reason")
                    if reason:
                        current_stop_reason = _STOP_REASON_MAP.get(reason, StopReason.other)
                    usage = payload.get("usage")
                    if usage:
                        yield StreamEvent(
                            content="",
                            done=False,
                            event_type="usage",
                            usage=_usage_from_dict(usage),
                        )
                    continue

                if event_type == "content_block_start":
                    block = payload.get("content_block") or {}
                    if block.get("type") == "tool_use":
                        current_tool_id = block.get("id", "")
                        yield StreamEvent(
                            content="",
                            done=False,
                            tool_call_id=current_tool_id,
                            tool_call_name=block.get("name", ""),
                            event_type="tool_call_start",
                        )

                elif event_type == "content_block_delta":
                    delta = payload.get("delta") or {}
                    delta_type = delta.get("type")
                    if delta_type == "text_delta":
                        text = delta.get("text", "")
                        if text:
                            yield StreamEvent(content=text, done=False)
                    elif delta_type == "thinking_delta":
                        thinking_text = delta.get("thinking", "")
                        if thinking_text:
                            yield StreamEvent(
                                content=thinking_text, done=False, event_type="thinking"
                            )
                    elif delta_type == "input_json_delta":
                        partial = delta.get("partial_json", "")
                        if partial:
                            yield StreamEvent(
                                content="",
                                done=False,
                                tool_call_id=current_tool_id,
                                tool_call_args_delta=partial,
                                event_type="tool_call_args",
                            )

                elif event_type == "content_block_stop":
                    if current_tool_id is not None:
                        yield StreamEvent(
                            content="",
                            done=False,
                            tool_call_id=current_tool_id,
                            event_type="tool_call_end",
                        )
                        current_tool_id = None

                elif event_type == "message_stop":
                    yield StreamEvent(content="", done=True, stop_reason=current_stop_reason)
                    return
        except StreamError:
            raise
        except (AuthError, RateLimitError, ProviderError, NetworkError):
            raise
        except httpx.HTTPError as exc:
            raise StreamError(f"stream transport error: {exc}") from exc
        finally:
            await resp.aclose()
