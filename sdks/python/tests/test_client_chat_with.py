from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai import Client, Provider
from motosan_ai.types import ChatRequest, Message, SystemBlock, ToolChoice

_RESPONSE = {
    "model": "claude-sonnet-4-6",
    "stop_reason": "end_turn",
    "usage": {"input_tokens": 1, "output_tokens": 1},
    "content": [{"type": "text", "text": "ok"}],
}


def _mock_anthropic():
    return respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, json=_RESPONSE)
    )


@respx.mock
@pytest.mark.asyncio
async def test_chat_with_passes_thinking_to_provider(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = _mock_anthropic()
    client = Client(provider=Provider.anthropic)
    req = ChatRequest.builder().message(Message.user("hi")).thinking(2048).build()
    await client.chat_with(req)
    body = json.loads(route.calls[0].request.content)
    assert body["thinking"] == {
        "type": "enabled",
        "budget_tokens": 2048,
        "display": "summarized",
    }


@respx.mock
@pytest.mark.asyncio
async def test_chat_with_passes_system_blocks(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = _mock_anthropic()
    client = Client(provider=Provider.anthropic)
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .system_block(SystemBlock.cached("Base"))
        .system_block(SystemBlock.new("Dynamic"))
        .build()
    )
    await client.chat_with(req)
    body = json.loads(route.calls[0].request.content)
    assert body["system"] == [
        {"type": "text", "text": "Base", "cache_control": {"type": "ephemeral"}},
        {"type": "text", "text": "Dynamic"},
    ]


@respx.mock
@pytest.mark.asyncio
async def test_chat_with_passes_tool_choice(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = _mock_anthropic()
    client = Client(provider=Provider.anthropic)
    req = (
        ChatRequest.builder().message(Message.user("hi")).tool_choice(ToolChoice.required()).build()
    )
    await client.chat_with(req)
    body = json.loads(route.calls[0].request.content)
    assert body["tool_choice"] == {"type": "any"}


@respx.mock
@pytest.mark.asyncio
async def test_chat_with_falls_back_to_client_model_when_request_omits(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = _mock_anthropic()
    client = Client(provider=Provider.anthropic, model="claude-haiku-4-5-20251001")
    req = ChatRequest.builder().message(Message.user("hi")).build()
    await client.chat_with(req)
    body = json.loads(route.calls[0].request.content)
    assert body["model"] == "claude-haiku-4-5-20251001"


@respx.mock
@pytest.mark.asyncio
async def test_chat_with_request_model_overrides_client_default(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = _mock_anthropic()
    client = Client(provider=Provider.anthropic, model="default-model")
    req = ChatRequest.builder().message(Message.user("hi")).model("override-model").build()
    await client.chat_with(req)
    body = json.loads(route.calls[0].request.content)
    assert body["model"] == "override-model"


@respx.mock
@pytest.mark.asyncio
async def test_chat_kwargs_path_unchanged_regression(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = _mock_anthropic()
    client = Client(provider=Provider.anthropic)
    resp = await client.chat(
        [Message.user("hi")],
        system="Be terse.",
        temperature=0.3,
        max_tokens=100,
    )
    assert resp.content == "ok"
    body = json.loads(route.calls[0].request.content)
    assert body["system"] == "Be terse."
    assert body["temperature"] == 0.3
    assert body["max_tokens"] == 100
