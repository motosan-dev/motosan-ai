from __future__ import annotations

from abc import ABC, abstractmethod
from collections.abc import AsyncIterator
from dataclasses import dataclass

from motosan_ai.error import InvalidRequestError
from motosan_ai.types import ChatRequest, ChatResponse, DocumentBlock, ImageBlock, StreamEvent


@dataclass(frozen=True)
class ProviderCapabilities:
    supports_image: bool
    supports_document: bool

    @classmethod
    def text_only(cls) -> ProviderCapabilities:
        return cls(supports_image=False, supports_document=False)

    @classmethod
    def with_image(cls) -> ProviderCapabilities:
        return cls(supports_image=True, supports_document=False)

    @classmethod
    def full(cls) -> ProviderCapabilities:
        return cls(supports_image=True, supports_document=True)


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


class BaseProvider(ABC):
    capabilities: ProviderCapabilities = ProviderCapabilities.text_only()

    def validate_request(self, request: ChatRequest) -> None:
        validate_request(request, self.capabilities)

    @abstractmethod
    async def chat(self, request: ChatRequest) -> ChatResponse: ...

    @abstractmethod
    def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]: ...
