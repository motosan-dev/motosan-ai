from motosan_ai.types import (
    ChatRequest,
    McpServerConfig,
    McpToolConfigAll,
    McpToolConfigAllowed,
    Message,
    SystemBlock,
    Tool,
    ToolChoice,
)


def test_builder_minimal():
    req = ChatRequest.builder().message(Message.user("hi")).build()
    assert len(req.messages) == 1
    assert req.messages[0].content == "hi"


def test_builder_system_cached():
    req = (
        ChatRequest.builder().message(Message.user("hi")).system_cached("You are a helper.").build()
    )
    assert req.system == "You are a helper."
    assert req.system_cache is True


def test_builder_system_block_appends():
    req = (
        ChatRequest.builder()
        .system_block(SystemBlock.cached("A"))
        .system_block(SystemBlock.new("B"))
        .message(Message.user("hi"))
        .build()
    )
    assert len(req.system_blocks) == 2
    assert req.system_blocks[0].cache_control is True
    assert req.system_blocks[1].cache_control is False


def test_builder_tools_cached_marks_last():
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .tools_cached([Tool(name="a"), Tool(name="b")])
        .build()
    )
    assert req.tools[0].cache is False
    assert req.tools[1].cache is True


def test_builder_tool_choice():
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .tool_choice(ToolChoice.tool("get_weather"))
        .build()
    )
    assert req.tool_choice.type == "tool"
    assert req.tool_choice.name == "get_weather"


def test_builder_mcp_server_auto_adds_all_config():
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .mcp_server(McpServerConfig(url="https://x", name="srv"))
        .build()
    )
    assert len(req.mcp_servers) == 1
    assert len(req.mcp_tool_configs) == 1
    cfg = req.mcp_tool_configs[0]
    assert isinstance(cfg, McpToolConfigAll)
    assert cfg.mcp_server_name == "srv"


def test_builder_mcp_tool_config_replaces_same_server():
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .mcp_server(McpServerConfig(url="https://x", name="srv"))
        .mcp_tool_config(McpToolConfigAllowed(mcp_server_name="srv", allowed_tools=["r"]))
        .build()
    )
    assert len(req.mcp_tool_configs) == 1
    cfg = req.mcp_tool_configs[0]
    assert isinstance(cfg, McpToolConfigAllowed)
    assert cfg.allowed_tools == ["r"]


def test_builder_mcp_server_preserves_existing_config_for_same_server():
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .mcp_tool_config(McpToolConfigAllowed(mcp_server_name="srv", allowed_tools=["r"]))
        .mcp_server(McpServerConfig(url="https://x", name="srv"))
        .build()
    )
    assert len(req.mcp_tool_configs) == 1
    cfg = req.mcp_tool_configs[0]
    assert isinstance(cfg, McpToolConfigAllowed)
    assert cfg.allowed_tools == ["r"]


def test_builder_thinking_and_stop():
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .thinking(2048)
        .stop("END")
        .stop("STOP")
        .build()
    )
    assert req.thinking.budget_tokens == 2048
    assert req.stop_sequences == ["END", "STOP"]
