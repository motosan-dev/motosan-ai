from __future__ import annotations

import json
from collections.abc import AsyncIterator

import httpx
import pytest
import respx

from motosan_ai import Client
from motosan_ai.error import UnsupportedFeatureError
from motosan_ai.retry import RetryPolicy
from motosan_ai.types import (
    FreeformTool,
    FreeformToolFormat,
    Message,
    ModelChatRequest,
    ModelChatResponse,
    ModelStreamDelta,
    ModelStreamDone,
    ModelStreamText,
    ModelStreamToolCallDone,
    ModelStreamUsage,
    ModelToolCallFreeform,
    ModelToolSpecFreeform,
    StopReason,
    Usage,
)

FREEFORM_SPEC = ModelToolSpecFreeform(
    tool=FreeformTool(
        name="exec",
        description="Run JavaScript",
        format=FreeformToolFormat(type="grammar", syntax="lark", definition="start: source"),
    )
)


def _sse(*payloads: dict) -> str:
    return "\n".join(f"data: {json.dumps(p)}" for p in payloads) + "\n"


class _RecordingProvider:
    """Structurally-typed provider — deliberately NOT a BaseProvider subclass."""

    def __init__(self, capabilities=None) -> None:
        from motosan_ai.provider_base import ProviderCapabilities

        self.capabilities = capabilities or ProviderCapabilities.with_freeform_tools()
        self.seen: list[ModelChatRequest] = []

    async def model_chat(self, request: ModelChatRequest) -> ModelChatResponse:
        self.seen.append(request)
        return ModelChatResponse(content="native", model="", stop_reason=StopReason.end_turn)

    async def model_stream(self, request: ModelChatRequest) -> AsyncIterator[ModelStreamDelta]:
        self.seen.append(request)
        yield ModelStreamText(delta="nat")
        yield ModelStreamText(delta="ive")
        yield ModelStreamUsage(usage=Usage(input_tokens=1, output_tokens=2))
        yield ModelStreamDone(stop_reason=StopReason.end_turn)

    async def aclose(self) -> None:
        return None


class _NoNativeProvider:
    async def aclose(self) -> None:
        return None


def _client(provider_obj) -> Client:
    client = Client(provider="anthropic", api_key="k", model="client-model")
    client._provider = provider_obj
    return client


def test_openai_responses_api_flag_is_threaded_through_the_constructor():
    off = Client(provider="openai", api_key="k")
    on = Client(provider="openai", api_key="k", openai_responses_api=True)
    assert off._provider.responses_api is False
    assert on._provider.responses_api is True
    assert on._provider.capabilities.supports_freeform_tools is True


def test_openai_responses_api_flag_is_threaded_through_the_shortcut():
    assert Client.openai(api_key="k")._provider.responses_api is False
    assert Client.openai(api_key="k", openai_responses_api=True)._provider.responses_api is True


def test_ollama_over_openai_never_receives_the_responses_opt_in():
    client = Client(provider="ollama", model="llama3.2")
    assert client._provider.responses_api is False


async def test_model_chat_with_dispatches_and_backfills_the_model():
    provider = _RecordingProvider()
    client = _client(provider)

    response = await client.model_chat_with(
        ModelChatRequest.builder().message(Message.user("hi")).build()
    )

    assert response.content == "native"
    assert provider.seen[0].model == "client-model"


async def test_model_chat_with_keeps_an_explicit_request_model():
    provider = _RecordingProvider()
    response = await _client(provider).model_chat_with(
        ModelChatRequest.builder().model("explicit").message(Message.user("hi")).build()
    )
    assert provider.seen[0].model == "explicit"
    assert response.content == "native"


async def test_model_stream_with_yields_every_delta():
    deltas = [
        delta
        async for delta in _client(_RecordingProvider()).model_stream_with(
            ModelChatRequest.builder().message(Message.user("hi")).build()
        )
    ]
    assert deltas == [
        ModelStreamText(delta="nat"),
        ModelStreamText(delta="ive"),
        ModelStreamUsage(usage=Usage(input_tokens=1, output_tokens=2)),
        ModelStreamDone(stop_reason=StopReason.end_turn),
    ]


async def test_model_stream_collect_with_assembles_and_backfills_the_model():
    response = await _client(_RecordingProvider()).model_stream_collect_with(
        ModelChatRequest.builder().message(Message.user("hi")).build()
    )
    assert response.content == "native"
    assert response.usage == Usage(input_tokens=1, output_tokens=2)
    assert response.stop_reason == StopReason.end_turn
    assert response.model == "client-model"


async def test_native_dispatch_is_duck_typed_not_isinstance_based():
    client = _client(_NoNativeProvider())
    request = ModelChatRequest.builder().message(Message.user("hi")).build()

    with pytest.raises(UnsupportedFeatureError, match="native model requests"):
        await client.model_chat_with(request)
    with pytest.raises(UnsupportedFeatureError, match="native model streams"):
        async for _ in client.model_stream_with(request):
            pass


async def test_capabilities_are_enforced_before_native_dispatch():
    from motosan_ai.provider_base import ProviderCapabilities

    provider = _RecordingProvider(capabilities=ProviderCapabilities.with_image())
    client = _client(provider)
    request = (
        ModelChatRequest.builder().message(Message.user("hi")).tool_spec(FREEFORM_SPEC).build()
    )

    with pytest.raises(UnsupportedFeatureError, match="native freeform tools"):
        await client.model_chat_with(request)
    with pytest.raises(UnsupportedFeatureError, match="native freeform tools"):
        async for _ in client.model_stream_with(request):
            pass
    assert provider.seen == []


async def test_provider_without_capabilities_is_not_validated():
    # The LlmClient Protocol does not require `capabilities`; native
    # validation must be skipped, not crash, for such providers.
    provider = _RecordingProvider()
    del provider.capabilities
    client = _client(provider)
    response = await client.model_chat_with(
        ModelChatRequest.builder().tool_spec(FREEFORM_SPEC).build()
    )
    assert response.content == "native"


@respx.mock
async def test_end_to_end_over_the_chatgpt_codex_provider():
    respx.post("https://chatgpt.com/backend-api/codex/responses").mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {
                    "type": "response.output_item.done",
                    "item": {
                        "type": "custom_tool_call",
                        "call_id": "call_js",
                        "name": "exec",
                        "input": "text('captured');",
                    },
                },
                {
                    "type": "response.completed",
                    "response": {
                        "status": "completed",
                        "usage": {"input_tokens": 4, "output_tokens": 5},
                    },
                },
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    client = Client.chatgpt_codex(
        access_token="tok",
        account_id="acct-123",
        model="gpt-5.5",
        retry_policy=RetryPolicy(max_retries=0),
    )
    response = await client.model_stream_collect_with(
        ModelChatRequest.builder().message(Message.user("run js")).tool_spec(FREEFORM_SPEC).build()
    )

    assert response.tool_calls == [
        ModelToolCallFreeform(id="call_js", name="exec", input="text('captured');")
    ]
    assert response.model == "gpt-5.5"
    assert response.stop_reason == StopReason.tool_use


async def test_native_stream_does_not_strip_think_tags():
    class _ThinkProvider(_RecordingProvider):
        async def model_stream(self, request: ModelChatRequest) -> AsyncIterator[ModelStreamDelta]:
            yield ModelStreamText(delta="<think>secret</think>visible")
            yield ModelStreamDone(stop_reason=StopReason.end_turn)

    response = await _client(_ThinkProvider()).model_stream_collect_with(
        ModelChatRequest.builder().message(Message.user("hi")).build()
    )
    # The native path carries thinking as explicit deltas, so text passes
    # through untouched — unlike Client.stream_with.
    assert response.content == "<think>secret</think>visible"


async def test_tool_call_done_survives_client_level_collection():
    class _ToolProvider(_RecordingProvider):
        async def model_stream(self, request: ModelChatRequest) -> AsyncIterator[ModelStreamDelta]:
            yield ModelStreamToolCallDone(
                call=ModelToolCallFreeform(id="c", name="exec", input="raw();")
            )
            yield ModelStreamDone(stop_reason=StopReason.tool_use)

    response = await _client(_ToolProvider()).model_stream_collect_with(
        ModelChatRequest.builder().build()
    )
    assert response.tool_calls == [ModelToolCallFreeform(id="c", name="exec", input="raw();")]
