from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai.error import IncompleteStreamError, ProviderError, StreamError
from motosan_ai.provider_base import ProviderCapabilities
from motosan_ai.providers.chatgpt_codex import ChatGptCodexProvider
from motosan_ai.types import (
    FreeformTool,
    FreeformToolFormat,
    FunctionCallOutputText,
    Message,
    ModelChatRequest,
    ModelToolCallFreeform,
    ModelToolOutputCustom,
    ModelToolSpecFreeform,
    StopReason,
    ToolChoice,
)

_URL = "https://chatgpt.com/backend-api/codex/responses"

FREEFORM_SPEC = ModelToolSpecFreeform(
    tool=FreeformTool(
        name="exec",
        description="Run JavaScript",
        format=FreeformToolFormat(type="grammar", syntax="lark", definition="start: source"),
    )
)


def _provider() -> ChatGptCodexProvider:
    return ChatGptCodexProvider("oauth-token", "acct-123", "gpt-5.5", None)


def _native_request() -> ModelChatRequest:
    return (
        ModelChatRequest.builder().message(Message.user("run js")).tool_spec(FREEFORM_SPEC).build()
    )


def _sse(*payloads: dict) -> str:
    return "\n".join(f"data: {json.dumps(p)}" for p in payloads) + "\n"


def test_capabilities_declare_freeform_but_not_image_or_document():
    assert _provider().capabilities == ProviderCapabilities.with_freeform_tools()


def test_native_body_has_the_codex_hard_overrides():
    body = _provider().build_model_responses_body(_native_request())

    assert body["model"] == "gpt-5.5"
    assert body["stream"] is True
    assert body["store"] is False
    assert body["include"] == ["reasoning.encrypted_content"]
    assert body["parallel_tool_calls"] is True
    assert body["tool_choice"] == "auto"
    assert body["instructions"] == "You are a helpful assistant."
    assert body["tools"][0]["type"] == "custom"
    assert body["tools"][0]["format"]["syntax"] == "lark"


def test_native_body_tool_choice_override_beats_the_caller():
    request = (
        ModelChatRequest.builder()
        .message(Message.user("hi"))
        .tool_choice(ToolChoice.required())
        .build()
    )
    assert _provider().build_model_responses_body(request)["tool_choice"] == "auto"


def test_native_body_per_request_effort_beats_the_provider_default():
    provider = _provider().reasoning_effort("low")
    request = (
        ModelChatRequest.builder()
        .message(Message.user("hi"))
        .provider_options({"reasoning_effort": "high"})
        .build()
    )
    body = provider.build_model_responses_body(request)

    assert body["reasoning"] == {"effort": "high", "summary": "auto"}
    # The shallow merge injected the raw key; it must never reach the wire.
    assert "reasoning_effort" not in body


def test_native_body_falls_back_to_the_provider_default_effort():
    body = (
        _provider()
        .reasoning_effort("medium")
        .build_model_responses_body(ModelChatRequest.builder().message(Message.user("hi")).build())
    )
    assert body["reasoning"] == {"effort": "medium", "summary": "auto"}


def test_native_body_omits_reasoning_when_no_effort_resolves():
    body = _provider().build_model_responses_body(
        ModelChatRequest.builder().message(Message.user("hi")).build()
    )
    assert "reasoning" not in body
    assert "reasoning_effort" not in body


def test_native_body_hoists_system_messages_into_instructions():
    request = (
        ModelChatRequest.builder()
        .message(Message.system("be terse"))
        .message(Message.user("hi"))
        .build()
    )
    body = _provider().build_model_responses_body(request)
    assert body["instructions"] == "be terse"
    assert len(body["input"]) == 1
    assert body["input"][0]["role"] == "user"


@respx.mock
async def test_native_stream_decodes_custom_delta_and_done():
    respx.post(_URL).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {
                    "type": "response.custom_tool_call_input.delta",
                    "call_id": "call_js",
                    "delta": "console.",
                },
                {
                    "type": "response.custom_tool_call_input.delta",
                    "call_id": "call_js",
                    "delta": "log(1);\n",
                },
                {
                    "type": "response.output_item.done",
                    "item": {
                        "type": "custom_tool_call",
                        "call_id": "call_js",
                        "name": "exec",
                        "input": "console.log(1);\n",
                    },
                },
                {
                    "type": "response.completed",
                    "response": {
                        "status": "completed",
                        "usage": {"input_tokens": 2, "output_tokens": 3},
                    },
                },
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    response = await _provider().model_chat(_native_request())

    assert response.tool_calls == [
        ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);\n")
    ]
    assert response.stop_reason == StopReason.tool_use
    assert response.usage.output_tokens == 3
    assert response.model == "gpt-5.5"


@respx.mock
async def test_native_stream_maps_response_incomplete_to_max_tokens():
    respx.post(_URL).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {"type": "response.output_text.delta", "delta": "partial"},
                {
                    "type": "response.incomplete",
                    "response": {
                        "status": "incomplete",
                        "usage": {"input_tokens": 6, "output_tokens": 7},
                        "incomplete_details": {"reason": "max_output_tokens"},
                    },
                },
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    response = await _provider().model_chat(
        ModelChatRequest.builder().message(Message.user("short")).build()
    )
    assert response.content == "partial"
    assert response.stop_reason == StopReason.max_tokens
    assert response.usage.output_tokens == 7


@respx.mock
async def test_native_stream_eof_without_terminal_is_incomplete():
    respx.post(_URL).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {
                    "type": "response.custom_tool_call_input.delta",
                    "call_id": "call_js",
                    "delta": "console.",
                }
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    # Note the hyphen: the native provider string is `chatgpt-codex`, not the
    # legacy adapter's `chatgpt_codex`.
    with pytest.raises(
        IncompleteStreamError,
        match="incomplete stream: chatgpt-codex ended without a terminal event",
    ):
        async for _ in _provider().model_stream(_native_request()):
            pass
    assert issubclass(IncompleteStreamError, StreamError)


@respx.mock
async def test_native_stream_sends_history_byte_exact():
    captured: dict = {}
    raw = '{"this":"looks like json"}\nconsole.log(\'but is JS\');'

    def _capture(request):
        captured["body"] = json.loads(request.content)
        captured["headers"] = dict(request.headers)
        return httpx.Response(
            200,
            text=_sse(
                {
                    "type": "response.completed",
                    "response": {
                        "status": "completed",
                        "usage": {"input_tokens": 1, "output_tokens": 1},
                    },
                }
            ),
            headers={"content-type": "text/event-stream"},
        )

    respx.post(_URL).mock(side_effect=_capture)

    request = (
        ModelChatRequest.builder()
        .message(Message.user("run js"))
        .tool_call(ModelToolCallFreeform(id="call_js", name="exec", input=raw))
        .tool_output(
            ModelToolOutputCustom(
                call_id="call_js", output=FunctionCallOutputText(text="done"), name="exec"
            )
        )
        .tool_spec(FREEFORM_SPEC)
        .build()
    )
    response = await _provider().model_chat(request)

    assert response.stop_reason == StopReason.end_turn
    body = captured["body"]
    assert [item["type"] for item in body["input"]] == [
        "message",
        "custom_tool_call",
        "custom_tool_call_output",
    ]
    assert body["input"][1]["input"] == raw
    assert body["tools"][0]["type"] == "custom"
    assert captured["headers"]["authorization"] == "Bearer oauth-token"
    assert captured["headers"]["chatgpt-account-id"] == "acct-123"
    assert captured["headers"]["openai-beta"] == "responses=experimental"


@respx.mock
async def test_native_stream_maps_http_errors():
    respx.post(_URL).mock(return_value=httpx.Response(500, text="boom"))
    with pytest.raises(ProviderError, match="HTTP 500"):
        async for _ in _provider().model_stream(_native_request()):
            pass


@respx.mock
async def test_native_stream_raises_on_a_stream_error_frame():
    respx.post(_URL).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {"type": "response.output_text.delta", "delta": "partial"},
                {"type": "response.failed", "response": {"error": {"message": "upstream died"}}},
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    seen = []
    with pytest.raises(StreamError, match="upstream died"):
        async for delta in _provider().model_stream(_native_request()):
            seen.append(delta)
    # Pending deltas drain before the stored error surfaces.
    assert len(seen) == 1
