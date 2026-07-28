from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai.error import (
    IncompleteStreamError,
    ProviderError,
    StreamError,
    UnsupportedFeatureError,
)
from motosan_ai.provider_base import ProviderCapabilities
from motosan_ai.providers.openai import OpenAIProvider
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
)

_RESPONSES = "https://api.openai.com/v1/responses"

FREEFORM_SPEC = ModelToolSpecFreeform(
    tool=FreeformTool(
        name="exec",
        description="Run JavaScript",
        format=FreeformToolFormat(type="grammar", syntax="lark", definition="start: source"),
    )
)


def _native_provider() -> OpenAIProvider:
    return OpenAIProvider(api_key="test-key", model="gpt-5.5-codex", responses_api=True)


def _native_request() -> ModelChatRequest:
    return (
        ModelChatRequest.builder()
        .model("gpt-5.5-codex")
        .message(Message.user("run js"))
        .tool_spec(FREEFORM_SPEC)
        .build()
    )


def _sse(*payloads: dict) -> str:
    return "\n".join(f"data: {json.dumps(p)}" for p in payloads) + "\n"


def test_capabilities_switch_on_the_opt_in():
    assert OpenAIProvider(api_key="k").capabilities == ProviderCapabilities.with_image()
    assert (
        OpenAIProvider(api_key="k", responses_api=True).capabilities
        == ProviderCapabilities.with_image_and_freeform_tools()
    )


def test_responses_endpoint_defaults_and_override():
    assert OpenAIProvider(api_key="k")._responses_endpoint() == _RESPONSES
    assert (
        OpenAIProvider(api_key="k", base_url="https://proxy.test/")._responses_endpoint()
        == "https://proxy.test/v1/responses"
    )
    assert (
        OpenAIProvider(
            api_key="k", responses_url="https://mock.test/v1/responses/"
        )._responses_endpoint()
        == "https://mock.test/v1/responses"
    )
    # The chat endpoint is untouched by the opt-in.
    assert OpenAIProvider(api_key="k")._endpoint() == "https://api.openai.com/v1/chat/completions"


@respx.mock
async def test_chat_completions_rejects_freeform_before_any_http():
    route = respx.post(host="api.openai.com").mock(return_value=httpx.Response(500))
    with pytest.raises(UnsupportedFeatureError, match="freeform"):
        await OpenAIProvider(api_key="k").model_chat(_native_request())
    assert route.call_count == 0


@respx.mock
async def test_chat_completions_rejects_freeform_streams_before_any_http():
    route = respx.post(host="api.openai.com").mock(return_value=httpx.Response(500))
    with pytest.raises(UnsupportedFeatureError, match="freeform"):
        async for _ in OpenAIProvider(api_key="k").model_stream(_native_request()):
            pass
    assert route.call_count == 0


@respx.mock
async def test_chat_completions_rejects_plain_native_requests_with_the_opt_in_message():
    route = respx.post(host="api.openai.com").mock(return_value=httpx.Response(500))
    plain = ModelChatRequest.builder().message(Message.user("hi")).build()
    with pytest.raises(UnsupportedFeatureError, match="enable OpenAI Responses API"):
        await OpenAIProvider(api_key="k").model_chat(plain)
    with pytest.raises(UnsupportedFeatureError, match="enable OpenAI Responses API"):
        async for _ in OpenAIProvider(api_key="k").model_stream(plain):
            pass
    assert route.call_count == 0


@respx.mock
async def test_native_chat_posts_a_non_streaming_body_and_decodes_custom_calls():
    captured: dict = {}
    raw = "const x = {a: 1};\nconsole.log(x.a);\n"

    def _capture(request):
        captured["body"] = json.loads(request.content)
        captured["headers"] = dict(request.headers)
        return httpx.Response(
            200,
            json={
                "model": "gpt-5.5-codex",
                "status": "completed",
                "output": [
                    {
                        "type": "custom_tool_call",
                        "call_id": "call_js",
                        "name": "exec",
                        "input": raw,
                    }
                ],
                "usage": {"input_tokens": 9, "output_tokens": 7},
            },
        )

    respx.post(_RESPONSES).mock(side_effect=_capture)

    response = await _native_provider().model_chat(_native_request())

    assert response.tool_calls == [ModelToolCallFreeform(id="call_js", name="exec", input=raw)]
    assert response.stop_reason == StopReason.tool_use
    assert response.usage.input_tokens == 9
    assert captured["headers"]["authorization"] == "Bearer test-key"
    assert "stream" not in captured["body"]
    assert captured["body"]["tools"][0]["type"] == "custom"
    assert captured["body"]["tools"][0]["format"]["definition"] == "start: source"


@respx.mock
async def test_native_chat_encodes_image_blocks():
    captured: dict = {}

    def _capture(request):
        captured["body"] = json.loads(request.content)
        return httpx.Response(
            200,
            json={
                "model": "gpt-5.5-codex",
                "status": "completed",
                "output": [
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": "ok"}],
                    }
                ],
                "usage": {"input_tokens": 1, "output_tokens": 1},
            },
        )

    respx.post(_RESPONSES).mock(side_effect=_capture)
    request = (
        ModelChatRequest.builder()
        .message(Message.user_with_image("inspect", "abc123", "image/png"))
        .build()
    )

    response = await _native_provider().model_chat(request)

    assert response.content == "ok"
    content = captured["body"]["input"][0]["content"]
    assert content[0] == {"type": "input_text", "text": "inspect"}
    assert content[1] == {"type": "input_image", "image_url": "data:image/png;base64,abc123"}


@respx.mock
async def test_native_chat_replays_symmetric_history_byte_exact():
    captured: dict = {}
    raw = '{"this":"looks like json"}\nconsole.log(\'but is JS\');'

    def _capture(request):
        captured["body"] = json.loads(request.content)
        return httpx.Response(
            200,
            json={
                "model": "gpt-5.5-codex",
                "status": "completed",
                "output": [
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": "ok"}],
                    }
                ],
                "usage": {"input_tokens": 1, "output_tokens": 1},
            },
        )

    respx.post(_RESPONSES).mock(side_effect=_capture)

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
    response = await _native_provider().model_chat(request)

    assert response.content == "ok"
    body = captured["body"]
    assert [item["type"] for item in body["input"]] == [
        "message",
        "custom_tool_call",
        "custom_tool_call_output",
    ]
    assert body["input"][1]["input"] == raw
    assert body["input"][1]["call_id"] == "call_js"
    assert body["input"][1]["name"] == "exec"


@respx.mock
async def test_native_chat_maps_http_errors():
    respx.post(_RESPONSES).mock(return_value=httpx.Response(500, text="boom"))
    with pytest.raises(ProviderError, match="HTTP 500"):
        await _native_provider().model_chat(_native_request())


@respx.mock
async def test_native_stream_decodes_custom_delta_and_done():
    from motosan_ai._stream_collect import collect_model_stream

    respx.post(_RESPONSES).mock(
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

    response = await collect_model_stream(_native_provider().model_stream(_native_request()))

    assert response.tool_calls == [
        ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);\n")
    ]
    assert response.stop_reason == StopReason.tool_use
    assert response.usage.output_tokens == 3


@respx.mock
async def test_native_stream_sets_the_stream_flag():
    captured: dict = {}

    def _capture(request):
        captured["body"] = json.loads(request.content)
        return httpx.Response(
            200,
            text=_sse({"type": "response.completed", "response": {"status": "completed"}}),
            headers={"content-type": "text/event-stream"},
        )

    respx.post(_RESPONSES).mock(side_effect=_capture)
    async for _ in _native_provider().model_stream(_native_request()):
        pass
    assert captured["body"]["stream"] is True


@respx.mock
async def test_native_stream_eof_without_terminal_is_incomplete():
    respx.post(_RESPONSES).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {"type": "response.output_text.delta", "delta": "hel"},
                {"type": "response.output_text.delta", "delta": "lo"},
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    seen = []
    with pytest.raises(
        IncompleteStreamError, match="incomplete stream: openai ended without a terminal event"
    ):
        async for delta in _native_provider().model_stream(_native_request()):
            seen.append(delta)
    assert len(seen) == 2
    assert issubclass(IncompleteStreamError, StreamError)
