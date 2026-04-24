import json

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message, SystemBlock, Tool, ToolCall


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
async def test_cached_plain_user_wraps_in_block_with_cache_control(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    await provider.chat(ChatRequest(messages=[Message.user_with_cache("cache me")]))

    body = json.loads(route.calls[0].request.content)
    assert body["messages"][0] == {
        "role": "user",
        "content": [{"type": "text", "text": "cache me", "cache_control": {"type": "ephemeral"}}],
    }


@respx.mock
@pytest.mark.asyncio
async def test_cached_user_with_image_tags_last_block(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    msg = Message.user_with_image("look", "abc", "image/png").with_cache()
    await provider.chat(ChatRequest(messages=[msg]))

    body = json.loads(route.calls[0].request.content)
    blocks = body["messages"][0]["content"]
    assert blocks[0] == {"type": "text", "text": "look"}
    assert blocks[1]["cache_control"] == {"type": "ephemeral"}


@respx.mock
@pytest.mark.asyncio
async def test_cached_assistant_with_tool_calls_tags_last_block(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    tc = ToolCall(id="toolu_1", name="x", input={})
    msg = Message.assistant_with_tool_calls("thinking", [tc])
    msg.cache = True
    await provider.chat(ChatRequest(messages=[msg]))

    body = json.loads(route.calls[0].request.content)
    blocks = body["messages"][0]["content"]
    assert blocks[-1]["type"] == "tool_use"
    assert blocks[-1]["cache_control"] == {"type": "ephemeral"}


@respx.mock
@pytest.mark.asyncio
async def test_uncached_message_has_no_cache_control(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    await provider.chat(ChatRequest(messages=[Message.user("plain")]))

    body = json.loads(route.calls[0].request.content)
    assert "cache_control" not in json.dumps(body)


@respx.mock
@pytest.mark.asyncio
async def test_system_blocks_serialized_as_array(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        system_blocks=[SystemBlock.cached("Base"), SystemBlock.new("Dynamic")],
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["system"] == [
        {"type": "text", "text": "Base", "cache_control": {"type": "ephemeral"}},
        {"type": "text", "text": "Dynamic"},
    ]


@respx.mock
@pytest.mark.asyncio
async def test_system_cache_wraps_plain_string(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(messages=[Message.user("hi")], system="You are helpful.", system_cache=True)
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["system"] == [
        {
            "type": "text",
            "text": "You are helpful.",
            "cache_control": {"type": "ephemeral"},
        }
    ]


@respx.mock
@pytest.mark.asyncio
async def test_plain_system_unchanged_regression(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(messages=[Message.user("hi")], system="You are helpful.")
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["system"] == "You are helpful."


@respx.mock
@pytest.mark.asyncio
async def test_system_blocks_take_priority_over_plain_system(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        system="IGNORED",
        system_blocks=[SystemBlock.new("WINS")],
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["system"] == [{"type": "text", "text": "WINS"}]


@respx.mock
@pytest.mark.asyncio
async def test_tool_cache_flag_emits_cache_control(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    tools = [Tool(name="a", description="A"), Tool(name="b", description="B", cache=True)]
    req = ChatRequest(messages=[Message.user("hi")], tools=tools)
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert "cache_control" not in body["tools"][0]
    assert body["tools"][1]["cache_control"] == {"type": "ephemeral"}


@respx.mock
@pytest.mark.asyncio
async def test_cache_usage_tokens_parsed(provider):
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200,
            json={
                "model": "claude-sonnet-4-6",
                "stop_reason": "end_turn",
                "usage": {
                    "input_tokens": 100,
                    "output_tokens": 20,
                    "cache_creation_input_tokens": 50,
                    "cache_read_input_tokens": 200,
                },
                "content": [{"type": "text", "text": "ok"}],
            },
        )
    )
    resp = await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    assert resp.usage.input_tokens == 100
    assert resp.usage.output_tokens == 20
    assert resp.usage.cache_creation_input_tokens == 50
    assert resp.usage.cache_read_input_tokens == 200
