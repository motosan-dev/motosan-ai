from __future__ import annotations

import json

import pytest

from motosan_ai.error import ConfigError
from motosan_ai.providers.chatgpt_codex import ChatGptCodexProvider
from motosan_ai.types import (
    ChatRequest,
    Message,
    SystemBlock,
    Tool,
    ToolCall,
)


def _provider() -> ChatGptCodexProvider:
    return ChatGptCodexProvider("test-token", "acct-123", "gpt-5.5", None)


def test_default_model_and_base_url():
    p = ChatGptCodexProvider("tok", "acct-123")
    assert p.model == "gpt-5.5"
    assert p.base_url == "https://chatgpt.com/backend-api/codex/responses"
    assert p.access_token == "tok"
    assert p.account_id == "acct-123"


def test_explicit_model_and_base_url():
    p = ChatGptCodexProvider("tok", "acct-123", model="gpt-x", base_url="https://mock.test/r")
    assert p.model == "gpt-x"
    assert p.base_url == "https://mock.test/r"


def test_stream_url_returns_base_url_verbatim():
    p = ChatGptCodexProvider("tok", "acct-123", base_url="https://mock.test/r")
    assert p._stream_url() == "https://mock.test/r"


def test_auth_headers_present_and_lowercase():
    h = ChatGptCodexProvider("tok", "acct-123")._headers("tok")
    assert h["authorization"] == "Bearer tok"
    assert h["chatgpt-account-id"] == "acct-123"
    assert h["originator"] == "codex_cli_rs"
    assert h["openai-beta"] == "responses=experimental"
    assert h["accept"] == "text/event-stream"
    assert h["content-type"] == "application/json"


def test_body_has_required_codex_fields():
    p = _provider()
    body = p._build_responses_body(ChatRequest(messages=[Message.user("hi")]))

    assert body["store"] is False
    assert body["stream"] is True
    assert body["model"] == "gpt-5.5"
    assert isinstance(body["instructions"], str)
    assert isinstance(body["input"], list)
    assert body["include"] == ["reasoning.encrypted_content"]
    assert body["tool_choice"] == "auto"
    assert body["parallel_tool_calls"] is True

    assert len(body["input"]) == 1
    assert body["input"][0]["type"] == "message"
    assert body["input"][0]["role"] == "user"
    assert body["input"][0]["content"][0]["type"] == "input_text"
    assert body["input"][0]["content"][0]["text"] == "hi"

    assert "tools" not in body
    assert "reasoning" not in body
    assert "temperature" not in body


def test_empty_request_uses_default_instructions():
    p = _provider()
    body = p._build_responses_body(ChatRequest(messages=[Message.user("hi")]))
    assert body["instructions"] == "You are a helpful assistant."


def test_system_message_goes_to_instructions_not_input():
    p = _provider()
    req = ChatRequest(messages=[Message.system("You are a pirate."), Message.user("hi")])
    body = p._build_responses_body(req)

    assert body["instructions"] == "You are a pirate."
    assert len(body["input"]) == 1
    assert body["input"][0]["role"] == "user"


def test_system_field_used_for_instructions():
    p = _provider()
    req = ChatRequest(messages=[Message.user("hi")], system="be terse")
    body = p._build_responses_body(req)
    assert body["instructions"] == "be terse"


def test_system_blocks_take_priority_over_system_field():
    p = _provider()
    req = ChatRequest(
        messages=[Message.user("hi")],
        system="ignored",
        system_blocks=[SystemBlock.new("block one"), SystemBlock.new("block two")],
    )
    body = p._build_responses_body(req)
    assert body["instructions"] == "block one\n\nblock two"


def test_assistant_text_becomes_output_text_item():
    p = _provider()
    req = ChatRequest(messages=[Message.user("hi"), Message.assistant("hello there")])
    body = p._build_responses_body(req)

    assert len(body["input"]) == 2
    assert body["input"][1]["type"] == "message"
    assert body["input"][1]["role"] == "assistant"
    assert body["input"][1]["content"][0]["type"] == "output_text"
    assert body["input"][1]["content"][0]["text"] == "hello there"


def test_tool_call_and_result_serialize_as_function_items():
    p = _provider()
    tool = Tool(
        name="get_weather",
        description="Fetch the weather",
        input_schema={"type": "object", "properties": {"city": {"type": "string"}}},
    )
    req = ChatRequest(
        messages=[
            Message.user("weather in Paris?"),
            Message.assistant_with_tool_calls(
                "",
                [ToolCall(id="call_1", name="get_weather", input={"city": "Paris"})],
            ),
            Message.tool_result("call_1", "sunny, 21C"),
        ],
        tools=[tool],
    )
    body = p._build_responses_body(req)

    tools = body["tools"]
    assert len(tools) == 1
    assert tools[0]["type"] == "function"
    assert tools[0]["name"] == "get_weather"
    assert tools[0]["description"] == "Fetch the weather"
    assert isinstance(tools[0]["parameters"], dict)
    assert tools[0]["strict"] is None

    input_items = body["input"]
    assert len(input_items) == 3

    fc = input_items[1]
    assert fc["type"] == "function_call"
    assert fc["call_id"] == "call_1"
    assert fc["name"] == "get_weather"
    assert json.loads(fc["arguments"]) == {"city": "Paris"}

    out = input_items[2]
    assert out["type"] == "function_call_output"
    assert out["call_id"] == "call_1"
    assert out["output"] == "sunny, 21C"


def test_empty_tools_list_omits_tools_key():
    p = _provider()
    body = p._build_responses_body(ChatRequest(messages=[Message.user("hi")], tools=[]))
    assert "tools" not in body


def test_reasoning_and_temperature_are_conditional():
    p = _provider()
    req = ChatRequest(
        messages=[Message.user("hi")],
        temperature=0.3,
        provider_options={"reasoning_effort": "high"},
    )
    body = p._build_responses_body(req)

    assert body["temperature"] == 0.3
    assert body["reasoning"]["effort"] == "high"
    assert body["reasoning"]["summary"] == "auto"


def test_reasoning_absent_when_effort_not_a_string():
    p = _provider()
    req = ChatRequest(messages=[Message.user("hi")], provider_options={"reasoning_effort": 5})
    body = p._build_responses_body(req)
    assert "reasoning" not in body


def test_per_request_model_overrides_default():
    p = _provider()
    body = p._build_responses_body(ChatRequest(messages=[Message.user("hi")], model="gpt-override"))
    assert body["model"] == "gpt-override"


# --- C1: provider-level default reasoning effort (mirrors Rust with_reasoning_effort) ---


def test_provider_default_reasoning_effort_is_emitted():
    p = _provider().reasoning_effort("high")
    body = p._build_responses_body(ChatRequest(messages=[Message.user("hi")]))
    assert body["reasoning"]["effort"] == "high"
    assert body["reasoning"]["summary"] == "auto"


def test_per_request_reasoning_effort_wins_over_provider_default():
    p = _provider().reasoning_effort("low")
    req = ChatRequest(
        messages=[Message.user("hi")],
        provider_options={"reasoning_effort": "high"},
    )
    body = p._build_responses_body(req)
    assert body["reasoning"]["effort"] == "high"


def test_no_reasoning_emitted_when_neither_set():
    p = _provider()
    body = p._build_responses_body(ChatRequest(messages=[Message.user("hi")]))
    assert "reasoning" not in body


def test_reasoning_effort_setter_returns_self():
    p = _provider()
    assert p.reasoning_effort("medium") is p
    assert p._reasoning_effort == "medium"


def test_reasoning_effort_none_clears_default():
    p = _provider().reasoning_effort("high").reasoning_effort(None)
    body = p._build_responses_body(ChatRequest(messages=[Message.user("hi")]))
    assert "reasoning" not in body


def test_constructor_requires_access_token_or_token_source():
    with pytest.raises(ConfigError, match="access_token or token_source"):
        ChatGptCodexProvider(account_id="acct-123")


def test_constructor_requires_account_id():
    with pytest.raises(ConfigError, match="account_id"):
        ChatGptCodexProvider(access_token="tok")


def test_constructor_accepts_token_source_without_access_token():
    async def source() -> str:
        return "tok"

    p = ChatGptCodexProvider(account_id="acct-123", token_source=source)
    assert p.access_token is None
    assert p.token_source is source


def test_static_access_token_leaves_token_source_none():
    p = ChatGptCodexProvider("tok", "acct-123")
    assert p.access_token == "tok"
    assert p.token_source is None
