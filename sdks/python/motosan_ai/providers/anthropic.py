from __future__ import annotations

import json
from collections.abc import AsyncIterator
from typing import Any

import httpx

from motosan_ai.error import AuthError, NetworkError, ProviderError, RateLimitError
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

_DEFAULT_BASE_URL = "https://api.anthropic.com"
_ANTHROPIC_VERSION = "2023-06-01"
_CLAUDE_CODE_PREFIX = "You are Claude Code, Anthropic's official CLI for Claude."


class AnthropicProvider:
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

    def _headers(self) -> dict[str, str]:
        headers: dict[str, str] = {
            "anthropic-version": _ANTHROPIC_VERSION,
            "content-type": "application/json",
        }
        if self._is_oauth:
            headers["authorization"] = f"Bearer {self.api_key}"
            headers["anthropic-beta"] = (
                "claude-code-20250219,oauth-2025-04-20,"
                "fine-grained-tool-streaming-2025-05-14,"
                "interleaved-thinking-2025-05-14"
            )
            headers["user-agent"] = "claude-code/1.0.33"
            headers["x-app"] = "cli"
        else:
            headers["x-api-key"] = self.api_key
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
                if oauth:
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

    def _build_system(self, user_system: str | None) -> Any:
        if not self._is_oauth:
            if user_system:
                return user_system
            return None
        # OAuth: system must be array of blocks.
        # Claude Code prefix in first block (with cache_control), user system in second block.
        blocks: list[dict[str, Any]] = [
            {"type": "text", "text": _CLAUDE_CODE_PREFIX, "cache_control": {"type": "ephemeral"}},
        ]
        if user_system:
            blocks.append({"type": "text", "text": user_system})
        return blocks

    def _build_body(self, request: ChatRequest, *, stream: bool = False) -> dict[str, Any]:
        messages, extracted_system = self._serialize_messages(
            request.messages,
            oauth=self._is_oauth,
        )
        system = self._build_system(request.system or extracted_system)

        body: dict[str, Any] = {
            "model": request.model or self.model,
            "messages": messages,
            "max_tokens": request.max_tokens or 4096,
        }
        if stream:
            body["stream"] = True
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
        return body

    @staticmethod
    def _map_http_error(status: int, message: str) -> Exception:
        if status == 401:
            return AuthError(message)
        if status == 429:
            return RateLimitError(message)
        return ProviderError(message)

    async def chat(self, request: ChatRequest) -> ChatResponse:
        # OAuth tokens require streaming — collect stream into a single response.
        if self._is_oauth:
            content = ""
            tool_calls: list[ToolCall] = []
            current_tc_id = ""
            current_tc_name = ""
            current_tc_args = ""
            stop_reason = StopReason.end_turn

            async for event in self.stream(request):
                if event.done:
                    break
                if event.event_type == "text" and event.content:
                    content += event.content
                elif event.event_type == "tool_call_start":
                    current_tc_id = event.tool_call_id or ""
                    current_tc_name = event.tool_call_name or ""
                    current_tc_args = ""
                elif event.event_type == "tool_call_args":
                    current_tc_args += event.tool_call_args_delta or ""
                elif event.event_type == "tool_call_end":
                    try:
                        parsed_input = json.loads(current_tc_args) if current_tc_args else {}
                    except json.JSONDecodeError:
                        parsed_input = {}
                    tool_calls.append(
                        ToolCall(id=current_tc_id, name=current_tc_name, input=parsed_input)
                    )
                    current_tc_id = ""
                    current_tc_name = ""
                    current_tc_args = ""

            if tool_calls:
                stop_reason = StopReason.tool_use

            return ChatResponse(
                content=content,
                model=request.model or self.model,
                usage=Usage(0, 0),
                stop_reason=stop_reason,
                tool_calls=tool_calls,
            )

        body = self._build_body(request)
        try:
            resp = await self._http.post(self._endpoint(), headers=self._headers(), json=body)
        except httpx.HTTPError as exc:
            raise NetworkError(str(exc)) from exc

        if not resp.is_success:
            raise self._map_http_error(resp.status_code, resp.text)

        payload = resp.json()
        return self._parse_response(payload)

    def _parse_response(self, payload: dict[str, Any]) -> ChatResponse:
        content_blocks = payload.get("content", [])
        text = "".join(
            block.get("text", "") for block in content_blocks if block.get("type") == "text"
        )
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

        if not resp.is_success:
            error_body = await resp.aread()
            raise self._map_http_error(resp.status_code, error_body.decode())

        current_tool_id: str | None = None

        async for line in resp.aiter_lines():
            if not line.startswith("data: "):
                continue
            data = line[6:]
            if data == "[DONE]":
                yield StreamEvent(content="", done=True)
                return

            try:
                payload = json.loads(data)
            except json.JSONDecodeError:
                continue

            event_type = payload.get("type")

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
                yield StreamEvent(content="", done=True)
                return
