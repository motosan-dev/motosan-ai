from __future__ import annotations

from collections.abc import AsyncIterator

import pytest

from motosan_ai.error import InvalidRequestError, UnsupportedFeatureError
from motosan_ai.provider_base import BaseProvider, ProviderCapabilities, validate_model_request
from motosan_ai.types import (
    ChatRequest,
    ChatResponse,
    FreeformTool,
    FreeformToolFormat,
    FunctionCallOutputText,
    Message,
    ModelChatRequest,
    ModelContextToolCall,
    ModelToolCallFreeform,
    ModelToolCallFunction,
    ModelToolOutputCustom,
    ModelToolOutputFunction,
    ModelToolSpecFreeform,
    ModelToolSpecFunction,
    StreamEvent,
    Tool,
)

FREEFORM_SPEC = ModelToolSpecFreeform(
    tool=FreeformTool(
        name="exec",
        description="Run JavaScript",
        format=FreeformToolFormat(type="grammar", syntax="lark", definition="start: source"),
    )
)


def test_with_freeform_tools_constructor():
    caps = ProviderCapabilities.with_freeform_tools()
    assert caps.supports_image is False
    assert caps.supports_document is False
    assert caps.supports_freeform_tools is True


def test_with_image_and_freeform_tools_constructor():
    caps = ProviderCapabilities.with_image_and_freeform_tools()
    assert caps.supports_image is True
    assert caps.supports_document is False
    assert caps.supports_freeform_tools is True


def test_full_deliberately_leaves_freeform_false():
    # Rust parity: full() is image + document, never freeform.
    assert ProviderCapabilities.full().supports_freeform_tools is False
    assert ProviderCapabilities.text_only().supports_freeform_tools is False
    assert ProviderCapabilities.with_image().supports_freeform_tools is False


def test_freeform_field_defaults_to_false_for_positional_construction():
    assert ProviderCapabilities(supports_image=True, supports_document=True) == (
        ProviderCapabilities.full()
    )


def test_rejects_freeform_spec_when_unsupported():
    request = ModelChatRequest.builder().tool_spec(FREEFORM_SPEC).build()
    with pytest.raises(UnsupportedFeatureError, match="native freeform tools"):
        validate_model_request(request, ProviderCapabilities.with_image())


def test_rejects_freeform_history_call_when_unsupported():
    request = (
        ModelChatRequest.builder()
        .tool_call(ModelToolCallFreeform(id="call_js", name="exec", input="x"))
        .build()
    )
    with pytest.raises(UnsupportedFeatureError, match="native freeform tools"):
        validate_model_request(request, ProviderCapabilities.full())


def test_rejects_custom_history_output_when_unsupported():
    request = (
        ModelChatRequest.builder()
        .tool_output(
            ModelToolOutputCustom(call_id="call_js", output=FunctionCallOutputText(text="1"))
        )
        .build()
    )
    with pytest.raises(UnsupportedFeatureError, match="native freeform tools"):
        validate_model_request(request, ProviderCapabilities.text_only())


def test_accepts_freeform_when_supported():
    request = (
        ModelChatRequest.builder()
        .tool_spec(FREEFORM_SPEC)
        .tool_call(ModelToolCallFreeform(id="call_js", name="exec", input="x"))
        .tool_output(
            ModelToolOutputCustom(call_id="call_js", output=FunctionCallOutputText(text="1"))
        )
        .build()
    )
    validate_model_request(request, ProviderCapabilities.with_freeform_tools())


def test_function_only_history_is_accepted_everywhere():
    request = (
        ModelChatRequest.builder()
        .tool_spec(ModelToolSpecFunction(tool=Tool(name="sum")))
        .tool_call(ModelToolCallFunction(id="call_fn", name="sum", arguments="{}"))
        .tool_output(
            ModelToolOutputFunction(call_id="call_fn", output=FunctionCallOutputText(text="1"))
        )
        .message(Message.user("hi"))
        .build()
    )
    validate_model_request(request, ProviderCapabilities.text_only())


def test_rejects_image_and_document_context_blocks():
    image = (
        ModelChatRequest.builder()
        .message(Message.user_with_image("look", "abc", "image/png"))
        .build()
    )
    with pytest.raises(UnsupportedFeatureError, match="image"):
        validate_model_request(image, ProviderCapabilities.with_freeform_tools())
    validate_model_request(image, ProviderCapabilities.with_image_and_freeform_tools())

    document = (
        ModelChatRequest.builder().message(Message.user_with_pdf_base64("read", "abc")).build()
    )
    with pytest.raises(UnsupportedFeatureError, match="document"):
        validate_model_request(document, ProviderCapabilities.with_image())
    validate_model_request(document, ProviderCapabilities.full())


def test_rejection_is_catchable_as_invalid_request_error():
    request = ModelChatRequest.builder().tool_spec(FREEFORM_SPEC).build()
    with pytest.raises(InvalidRequestError):
        validate_model_request(request, ProviderCapabilities.text_only())


def test_base_provider_method_delegates_to_its_capabilities():
    class _Freeform(BaseProvider):
        capabilities = ProviderCapabilities.with_freeform_tools()

        async def chat(self, request: ChatRequest) -> ChatResponse:  # pragma: no cover
            raise NotImplementedError

        async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
            if False:  # pragma: no cover
                yield StreamEvent(content="", done=True)
            raise NotImplementedError

    class _TextOnly(_Freeform):
        capabilities = ProviderCapabilities.text_only()

    request = ModelChatRequest.builder().tool_spec(FREEFORM_SPEC).build()
    _Freeform().validate_model_request(request)
    with pytest.raises(UnsupportedFeatureError, match="native freeform tools"):
        _TextOnly().validate_model_request(request)
