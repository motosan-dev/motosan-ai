import pytest

from motosan_ai.types import ToolChoice, tool_choice_to_dict


def test_tool_choice_auto():
    tc = ToolChoice.auto()
    assert tc.type == "auto"
    assert tc.name is None
    assert tool_choice_to_dict(tc) == {"type": "auto"}


def test_tool_choice_required():
    tc = ToolChoice.required()
    assert tc.type == "required"
    assert tool_choice_to_dict(tc) == {"type": "required"}


def test_tool_choice_none():
    tc = ToolChoice.none()
    assert tc.type == "none"
    assert tool_choice_to_dict(tc) == {"type": "none"}


def test_tool_choice_tool():
    tc = ToolChoice.tool("get_weather")
    assert tc.type == "tool"
    assert tc.name == "get_weather"
    assert tool_choice_to_dict(tc) == {"type": "tool", "name": "get_weather"}


def test_tool_choice_tool_requires_name():
    with pytest.raises(ValueError, match="tool name required"):
        ToolChoice(type="tool", name=None)
