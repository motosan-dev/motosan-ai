"""OpenAI provider. Requires: pip install motosan-ai[openai]"""
from __future__ import annotations
from typing import AsyncIterator

from motosan_ai.types import ChatRequest, ChatResponse, StreamEvent
from motosan_ai.error import AuthError, ProviderError, RateLimitError

DEFAULT_MODEL = "gpt-4o"
MAX_TOKENS_DEFAULT = 1024


def _check_import() -> None:
    try:
        import openai  # noqa: F401
    except ImportError:
        raise ImportError(
            "The 'openai' package is required for OpenAIProvider.\n"
            "Install it with: pip install 'motosan-ai[openai]'"
        )


class OpenAIProvider:
    def __init__(self, api_key: str):
        _check_import()
        import openai
        self._client = openai.AsyncOpenAI(api_key=api_key)

    async def chat(self, req: ChatRequest) -> ChatResponse:
        # TODO: implement — tracked in issue #14
        raise NotImplementedError("OpenAI provider not yet implemented")

    async def stream(self, req: ChatRequest) -> AsyncIterator[StreamEvent]:
        # TODO: implement — tracked in issue #15
        raise NotImplementedError("OpenAI streaming not yet implemented")
        yield  # make this an async generator
