import json

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, McpServerConfig, McpToolConfigAllowed, Message


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
async def test_mcp_server_config_serialized(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        mcp_servers=[McpServerConfig(url="https://mcp.x.com", name="srv")],
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["mcp_servers"] == [{"type": "url", "url": "https://mcp.x.com", "name": "srv"}]


@respx.mock
@pytest.mark.asyncio
async def test_mcp_tool_config_appended_to_tools_array(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        mcp_servers=[McpServerConfig(url="https://mcp.x.com", name="srv")],
        mcp_tool_configs=[McpToolConfigAllowed(mcp_server_name="srv", allowed_tools=["read"])],
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["tools"] == [
        {"type": "mcp_toolset", "mcp_server_name": "srv", "allowed_tools": ["read"]}
    ]


@respx.mock
@pytest.mark.asyncio
async def test_mcp_beta_header_attached(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        mcp_servers=[McpServerConfig(url="https://mcp.x.com", name="srv")],
    )
    await provider.chat(req)
    assert "mcp-client-2025-11-20" in route.calls[0].request.headers["anthropic-beta"]


@respx.mock
@pytest.mark.asyncio
async def test_mcp_with_auth_token(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        mcp_servers=[
            McpServerConfig(
                url="https://mcp.x.com",
                name="srv",
                authorization_token="Bearer x",
            )
        ],
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["mcp_servers"][0]["authorization_token"] == "Bearer x"


@respx.mock
@pytest.mark.asyncio
async def test_no_mcp_no_beta_header_added(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    assert "mcp-client-2025-11-20" not in route.calls[0].request.headers.get("anthropic-beta", "")


@respx.mock
@pytest.mark.asyncio
async def test_oauth_and_mcp_beta_headers_are_combined():
    provider = AnthropicProvider("sk-ant-oat01-test", base_url="https://mock.anthropic.com")
    sse = 'data: {"type":"message_stop"}\n'
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    req = ChatRequest(
        messages=[Message.user("hi")],
        mcp_servers=[McpServerConfig(url="https://mcp.x.com", name="srv")],
    )
    await provider.chat(req)
    beta = route.calls[0].request.headers["anthropic-beta"]
    assert "oauth-2025-04-20" in beta
    assert "mcp-client-2025-11-20" in beta
