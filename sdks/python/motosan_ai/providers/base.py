from typing import AsyncIterator, Protocol, runtime_checkable
from motosan_ai.types import ChatRequest, ChatResponse, StreamEvent


@runtime_checkable
class ProviderProtocol(Protocol):
    async def chat(self, req: ChatRequest) -> ChatResponse: ...
    async def stream(self, req: ChatRequest) -> AsyncIterator[StreamEvent]: ...
