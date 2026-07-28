from __future__ import annotations

from abc import ABC, abstractmethod
from collections.abc import AsyncIterator
from dataclasses import dataclass

from motosan_ai.error import InvalidRequestError, UnsupportedFeatureError
from motosan_ai.types import (
    ChatRequest,
    ChatResponse,
    DocumentBlock,
    ImageBlock,
    ModelChatRequest,
    ModelContextMessage,
    ModelContextToolCall,
    ModelContextToolOutput,
    ModelToolCallFreeform,
    ModelToolOutputCustom,
    ModelToolSpecFreeform,
    StreamEvent,
)


@dataclass(frozen=True)
class ProviderCapabilities:
    supports_image: bool
    supports_document: bool
    # Defaulted so existing two-argument construction keeps working.
    supports_freeform_tools: bool = False

    @classmethod
    def text_only(cls) -> ProviderCapabilities:
        return cls(supports_image=False, supports_document=False, supports_freeform_tools=False)

    @classmethod
    def with_image(cls) -> ProviderCapabilities:
        return cls(supports_image=True, supports_document=False, supports_freeform_tools=False)

    @classmethod
    def with_freeform_tools(cls) -> ProviderCapabilities:
        return cls(supports_image=False, supports_document=False, supports_freeform_tools=True)

    @classmethod
    def with_image_and_freeform_tools(cls) -> ProviderCapabilities:
        return cls(supports_image=True, supports_document=False, supports_freeform_tools=True)

    @classmethod
    def full(cls) -> ProviderCapabilities:
        # Rust parity: full() is image + document and deliberately leaves
        # freeform False. A provider that claimed freeform support it lacks
        # is exactly what this flag exists to prevent.
        return cls(supports_image=True, supports_document=True, supports_freeform_tools=False)


def validate_request(request: ChatRequest, capabilities: ProviderCapabilities) -> None:
    """Raise InvalidRequestError for content blocks the capabilities do not support.

    Central choke point mirroring Rust's ``validate_for_dispatch`` and the TS
    ``validateRequest``: runs before any network/CLI dispatch.
    """
    for message in request.messages:
        for block in message.content_blocks:
            if isinstance(block, ImageBlock) and not capabilities.supports_image:
                raise InvalidRequestError("provider does not support image input")
            if isinstance(block, DocumentBlock) and not capabilities.supports_document:
                raise InvalidRequestError("provider does not support document input")


def validate_model_request(request: ModelChatRequest, capabilities: ProviderCapabilities) -> None:
    """Reject native model requests the capabilities do not support, pre-network.

    Mirrors Rust ``ProviderImpl::validate_model_request`` minus the three
    reject-only fields (thinking / mcp_servers / mcp_tool_configs), which the
    Python ``ModelChatRequest`` deliberately does not carry (milestone D3).
    """
    has_freeform_spec = any(isinstance(spec, ModelToolSpecFreeform) for spec in request.tool_specs)
    has_freeform_history = any(
        (isinstance(item, ModelContextToolCall) and isinstance(item.call, ModelToolCallFreeform))
        or (
            isinstance(item, ModelContextToolOutput)
            and isinstance(item.output, ModelToolOutputCustom)
        )
        for item in request.context
    )
    if (has_freeform_spec or has_freeform_history) and not capabilities.supports_freeform_tools:
        raise UnsupportedFeatureError("provider does not support native freeform tools")

    for item in request.context:
        if not isinstance(item, ModelContextMessage):
            continue
        for block in item.message.content_blocks:
            if isinstance(block, ImageBlock) and not capabilities.supports_image:
                raise UnsupportedFeatureError("provider does not support image input")
            if isinstance(block, DocumentBlock) and not capabilities.supports_document:
                raise UnsupportedFeatureError("provider does not support document input")


class BaseProvider(ABC):
    capabilities: ProviderCapabilities = ProviderCapabilities.text_only()

    def validate_request(self, request: ChatRequest) -> None:
        validate_request(request, self.capabilities)

    def validate_model_request(self, request: ModelChatRequest) -> None:
        validate_model_request(request, self.capabilities)

    @abstractmethod
    async def chat(self, request: ChatRequest) -> ChatResponse: ...

    @abstractmethod
    def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]: ...
