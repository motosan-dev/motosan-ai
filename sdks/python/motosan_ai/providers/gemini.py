from __future__ import annotations

import json
import uuid
from collections.abc import AsyncIterator
from typing import Any

import httpx

from motosan_ai.error import AuthError, NetworkError, ProviderError, RateLimitError
from motosan_ai.provider_base import BaseProvider, ProviderCapabilities
from motosan_ai.types import (
    ChatRequest,
    ChatResponse,
    ImageBlock,
    ImageSourceBase64,
    ImageSourceUrl,
    Role,
    StopReason,
    StreamEvent,
    TextBlock,
    ToolCall,
    Usage,
)

_DEFAULT_BASE_URL = "https://generativelanguage.googleapis.com/v1beta"
_DEFAULT_MODEL = "gemini-2.5-flash"
_DEFAULT_MAX_TOKENS = 8192


def _gen_tool_call_id() -> str:
    return f"call_{uuid.uuid4().hex[:12]}"


def _part_for_block(block: Any) -> list[dict[str, Any]]:
    if isinstance(block, TextBlock):
        return [{"text": block.text}]
    if isinstance(block, ImageBlock):
        src = block.source
        if isinstance(src, ImageSourceBase64):
            return [{"inlineData": {"mimeType": src.media_type, "data": src.data}}]
        if isinstance(src, ImageSourceUrl):
            return [{"fileData": {"fileUri": src.url}}]
    return []


def _stop_reason_for(finish_reason: str | None, has_tool_calls: bool = False) -> StopReason:
    if finish_reason == "MAX_TOKENS":
        return StopReason.max_tokens
    if has_tool_calls:
        return StopReason.tool_use
    if finish_reason == "STOP":
        return StopReason.end_turn
    return StopReason.other


def _usage_from_metadata(meta: dict[str, Any] | None) -> Usage:
    meta = meta or {}
    return Usage(
        input_tokens=int(meta.get("promptTokenCount", 0) or 0),
        output_tokens=int(meta.get("candidatesTokenCount", 0) or 0),
    )


class GeminiProvider(BaseProvider):
    capabilities: ProviderCapabilities = ProviderCapabilities.with_image()

    def __init__(
        self,
        api_key: str,
        model: str | None = None,
        base_url: str | None = None,
    ) -> None:
        self.api_key = api_key
        self.model = model or _DEFAULT_MODEL
        self.base_url = (base_url or _DEFAULT_BASE_URL).rstrip("/")
        self._http = httpx.AsyncClient(timeout=120.0)

    def _model_for(self, request: ChatRequest) -> str:
        return request.model or self.model

    def _generate_url(self, request: ChatRequest) -> str:
        return f"{self.base_url}/models/{self._model_for(request)}:generateContent"

    def _stream_url(self, request: ChatRequest) -> str:
        return f"{self.base_url}/models/{self._model_for(request)}:streamGenerateContent?alt=sse"

    def _headers(self) -> dict[str, str]:
        return {"x-goog-api-key": self.api_key, "content-type": "application/json"}

    def _build_body(self, request: ChatRequest) -> dict[str, Any]:
        contents: list[dict[str, Any]] = []
        extracted_system: str | None = None

        for msg in request.messages:
            if msg.role == Role.system:
                if msg.content.strip():
                    extracted_system = msg.content
                continue

            if msg.role == Role.user:
                parts: list[dict[str, Any]] = []
                if msg.content:
                    parts.append({"text": msg.content})
                for block in msg.content_blocks:
                    parts.extend(_part_for_block(block))
                if not parts:
                    parts.append({"text": ""})
                contents.append({"role": "user", "parts": parts})
                continue

            if msg.role == Role.assistant:
                parts = []
                if msg.content:
                    parts.append({"text": msg.content})
                for tc in msg.tool_calls:
                    parts.append({"functionCall": {"name": tc.name, "args": tc.input}})
                if not parts:
                    parts.append({"text": ""})
                contents.append({"role": "model", "parts": parts})
                continue

            if msg.role == Role.tool:
                name = msg.tool_call_id or ""
                try:
                    response = json.loads(msg.content)
                except (json.JSONDecodeError, TypeError):
                    response = {"result": msg.content}
                if not isinstance(response, dict):
                    response = {"result": response}
                contents.append(
                    {
                        "role": "user",
                        "parts": [{"functionResponse": {"name": name, "response": response}}],
                    }
                )
                continue

        system_text = ""
        if request.system_blocks:
            system_text = "\n".join(block.text for block in request.system_blocks if block.text)
        if not system_text and request.system:
            system_text = request.system
        if not system_text and extracted_system:
            system_text = extracted_system

        body: dict[str, Any] = {"contents": contents}
        if system_text:
            body["systemInstruction"] = {"parts": [{"text": system_text}]}

        gen_config: dict[str, Any] = {"maxOutputTokens": request.max_tokens or _DEFAULT_MAX_TOKENS}
        if request.temperature is not None:
            gen_config["temperature"] = request.temperature
        if request.stop_sequences:
            gen_config["stopSequences"] = list(request.stop_sequences)
        body["generationConfig"] = gen_config

        if request.tools:
            declarations: list[dict[str, Any]] = []
            for tool in request.tools:
                decl: dict[str, Any] = {"name": tool.name, "description": tool.description or ""}
                if tool.input_schema:
                    decl["parameters"] = tool.input_schema
                declarations.append(decl)
            body["tools"] = [{"functionDeclarations": declarations}]

            choice = request.tool_choice
            if choice is not None and choice.type == "none":
                body.pop("tools", None)
            else:
                mode = "AUTO"
                if choice is not None and choice.type in {"required", "tool"}:
                    mode = "ANY"
                config: dict[str, Any] = {"mode": mode}
                if choice is not None and choice.type == "tool":
                    config["allowedFunctionNames"] = [choice.name]
                body["toolConfig"] = {"functionCallingConfig": config}

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

    async def chat(self, request: ChatRequest) -> ChatResponse:
        self.validate_request(request)
        try:
            resp = await self._http.post(
                self._generate_url(request), headers=self._headers(), json=self._build_body(request)
            )
        except httpx.HTTPError as exc:
            raise NetworkError(str(exc)) from exc
        if not resp.is_success:
            message = self._response_error_message(resp.status_code, resp.headers, resp.text)
            raise self._map_http_error(resp.status_code, message)
        return self._parse_response(resp.json(), request)

    def _parse_response(self, payload: dict[str, Any], request: ChatRequest) -> ChatResponse:
        candidates = payload.get("candidates") or []
        candidate = candidates[0] if candidates else {}
        parts = (candidate.get("content") or {}).get("parts") or []

        text = ""
        tool_calls: list[ToolCall] = []
        for part in parts:
            if part.get("text"):
                text += part["text"]
            function_call = part.get("functionCall")
            if function_call:
                tool_calls.append(
                    ToolCall(
                        id=_gen_tool_call_id(),
                        name=function_call.get("name", ""),
                        input=function_call.get("args") or {},
                    )
                )

        finish_reason = candidate.get("finishReason", "STOP")
        return ChatResponse(
            content=text,
            tool_calls=tool_calls,
            model=payload.get("modelVersion") or self._model_for(request),
            usage=_usage_from_metadata(payload.get("usageMetadata")),
            stop_reason=_stop_reason_for(finish_reason, bool(tool_calls)),
        )

    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        self.validate_request(request)
        try:
            resp = await self._http.send(
                self._http.build_request(
                    "POST",
                    self._stream_url(request),
                    headers=self._headers(),
                    json=self._build_body(request),
                ),
                stream=True,
            )
        except httpx.HTTPError as exc:
            raise NetworkError(str(exc)) from exc
        if not resp.is_success:
            error_body = await resp.aread()
            message = self._response_error_message(
                resp.status_code, resp.headers, error_body.decode()
            )
            raise self._map_http_error(resp.status_code, message)

        async for line in resp.aiter_lines():
            if not line.startswith("data: "):
                continue
            data = line[len("data: ") :].strip()
            if not data or data == "[DONE]":
                continue
            try:
                payload = json.loads(data)
            except json.JSONDecodeError:
                continue

            candidates = payload.get("candidates") or []
            if not candidates:
                continue
            candidate = candidates[0]
            parts = (candidate.get("content") or {}).get("parts") or []
            finish_reason = candidate.get("finishReason")
            has_tool_calls = False

            for part in parts:
                text = part.get("text")
                if text:
                    yield StreamEvent(content=text, done=False)
                function_call = part.get("functionCall")
                if function_call:
                    has_tool_calls = True
                    call_id = _gen_tool_call_id()
                    name = function_call.get("name", "")
                    args = function_call.get("args") or {}
                    yield StreamEvent(
                        content="",
                        done=False,
                        tool_call_id=call_id,
                        tool_call_name=name,
                        event_type="tool_call_start",
                    )
                    yield StreamEvent(
                        content="",
                        done=False,
                        tool_call_id=call_id,
                        tool_call_args_delta=json.dumps(args),
                        event_type="tool_call_args",
                    )
                    yield StreamEvent(
                        content="", done=False, tool_call_id=call_id, event_type="tool_call_end"
                    )

            usage_meta = payload.get("usageMetadata")
            if usage_meta:
                yield StreamEvent(
                    content="",
                    done=False,
                    event_type="usage",
                    usage=_usage_from_metadata(usage_meta),
                )

            if finish_reason:
                yield StreamEvent(
                    content="",
                    done=True,
                    stop_reason=_stop_reason_for(finish_reason, has_tool_calls),
                )
                return
