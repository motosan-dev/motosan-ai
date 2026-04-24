import json

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message, StopReason


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


def _response(reason: str = "end_turn") -> httpx.Response:
    return httpx.Response(
        200,
        json={
            "model": "claude-sonnet-4-6",
            "stop_reason": reason,
            "usage": {"input_tokens": 1, "output_tokens": 1},
            "content": [{"type": "text", "text": "hi"}],
        },
    )


@respx.mock
@pytest.mark.asyncio
async def test_stop_sequences_serialized(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=_response("stop_sequence")
    )
    req = ChatRequest(messages=[Message.user("hi")], stop_sequences=["END", "STOP"])
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["stop_sequences"] == ["END", "STOP"]


@respx.mock
@pytest.mark.asyncio
async def test_stop_sequence_reason_parsed(provider):
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=_response("stop_sequence")
    )
    resp = await provider.chat(ChatRequest(messages=[Message.user("hi")], stop_sequences=["END"]))
    assert resp.stop_reason == StopReason.stop_sequence


@respx.mock
@pytest.mark.asyncio
async def test_empty_stop_sequences_omitted(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_response())
    await provider.chat(ChatRequest(messages=[Message.user("hi")], stop_sequences=[]))
    body = json.loads(route.calls[0].request.content)
    assert "stop_sequences" not in body
