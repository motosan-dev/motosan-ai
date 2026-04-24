import pytest

from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.types import (
    ChatRequest,
    ImageBlock,
    ImageSourceBase64,
    ImageSourceUrl,
    Message,
    Role,
    SystemBlock,
    TextBlock,
    Tool,
    ToolCall,
    ToolChoice,
)


@pytest.fixture
def provider():
    return GeminiProvider(api_key="test-key")


def test_simple_user_message(provider):
    body = provider._build_body(ChatRequest(messages=[Message.user("Hello")]))
    assert body["contents"] == [{"role": "user", "parts": [{"text": "Hello"}]}]


def test_assistant_becomes_model_role(provider):
    body = provider._build_body(
        ChatRequest(messages=[Message.user("hi"), Message.assistant("hello back")])
    )
    assert body["contents"][0]["role"] == "user"
    assert body["contents"][1] == {"role": "model", "parts": [{"text": "hello back"}]}


def test_multi_turn_conversation(provider):
    body = provider._build_body(
        ChatRequest(messages=[Message.user("q1"), Message.assistant("a1"), Message.user("q2")])
    )
    assert [c["role"] for c in body["contents"]] == ["user", "model", "user"]


def test_empty_content_still_produces_part(provider):
    body = provider._build_body(ChatRequest(messages=[Message.user("")]))
    assert body["contents"][0]["parts"] == [{"text": ""}]


def test_system_string_becomes_system_instruction(provider):
    body = provider._build_body(ChatRequest(messages=[Message.user("hi")], system="Be concise."))
    assert body["systemInstruction"] == {"parts": [{"text": "Be concise."}]}
    assert len(body["contents"]) == 1


def test_extracted_system_role_becomes_system_instruction(provider):
    body = provider._build_body(
        ChatRequest(messages=[Message.system("Be concise."), Message.user("hi")])
    )
    assert body["systemInstruction"] == {"parts": [{"text": "Be concise."}]}


def test_system_blocks_joined_with_newlines(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user("hi")],
            system_blocks=[SystemBlock.new("Block A"), SystemBlock.new("Block B")],
        )
    )
    assert body["systemInstruction"] == {"parts": [{"text": "Block A\nBlock B"}]}


def test_system_blocks_take_priority_over_system_string(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user("hi")], system="IGNORED", system_blocks=[SystemBlock.new("WINS")]
        )
    )
    assert body["systemInstruction"] == {"parts": [{"text": "WINS"}]}


def test_no_system_omits_instruction(provider):
    body = provider._build_body(ChatRequest(messages=[Message.user("hi")]))
    assert "systemInstruction" not in body


def test_generation_config_default_max_tokens(provider):
    body = provider._build_body(ChatRequest(messages=[Message.user("hi")]))
    assert body["generationConfig"]["maxOutputTokens"] == 8192


def test_generation_config_custom_values(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user("hi")], temperature=0.3, max_tokens=512, stop_sequences=["END"]
        )
    )
    cfg = body["generationConfig"]
    assert cfg["temperature"] == 0.3
    assert cfg["maxOutputTokens"] == 512
    assert cfg["stopSequences"] == ["END"]


def test_empty_stop_sequences_omitted(provider):
    body = provider._build_body(ChatRequest(messages=[Message.user("hi")], stop_sequences=[]))
    assert "stopSequences" not in body["generationConfig"]


def test_user_with_image_base64_becomes_inline_data(provider):
    body = provider._build_body(
        ChatRequest(messages=[Message.user_with_image("describe", "JVBER", "image/png")])
    )
    assert body["contents"][0]["parts"] == [
        {"text": "describe"},
        {"text": "describe"},
        {"inlineData": {"mimeType": "image/png", "data": "JVBER"}},
    ]


def test_image_url_becomes_file_data(provider):
    msg = Message.user_with_blocks(
        [TextBlock(text="see"), ImageBlock(source=ImageSourceUrl(url="https://x.com/i.png"))]
    )
    body = provider._build_body(ChatRequest(messages=[msg]))
    assert body["contents"][0]["parts"] == [
        {"text": "see"},
        {"text": "see"},
        {"fileData": {"fileUri": "https://x.com/i.png"}},
    ]


def test_user_message_without_blocks_only_emits_content_part(provider):
    body = provider._build_body(ChatRequest(messages=[Message.user("plain")]))
    assert body["contents"][0]["parts"] == [{"text": "plain"}]


def test_manual_user_content_plus_image_blocks_preserves_content(provider):
    msg = Message(
        role=Role.user,
        content="describe this",
        content_blocks=[ImageBlock(source=ImageSourceBase64(media_type="image/png", data="abc"))],
    )
    body = provider._build_body(ChatRequest(messages=[msg]))
    assert body["contents"][0]["parts"] == [
        {"text": "describe this"},
        {"inlineData": {"mimeType": "image/png", "data": "abc"}},
    ]


def test_user_with_image_factory_matches_rust_content_plus_text_block(provider):
    body = provider._build_body(
        ChatRequest(messages=[Message.user_with_image("describe", "JVBER", "image/png")])
    )
    assert body["contents"][0]["parts"] == [
        {"text": "describe"},
        {"text": "describe"},
        {"inlineData": {"mimeType": "image/png", "data": "JVBER"}},
    ]


def test_tools_wrap_in_function_declarations_array(provider):
    tools = [
        Tool(
            name="get_weather",
            description="Weather for a city",
            input_schema={"type": "object", "properties": {"city": {"type": "string"}}},
        )
    ]
    body = provider._build_body(ChatRequest(messages=[Message.user("?")], tools=tools))
    assert body["tools"] == [
        {
            "functionDeclarations": [
                {
                    "name": "get_weather",
                    "description": "Weather for a city",
                    "parameters": {"type": "object", "properties": {"city": {"type": "string"}}},
                }
            ]
        }
    ]


def test_tool_without_schema_omits_parameters(provider):
    body = provider._build_body(
        ChatRequest(messages=[Message.user("?")], tools=[Tool(name="x", description="X")])
    )
    decl = body["tools"][0]["functionDeclarations"][0]
    assert "parameters" not in decl


def test_tool_choice_auto_is_default_mode(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user("?")], tools=[Tool(name="x")], tool_choice=ToolChoice.auto()
        )
    )
    assert body["toolConfig"]["functionCallingConfig"]["mode"] == "AUTO"


def test_tool_choice_required_is_any(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user("?")], tools=[Tool(name="x")], tool_choice=ToolChoice.required()
        )
    )
    assert body["toolConfig"]["functionCallingConfig"]["mode"] == "ANY"


def test_tool_choice_none_removes_tools_and_toolconfig(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user("?")], tools=[Tool(name="x")], tool_choice=ToolChoice.none()
        )
    )
    assert "tools" not in body
    assert "toolConfig" not in body


def test_tool_choice_specific_tool_restricts_allowed_names(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user("?")],
            tools=[Tool(name="get_weather"), Tool(name="search")],
            tool_choice=ToolChoice.tool("get_weather"),
        )
    )
    cfg = body["toolConfig"]["functionCallingConfig"]
    assert cfg["mode"] == "ANY"
    assert cfg["allowedFunctionNames"] == ["get_weather"]


def test_assistant_tool_call_becomes_function_call_part(provider):
    tc = ToolCall(id="ignored", name="get_weather", input={"city": "Taipei"})
    msg = Message.assistant_with_tool_calls("checking...", [tc])
    body = provider._build_body(ChatRequest(messages=[Message.user("weather?"), msg]))
    assert body["contents"][1]["parts"] == [
        {"text": "checking..."},
        {"functionCall": {"name": "get_weather", "args": {"city": "Taipei"}}},
    ]


def test_assistant_tool_call_without_text_still_valid(provider):
    tc = ToolCall(id="x", name="get_weather", input={})
    msg = Message.assistant_with_tool_calls("", [tc])
    body = provider._build_body(ChatRequest(messages=[Message.user("?"), msg]))
    assert body["contents"][1]["parts"] == [{"functionCall": {"name": "get_weather", "args": {}}}]


def test_tool_result_becomes_user_role_with_function_response(provider):
    tool_msg = Message.tool_result("get_weather", '{"result": "sunny"}')
    body = provider._build_body(ChatRequest(messages=[Message.user("?"), tool_msg]))
    assert body["contents"][1] == {
        "role": "user",
        "parts": [{"functionResponse": {"name": "get_weather", "response": {"result": "sunny"}}}],
    }


def test_tool_result_with_non_json_content_wraps_in_result_field(provider):
    tool_msg = Message.tool_result("x", "just a plain string")
    body = provider._build_body(ChatRequest(messages=[Message.user("?"), tool_msg]))
    part = body["contents"][1]["parts"][0]
    assert part["functionResponse"]["response"] == {"result": "just a plain string"}
