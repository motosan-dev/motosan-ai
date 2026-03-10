"""MiniMax provider. Uses httpx (no extra install needed)."""
from __future__ import annotations
from typing import AsyncIterator

from motosan_ai.types import ChatRequest, ChatResponse, StreamEvent

DEFAULT_MODEL = "MiniMax-Text-01"
MAX_TOKENS_DEFAULT = 1024
API_BASE = "https://api.minimax.chat/v1"


class MinimaxProvider:
    def __init__(self, api_key: str):
        import httpx
        self._api_key = api_key
        self._client = httpx.AsyncClient(
            base_url=API_BASE,
            headers={"Authorization": f"Bearer {api_key}"},
            timeout=60.0,
        )

    async def chat(self, req: ChatRequest) -> ChatResponse:
        # TODO: implement — tracked in issue #16
        raise NotImplementedError("MiniMax provider not yet implemented")

    async def stream(self, req: ChatRequest) -> AsyncIterator[StreamEvent]:
        # TODO: implement — tracked in issue #17
        raise NotImplementedError("MiniMax streaming not yet implemented")
        yield  # make this an async generator
