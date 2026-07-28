from __future__ import annotations

from motosan_ai.providers.responses import (
    encode_function_call_output_payload,
    encode_input,
    encode_message,
    encode_tool_call,
    encode_tool_choice,
    encode_tool_output,
    encode_tools,
    encode_user_content,
    tool_output_to_dict,
)
from motosan_ai.types import (
    FreeformTool,
    FreeformToolFormat,
    FunctionCallOutputContent,
    FunctionCallOutputEncryptedContent,
    FunctionCallOutputInputImage,
    FunctionCallOutputInputText,
    FunctionCallOutputText,
    ImageDetail,
    Message,
    ModelContextMessage,
    ModelContextToolCall,
    ModelContextToolOutput,
    ModelToolCallFreeform,
    ModelToolCallFunction,
    ModelToolOutputCustom,
    ModelToolOutputFunction,
    ModelToolSpecFreeform,
    ModelToolSpecFunction,
    Tool,
    ToolCall,
    ToolChoice,
)


def grammar_fixture() -> FreeformTool:
    return FreeformTool(
        name="exec",
        description="Run JavaScript",
        format=FreeformToolFormat(type="grammar", syntax="lark", definition="start: source"),
    )


def test_encodes_function_and_custom_tools():
    tools = encode_tools(
        [
            ModelToolSpecFunction(
                tool=Tool(
                    name="sum",
                    description="Add numbers",
                    input_schema={"type": "object", "properties": {"a": {"type": "number"}}},
                )
            ),
            ModelToolSpecFreeform(tool=grammar_fixture()),
        ]
    )

    assert tools[0]["type"] == "function"
    assert tools[0]["name"] == "sum"
    assert tools[0]["description"] == "Add numbers"
    # `input_schema` is spelled `parameters` on the wire.
    assert tools[0]["parameters"]["type"] == "object"
    assert "input_schema" not in tools[0]
    assert tools[1] == {
        "type": "custom",
        "name": "exec",
        "description": "Run JavaScript",
        "format": {"type": "grammar", "syntax": "lark", "definition": "start: source"},
    }


def test_encodes_tool_calls_with_call_id_key():
    fn = encode_tool_call(ModelToolCallFunction(id="call_fn", name="sum", arguments='{"a":1}'))
    assert fn == {
        "type": "function_call",
        "call_id": "call_fn",
        "name": "sum",
        "arguments": '{"a":1}',
    }
    assert "id" not in fn

    raw = '{"this":"looks like json"}\nconsole.log(\'but is JS\');'
    ff = encode_tool_call(ModelToolCallFreeform(id="call_js", name="exec", input=raw))
    assert ff == {
        "type": "custom_tool_call",
        "call_id": "call_js",
        "name": "exec",
        "input": raw,
    }
    assert ff["input"].encode() == raw.encode()
    assert "arguments" not in ff


def test_encodes_function_and_custom_outputs_and_drops_custom_name():
    encoded = encode_input(
        [
            ModelContextToolOutput(
                output=ModelToolOutputFunction(
                    call_id="call_fn", output=FunctionCallOutputText(text='{"ok":true}')
                )
            ),
            ModelContextToolOutput(
                output=ModelToolOutputCustom(
                    call_id="call_js",
                    output=FunctionCallOutputText(text="stdout"),
                    name="exec",
                )
            ),
        ]
    )

    assert encoded[0]["type"] == "function_call_output"
    assert encoded[0]["call_id"] == "call_fn"
    assert encoded[0]["output"] == '{"ok":true}'
    assert encoded[1]["type"] == "custom_tool_call_output"
    assert encoded[1]["call_id"] == "call_js"
    # The wire encoder deliberately drops `name` (Rust codec parity).
    assert "name" not in encoded[1]
    assert encoded[1]["output"] == "stdout"


def test_tool_output_to_dict_keeps_call_identity():
    output = ModelToolOutputCustom(
        call_id="call_js", output=FunctionCallOutputText(text="stdout: 42"), name="exec"
    )
    encoded = tool_output_to_dict(output)
    assert encoded["type"] == "custom_tool_call_output"
    assert encoded["call_id"] == "call_js"
    assert encoded["name"] == "exec"
    assert encoded["output"] == "stdout: 42"
    assert "name" not in tool_output_to_dict(
        ModelToolOutputCustom(call_id="c", output=FunctionCallOutputText(text=""))
    )
    assert "name" not in tool_output_to_dict(
        ModelToolOutputFunction(call_id="call_fn", output=FunctionCallOutputText(text="1"))
    )


def test_encodes_function_call_output_payload_shapes():
    assert encode_function_call_output_payload(FunctionCallOutputText(text="plain")) == "plain"
    assert encode_function_call_output_payload(
        FunctionCallOutputContent(
            items=[
                FunctionCallOutputInputText(text="hi"),
                FunctionCallOutputInputImage(image_url="https://x.test/i.png"),
                FunctionCallOutputInputImage(
                    image_url="https://x.test/j.png", detail=ImageDetail.low
                ),
                FunctionCallOutputEncryptedContent(encrypted_content="enc"),
            ]
        )
    ) == [
        {"type": "input_text", "text": "hi"},
        {"type": "input_image", "image_url": "https://x.test/i.png"},
        {"type": "input_image", "image_url": "https://x.test/j.png", "detail": "low"},
        {"type": "encrypted_content", "encrypted_content": "enc"},
    ]


def test_encodes_user_content_blocks_as_input_image():
    content = encode_user_content(Message.user_with_image("inspect", "abc123", "image/png"))
    assert content[0] == {"type": "input_text", "text": "inspect"}
    assert content[1] == {
        "type": "input_image",
        "image_url": "data:image/png;base64,abc123",
    }


def test_encodes_plain_user_message_and_document_only_message():
    assert encode_user_content(Message.user("hello")) == [{"type": "input_text", "text": "hello"}]
    # Document blocks are not representable on the Responses wire; the encoder
    # falls back to the message's flat text rather than emitting nothing.
    pdf = Message.user_with_pdf_base64("read this", "abc")
    content = encode_user_content(pdf)
    assert content == [{"type": "input_text", "text": "read this"}]


def test_encode_message_expands_assistant_text_and_tool_calls():
    message = Message.assistant_with_tool_calls(
        "on it", [ToolCall(id="call_fn", name="sum", input={"a": 1})]
    )
    items = encode_message(message)
    assert items[0] == {
        "type": "message",
        "role": "assistant",
        "content": [{"type": "output_text", "text": "on it"}],
    }
    assert items[1]["type"] == "function_call"
    assert items[1]["call_id"] == "call_fn"
    assert items[1]["name"] == "sum"
    assert '"a"' in items[1]["arguments"]

    # No text -> no message item, only the call.
    only_call = encode_message(
        Message.assistant_with_tool_calls("", [ToolCall(id="c", name="n", input={})])
    )
    assert len(only_call) == 1
    assert only_call[0]["type"] == "function_call"


def test_encode_message_maps_tool_result_and_drops_system():
    assert encode_message(Message.system("be terse")) == []
    assert encode_message(Message.tool_result("call_fn", "42")) == [
        {"type": "function_call_output", "call_id": "call_fn", "output": "42"}
    ]
    # A tool message without a call id has nothing to attach to.
    assert encode_message(Message(role=Message.tool_result("x", "y").role, content="z")) == []


def test_encode_input_preserves_mixed_ordered_history_byte_exact():
    raw = '{"not":"function args"}\nvalue.not;\n'
    encoded = encode_input(
        [
            ModelContextMessage(message=Message.user("run js")),
            ModelContextToolCall(
                call=ModelToolCallFunction(id="call_fn", name="sum", arguments='{"a":1}')
            ),
            ModelContextToolOutput(
                output=ModelToolOutputFunction(
                    call_id="call_fn", output=FunctionCallOutputText(text="1")
                )
            ),
            ModelContextToolCall(call=ModelToolCallFreeform(id="call_js", name="exec", input=raw)),
            ModelContextToolOutput(
                output=ModelToolOutputCustom(
                    call_id="call_js",
                    output=FunctionCallOutputText(text="function args"),
                    name="exec",
                )
            ),
        ]
    )

    assert [item["type"] for item in encoded] == [
        "message",
        "function_call",
        "function_call_output",
        "custom_tool_call",
        "custom_tool_call_output",
    ]
    assert encoded[3]["input"].encode() == raw.encode()
    assert "arguments" not in encoded[3]


def test_encode_tool_choice():
    assert encode_tool_choice(ToolChoice.auto()) == "auto"
    assert encode_tool_choice(ToolChoice.required()) == "required"
    assert encode_tool_choice(ToolChoice.none()) == "none"
    assert encode_tool_choice(ToolChoice.tool("run_js")) == {
        "type": "function",
        "name": "run_js",
    }
