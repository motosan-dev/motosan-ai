from motosan_ai import Message, Role, StopReason, ToolCall
from motosan_ai.types import (
    ChatRequest,
    ChatResponse,
    McpServerConfig,
    McpToolConfigAll,
    StreamEvent,
    StreamEventType,
    SystemBlock,
    ThinkingConfig,
    Tool,
    ToolChoice,
    Usage,
)


def test_message_helpers():
    user = Message.user("hi")
    assistant = Message.assistant("hello")
    system = Message.system("rules")
    tool = Message.tool_result("call_1", "25C")
    assistant_tc = Message.assistant_with_tool_calls(
        "checking",
        [ToolCall(id="call_1", name="get_weather", input={"city": "Taipei"})],
    )

    assert user.role == Role.user
    assert assistant.role == Role.assistant
    assert system.role == Role.system
    assert tool.role == Role.tool
    assert tool.tool_call_id == "call_1"
    assert assistant_tc.tool_calls[0].name == "get_weather"


def test_stop_reason_values():
    assert StopReason.end_turn.value == "end_turn"
    assert StopReason.tool_use.value == "tool_use"


def test_tool_cache_defaults_false():
    t = Tool(name="x")
    assert t.cache is False


def test_tool_cache_explicit_true():
    t = Tool(name="x", cache=True)
    assert t.cache is True


def test_thinking_config_budget():
    cfg = ThinkingConfig(budget_tokens=4096)
    assert cfg.budget_tokens == 4096


def test_usage_cache_fields_default_none():
    u = Usage(input_tokens=10, output_tokens=5)
    assert u.cache_creation_input_tokens is None
    assert u.cache_read_input_tokens is None


def test_usage_cache_fields_explicit():
    u = Usage(
        input_tokens=10,
        output_tokens=5,
        cache_creation_input_tokens=100,
        cache_read_input_tokens=50,
    )
    assert u.cache_creation_input_tokens == 100
    assert u.cache_read_input_tokens == 50


def test_stop_reason_stop_sequence_exists():
    assert StopReason.stop_sequence == "stop_sequence"


def test_stream_event_type_values():
    assert StreamEventType.text == "text"
    assert StreamEventType.tool_call_start == "tool_call_start"
    assert StreamEventType.tool_call_args == "tool_call_args"
    assert StreamEventType.tool_call_end == "tool_call_end"
    assert StreamEventType.usage == "usage"


def test_stream_event_type_thinking_members():
    assert StreamEventType.thinking_delta == "thinking_delta"
    assert StreamEventType.thinking_done == "thinking_done"
    # Full M4/F2 vocabulary. Note: NO "done" member — done is a bool field
    # on StreamEvent, never an event_type.
    assert {m.value for m in StreamEventType} == {
        "text",
        "tool_call_start",
        "tool_call_args",
        "tool_call_end",
        "usage",
        "thinking_delta",
        "thinking_done",
    }


def test_stream_event_new_optional_fields_default_none():
    ev = StreamEvent(content="hi", done=False)
    assert ev.stop_reason is None
    assert ev.usage is None


def test_chat_response_thinking_defaults_none():
    r = ChatResponse(content="hi")
    assert r.thinking is None


def test_chat_response_thinking_explicit():
    r = ChatResponse(content="hi", thinking="reasoning trace")
    assert r.thinking == "reasoning trace"


def test_chat_request_new_fields_default_none():
    req = ChatRequest(messages=[Message.user("hi")])
    assert req.system_blocks is None
    assert req.system_cache is False
    assert req.tool_choice is None
    assert req.mcp_servers is None
    assert req.mcp_tool_configs is None
    assert req.thinking is None
    assert req.stop_sequences is None


def test_chat_request_all_new_fields_settable():
    req = ChatRequest(
        messages=[Message.user("hi")],
        system_blocks=[SystemBlock.new("a")],
        system_cache=True,
        tool_choice=ToolChoice.required(),
        mcp_servers=[McpServerConfig(url="https://x", name="x")],
        mcp_tool_configs=[McpToolConfigAll(mcp_server_name="x")],
        thinking=ThinkingConfig(budget_tokens=1024),
        stop_sequences=["STOP"],
    )
    assert req.system_cache is True
    assert req.thinking.budget_tokens == 1024
    assert req.stop_sequences == ["STOP"]


def test_public_api_exports_new_types():
    import motosan_ai as m

    assert m.BaseProvider is not None
    assert m.ContentBlock is not None
    assert m.TextBlock is not None
    assert m.ImageBlock is not None
    assert m.DocumentBlock is not None
    assert m.ImageSourceBase64 is not None
    assert m.ImageSourceUrl is not None
    assert m.DocumentSourceBase64 is not None
    assert m.DocumentSourceUrl is not None
    assert m.SystemBlock is not None
    assert m.ToolChoice is not None
    assert m.ThinkingConfig is not None
    assert m.McpServerConfig is not None
    assert m.McpToolConfigAll is not None
    assert m.McpToolConfigAllowed is not None
    assert m.McpToolConfigDenied is not None
    assert m.ProviderCapabilities is not None
    assert m.StreamEventType is not None


def test_stream_event_session_id_defaults_none():
    assert StreamEvent(content="hi", done=False).session_id is None


def test_stream_event_session_id_settable():
    assert StreamEvent(content="", done=False, session_id="s").session_id == "s"


def test_chat_response_session_id_defaults_none():
    assert ChatResponse(content="hi").session_id is None


def test_chat_response_session_id_settable():
    assert ChatResponse(content="hi", session_id="s").session_id == "s"
