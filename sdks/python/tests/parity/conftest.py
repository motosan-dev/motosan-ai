from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Any

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.providers.minimax import MinimaxProvider
from motosan_ai.providers.openai import OpenAIProvider


@dataclass
class ProviderUnderTest:
    name: str
    provider: Any
    endpoint: str
    stream_endpoint: str
    ok_response: dict[str, Any]


_OK_ANTHROPIC = {
    "model": "claude-sonnet-4-6",
    "stop_reason": "end_turn",
    "usage": {"input_tokens": 1, "output_tokens": 1},
    "content": [{"type": "text", "text": "ok"}],
}

_OK_OPENAI = {
    "id": "chatcmpl-1",
    "object": "chat.completion",
    "model": "gpt-4o",
    "choices": [
        {"message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop", "index": 0}
    ],
    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
}

_OK_GEMINI = {
    "candidates": [{"content": {"parts": [{"text": "ok"}]}, "finishReason": "STOP"}],
    "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1},
    "modelVersion": "gemini-2.5-flash",
}

_OK_MINIMAX = {
    "id": "msg_1",
    "choices": [
        {"message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop", "index": 0}
    ],
    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
}


@pytest.fixture(params=["anthropic", "openai", "gemini", "minimax"])
def provider_under_test(request) -> ProviderUnderTest:
    name = request.param
    if name == "anthropic":
        return ProviderUnderTest(
            name=name,
            provider=AnthropicProvider("test-key", base_url="https://mock.anthropic.com"),
            endpoint="https://mock.anthropic.com/v1/messages",
            stream_endpoint="https://mock.anthropic.com/v1/messages",
            ok_response=_OK_ANTHROPIC,
        )
    if name == "openai":
        return ProviderUnderTest(
            name=name,
            provider=OpenAIProvider("test-key", base_url="https://mock.openai.com"),
            endpoint="https://mock.openai.com/v1/chat/completions",
            stream_endpoint="https://mock.openai.com/v1/chat/completions",
            ok_response=_OK_OPENAI,
        )
    if name == "gemini":
        return ProviderUnderTest(
            name=name,
            provider=GeminiProvider("test-key", base_url="https://mock.gemini.com"),
            endpoint="https://mock.gemini.com/models/gemini-2.5-flash:generateContent",
            stream_endpoint="https://mock.gemini.com/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            ok_response=_OK_GEMINI,
        )
    if name == "minimax":
        return ProviderUnderTest(
            name=name,
            provider=MinimaxProvider("test-key", base_url="https://mock.minimax.com"),
            endpoint="https://mock.minimax.com/v1/text/chatcompletion_v2",
            stream_endpoint="https://mock.minimax.com/v1/text/chatcompletion_v2",
            ok_response=_OK_MINIMAX,
        )
    raise AssertionError(f"unknown provider {name}")


async def capture_chat_body(provider: ProviderUnderTest, request) -> dict[str, Any]:
    route = respx.post(provider.endpoint).mock(
        return_value=httpx.Response(200, json=provider.ok_response)
    )
    await provider.provider.chat(request)
    return json.loads(route.calls[0].request.content)
