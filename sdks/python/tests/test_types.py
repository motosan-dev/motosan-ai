from motosan_ai import Message, Role, StopReason, ToolCall


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
