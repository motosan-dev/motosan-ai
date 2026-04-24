import json

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message, Tool, ToolChoice


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


def _ok() -> httpx.Response:
    return httpx.Response(
        200,
        json={
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 1},
            "content": [{"type": "text", "text": "ok"}],
        },
    )


@respx.mock
@pytest.mark.asyncio
async def test_tool_choice_auto_serializes_auto(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")], tools=[Tool(name="x")], tool_choice=ToolChoice.auto()
    )
    await provider.chat(req)
    body = json.loads(route.calls[0].request.content)
    assert body["tool_choice"] == {"type": "auto"}


@respx.mock
@pytest.mark.asyncio
async def test_tool_choice_required_serializes_any(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")], tools=[Tool(name="x")], tool_choice=ToolChoice.required()
    )
    await provider.chat(req)
    body = json.loads(route.calls[0].request.content)
    assert body["tool_choice"] == {"type": "any"}


@respx.mock
@pytest.mark.asyncio
async def test_tool_choice_none_removes_tools(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")], tools=[Tool(name="x")], tool_choice=ToolChoice.none()
    )
    await provider.chat(req)
    body = json.loads(route.calls[0].request.content)
    assert "tools" not in body
    assert "tool_choice" not in body


@respx.mock
@pytest.mark.asyncio
async def test_tool_choice_tool_name(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        tools=[Tool(name="get_weather")],
        tool_choice=ToolChoice.tool("get_weather"),
    )
    await provider.chat(req)
    body = json.loads(route.calls[0].request.content)
    assert body["tool_choice"] == {"type": "tool", "name": "get_weather"}
