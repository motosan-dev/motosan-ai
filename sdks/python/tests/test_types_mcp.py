from motosan_ai.types import (
    McpServerConfig,
    McpToolConfigAll,
    McpToolConfigAllowed,
    McpToolConfigDenied,
    mcp_server_config_to_dict,
    mcp_tool_config_to_dict,
)


def test_mcp_server_config_minimal():
    cfg = McpServerConfig(url="https://mcp.example.com", name="example")
    assert mcp_server_config_to_dict(cfg) == {
        "type": "url",
        "url": "https://mcp.example.com",
        "name": "example",
    }


def test_mcp_server_config_with_auth():
    cfg = McpServerConfig(
        url="https://mcp.example.com",
        name="example",
        authorization_token="secret",
    )
    assert mcp_server_config_to_dict(cfg) == {
        "type": "url",
        "url": "https://mcp.example.com",
        "name": "example",
        "authorization_token": "secret",
    }


def test_mcp_tool_config_all():
    cfg = McpToolConfigAll(mcp_server_name="example")
    assert mcp_tool_config_to_dict(cfg) == {
        "type": "mcp_toolset",
        "mcp_server_name": "example",
    }


def test_mcp_tool_config_allowed():
    cfg = McpToolConfigAllowed(mcp_server_name="example", allowed_tools=["read", "write"])
    assert mcp_tool_config_to_dict(cfg) == {
        "type": "mcp_toolset",
        "mcp_server_name": "example",
        "allowed_tools": ["read", "write"],
    }


def test_mcp_tool_config_denied():
    cfg = McpToolConfigDenied(mcp_server_name="example", denied_tools=["delete"])
    assert mcp_tool_config_to_dict(cfg) == {
        "type": "mcp_toolset",
        "mcp_server_name": "example",
        "denied_tools": ["delete"],
    }
