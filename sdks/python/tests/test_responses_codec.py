from __future__ import annotations

import json

from motosan_ai.providers.responses import (
    build_model_request_body,
    decode_function_call_output_payload,
    decode_output_text,
    decode_tool_call,
    decode_tool_output,
    decode_usage,
    encode_function_call_output_payload,
    encode_input,
    encode_message,
    encode_tool_call,
    encode_tool_choice,
    encode_tools,
    encode_user_content,
    model_chat_response_from_output,
    ModelStreamState,
    parse_model_sse_event,
    stop_reason_from_status,
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
    ModelChatRequest,
    ModelContextMessage,
    ModelContextToolCall,
    ModelContextToolOutput,
    ModelStreamDone,
    ModelStreamFreeformInput,
    ModelStreamFunctionArguments,
    ModelStreamText,
    ModelStreamThinkingDelta,
    ModelStreamThinkingDone,
    ModelStreamToolCallDone,
    ModelStreamUsage,
    ModelToolCallFreeform,
    ModelToolCallFunction,
    ModelToolOutputCustom,
    ModelToolOutputFunction,
    ModelToolSpecFreeform,
    ModelToolSpecFunction,
    StopReason,
    SystemBlock,
    Tool,
    ToolCall,
    ToolChoice,
    Usage,
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


def test_decodes_function_and_custom_calls():
    assert decode_tool_call(
        {
            "type": "function_call",
            "call_id": "call_fn",
            "name": "sum",
            "arguments": '{"a":1}',
        }
    ) == ModelToolCallFunction(id="call_fn", name="sum", arguments='{"a":1}')

    assert decode_tool_call(
        {
            "type": "custom_tool_call",
            "call_id": "call_js",
            "name": "exec",
            "input": "const a = {raw: true};\n",
        }
    ) == ModelToolCallFreeform(id="call_js", name="exec", input="const a = {raw: true};\n")


def test_decode_tool_call_accepts_id_when_call_id_is_absent():
    assert decode_tool_call(
        {"type": "function_call", "id": "fc_1", "name": "sum", "arguments": "{}"}
    ) == ModelToolCallFunction(id="fc_1", name="sum", arguments="{}")
    # call_id wins when both are present.
    assert decode_tool_call(
        {
            "type": "custom_tool_call",
            "id": "fc_1",
            "call_id": "call_js",
            "name": "exec",
            "input": "x",
        }
    ) == ModelToolCallFreeform(id="call_js", name="exec", input="x")


def test_decode_tool_call_returns_none_for_non_calls():
    assert decode_tool_call({"type": "message", "role": "assistant"}) is None
    assert decode_tool_call({"type": "reasoning", "summary": []}) is None
    assert decode_tool_call("not a dict") is None
    assert decode_tool_call({"type": "function_call", "name": "sum"}) is None
    assert decode_tool_call({"type": "function_call", "call_id": "c"}) is None


def test_decode_preserves_raw_custom_input_without_json_parsing():
    raw = '{"this":"looks like json"}\nconsole.log(\'but is JavaScript\');'
    decoded = decode_tool_call(
        {"type": "custom_tool_call", "call_id": "call_js", "name": "exec", "input": raw}
    )
    assert isinstance(decoded, ModelToolCallFreeform)
    assert decoded.input == raw
    assert decoded.input.encode() == raw.encode()


def test_tool_output_round_trips_with_call_identity():
    output = ModelToolOutputCustom(
        call_id="call_js", output=FunctionCallOutputText(text="stdout: 42"), name="exec"
    )
    assert decode_tool_output(tool_output_to_dict(output)) == output

    fn_output = ModelToolOutputFunction(
        call_id="call_fn", output=FunctionCallOutputText(text='{"ok":true}')
    )
    assert decode_tool_output(tool_output_to_dict(fn_output)) == fn_output
    assert decode_tool_output({"type": "message"}) is None
    assert decode_tool_output({"type": "function_call_output", "call_id": "c"}) is None


def test_decode_function_call_output_payload_content_items():
    decoded = decode_function_call_output_payload(
        [
            {"type": "input_text", "text": "hi"},
            {"type": "input_image", "image_url": "u", "detail": "high"},
            {"type": "encrypted_content", "encrypted_content": "enc"},
        ]
    )
    assert decoded == FunctionCallOutputContent(
        items=[
            FunctionCallOutputInputText(text="hi"),
            FunctionCallOutputInputImage(image_url="u", detail=ImageDetail.high),
            FunctionCallOutputEncryptedContent(encrypted_content="enc"),
        ]
    )
    assert decode_function_call_output_payload("plain") == FunctionCallOutputText(text="plain")


def test_stop_reason_from_status():
    assert stop_reason_from_status("completed", True) == StopReason.tool_use
    assert stop_reason_from_status("incomplete", True) == StopReason.tool_use
    assert stop_reason_from_status("completed", False) == StopReason.end_turn
    assert stop_reason_from_status(None, False) == StopReason.end_turn
    assert stop_reason_from_status("incomplete", False) == StopReason.max_tokens
    assert stop_reason_from_status("failed", False) == StopReason.other
    assert stop_reason_from_status("weird", False) == StopReason.other


def test_decode_usage_accepts_both_spellings():
    assert decode_usage({"input_tokens": 9, "output_tokens": 7}) == Usage(
        input_tokens=9, output_tokens=7
    )
    assert decode_usage({"prompt_tokens": 4, "completion_tokens": 5}) == Usage(
        input_tokens=4, output_tokens=5
    )
    assert decode_usage(None) == Usage(0, 0)
    assert decode_usage({}) == Usage(0, 0)

    cached = decode_usage(
        {"input_tokens": 9, "output_tokens": 7, "input_tokens_details": {"cached_tokens": 3}}
    )
    assert cached.cache_read_input_tokens == 3
    assert cached.cache_creation_input_tokens is None
    zero = decode_usage(
        {"input_tokens": 9, "output_tokens": 7, "input_tokens_details": {"cached_tokens": 0}}
    )
    assert zero.cache_read_input_tokens is None


def test_decode_output_text():
    assert (
        decode_output_text(
            {
                "type": "message",
                "role": "assistant",
                "content": [
                    {"type": "output_text", "text": "Hi "},
                    {"type": "refusal", "text": "ignored"},
                    {"type": "output_text", "text": "there"},
                ],
            }
        )
        == "Hi there"
    )
    assert decode_output_text({"type": "function_call"}) is None
    assert decode_output_text({"type": "message", "content": []}) is None


def test_model_chat_response_from_output_assembles_calls_thinking_and_usage():
    raw = "const x = {a: 1};\nconsole.log(x.a);\n"
    response = model_chat_response_from_output(
        {
            "model": "gpt-5.5-codex",
            "status": "completed",
            "output": [
                {
                    "type": "reasoning",
                    "summary": [{"text": "thought "}, {"content": "harder"}],
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "ok"}],
                },
                {
                    "type": "custom_tool_call",
                    "call_id": "call_js",
                    "name": "exec",
                    "input": raw,
                },
            ],
            "usage": {"input_tokens": 9, "output_tokens": 7},
        },
        "fallback-model",
    )

    assert response.content == "ok"
    assert response.thinking == "thought harder"
    assert response.tool_calls == [ModelToolCallFreeform(id="call_js", name="exec", input=raw)]
    assert response.model == "gpt-5.5-codex"
    assert response.usage == Usage(input_tokens=9, output_tokens=7)
    assert response.stop_reason == StopReason.tool_use
    assert response.session_id is None


def test_model_chat_response_from_output_defaults_and_output_text_field():
    response = model_chat_response_from_output(
        {"output_text": "flat text", "status": "incomplete"}, "fallback-model"
    )
    assert response.content == "flat text"
    assert response.model == "fallback-model"
    assert response.thinking is None
    assert response.tool_calls == []
    assert response.stop_reason == StopReason.max_tokens
    assert model_chat_response_from_output({}, "fallback-model").stop_reason == StopReason.end_turn


def test_build_body_minimum_shape_and_stream_flag():
    request = ModelChatRequest.builder().message(Message.user("hi")).build()

    body = build_model_request_body(request, "gpt-test", stream=False)
    assert body["model"] == "gpt-test"
    assert body["input"] == [
        {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]}
    ]
    assert "stream" not in body
    assert "tools" not in body
    assert "instructions" not in body

    streamed = build_model_request_body(request, "gpt-test", stream=True)
    assert streamed["stream"] is True


def test_build_body_prefers_request_model_over_default():
    request = ModelChatRequest.builder().model("gpt-5.5-codex").build()
    assert build_model_request_body(request, "gpt-test", stream=False)["model"] == "gpt-5.5-codex"


def test_build_body_hoists_system_messages_out_of_input():
    request = (
        ModelChatRequest.builder()
        .message(Message.system("  first  "))
        .message(Message.user("run it"))
        .message(Message.system("second"))
        .build()
    )
    body = build_model_request_body(request, "gpt-test", stream=False)

    assert body["instructions"] == "first\n\nsecond"
    assert [item["type"] for item in body["input"]] == ["message"]
    assert body["input"][0]["role"] == "user"


def test_build_body_system_blocks_beat_system_string_and_prefix_hoisted():
    from_blocks = (
        ModelChatRequest.builder()
        .system("ignored")
        .system_block(SystemBlock.new("  block a  "))
        .system_block(SystemBlock.new(""))
        .system_block(SystemBlock.new("block b"))
        .message(Message.system("from context"))
        .build()
    )
    body = build_model_request_body(from_blocks, "gpt-test", stream=False)
    assert body["instructions"] == "block a\n\nblock b\n\nfrom context"

    from_string = ModelChatRequest.builder().system("  plain  ").build()
    assert (
        build_model_request_body(from_string, "gpt-test", stream=False)["instructions"] == "plain"
    )


def test_build_body_default_instructions_are_a_fallback_only():
    empty = ModelChatRequest.builder().message(Message.user("hi")).build()
    assert (
        build_model_request_body(
            empty, "gpt-test", stream=True, default_instructions="You are a helpful assistant."
        )["instructions"]
        == "You are a helpful assistant."
    )
    assert "instructions" not in build_model_request_body(empty, "gpt-test", stream=True)

    with_system = ModelChatRequest.builder().system("be terse").build()
    assert (
        build_model_request_body(
            with_system,
            "gpt-test",
            stream=True,
            default_instructions="You are a helpful assistant.",
        )["instructions"]
        == "be terse"
    )


def test_build_body_scalar_fields_use_wire_key_names():
    request = (
        ModelChatRequest.builder()
        .temperature(0.5)
        .max_tokens(256)
        .tool_choice(ToolChoice.required())
        .stop("END")
        .build()
    )
    body = build_model_request_body(request, "gpt-test", stream=False)

    assert body["temperature"] == 0.5
    # max_tokens -> max_output_tokens on the wire.
    assert body["max_output_tokens"] == 256
    assert "max_tokens" not in body
    assert body["tool_choice"] == "required"
    assert body["stop"] == ["END"]

    forced = build_model_request_body(
        ModelChatRequest.builder().tool_choice(ToolChoice.tool("run_js")).build(),
        "gpt-test",
        stream=False,
    )
    assert forced["tool_choice"] == {"type": "function", "name": "run_js"}

    assert "stop" not in build_model_request_body(
        ModelChatRequest.builder().stop_sequences([]).build(), "gpt-test", stream=False
    )


def test_build_body_encodes_tool_specs():
    request = (
        ModelChatRequest.builder()
        .tool_spec(ModelToolSpecFreeform(tool=grammar_fixture()))
        .tool_spec(ModelToolSpecFunction(tool=Tool(name="sum", description="Add", input_schema={})))
        .build()
    )
    body = build_model_request_body(request, "gpt-test", stream=False)
    assert body["tools"][0]["type"] == "custom"
    assert body["tools"][0]["format"]["syntax"] == "lark"
    assert body["tools"][1]["type"] == "function"
    assert body["tools"][1]["name"] == "sum"


def test_build_body_shallow_merges_provider_options_last():
    request = (
        ModelChatRequest.builder()
        .model("gpt-5.5-codex")
        .temperature(0.1)
        .provider_options({"temperature": 0.9, "reasoning_effort": "high", "extra": True})
        .build()
    )
    body = build_model_request_body(request, "gpt-test", stream=False)

    # provider_options wins over everything the encoder produced.
    assert body["temperature"] == 0.9
    assert body["reasoning_effort"] == "high"
    assert body["extra"] is True
    assert body["model"] == "gpt-5.5-codex"


def _frames(state, *payloads):
    out = []
    for payload in payloads:
        out.extend(parse_model_sse_event(json.dumps(payload), state))
    return out


def test_parse_text_and_thinking_frames():
    state = ModelStreamState()
    deltas = _frames(
        state,
        {"type": "response.output_text.delta", "delta": "Hi "},
        {"type": "response.output_text.delta", "delta": ""},
        {"type": "response.output_text.delta", "delta": "there"},
        {"type": "response.reasoning_text.delta", "delta": "think "},
        {"type": "response.reasoning_summary_text.delta", "delta": "hard"},
        {"type": "response.reasoning_text.done", "text": "think hard"},
        {"type": "response.reasoning_summary_text.done", "delta": "fallback key"},
    )
    assert deltas == [
        ModelStreamText(delta="Hi "),
        ModelStreamText(delta="there"),
        ModelStreamThinkingDelta(delta="think "),
        ModelStreamThinkingDelta(delta="hard"),
        ModelStreamThinkingDone(thinking="think hard"),
        ModelStreamThinkingDone(thinking="fallback key"),
    ]
    # An explicitly empty thinking block still produces a delta; the collector
    # is what turns it into "no thinking".
    assert parse_model_sse_event(
        json.dumps({"type": "response.reasoning_text.done", "text": ""}), ModelStreamState()
    ) == [ModelStreamThinkingDone(thinking="")]


def test_parse_freeform_input_deltas_and_authoritative_done():
    state = ModelStreamState()
    deltas = _frames(
        state,
        {
            "type": "response.custom_tool_call_input.delta",
            "call_id": "call_js",
            "delta": "console.",
        },
        {
            "type": "response.custom_tool_call_input.delta",
            "call_id": "call_js",
            "delta": "log(1);\n",
        },
        {
            "type": "response.output_item.done",
            "item": {
                "type": "custom_tool_call",
                "call_id": "call_js",
                "name": "exec",
                "input": "console.log(1);\n",
            },
        },
    )
    assert deltas == [
        ModelStreamFreeformInput(call_id="call_js", delta="console."),
        ModelStreamFreeformInput(call_id="call_js", delta="log(1);\n"),
        ModelStreamToolCallDone(
            call=ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);\n")
        ),
    ]
    assert state.saw_tool_call is True


def test_parse_resolves_call_id_through_the_item_map():
    state = ModelStreamState()
    _frames(
        state,
        {
            "type": "response.output_item.added",
            "item": {"type": "function_call", "id": "fc_1", "call_id": "call_fn", "name": "sum"},
        },
    )
    assert state.item_to_call_id == {"fc_1": "call_fn"}
    assert state.saw_tool_call is True

    # 1. event call_id wins.
    assert parse_model_sse_event(
        json.dumps(
            {
                "type": "response.function_call_arguments.delta",
                "call_id": "explicit",
                "item_id": "fc_1",
                "delta": "{",
            }
        ),
        state,
    ) == [ModelStreamFunctionArguments(call_id="explicit", delta="{")]
    # 2. item_id resolves through the map.
    assert parse_model_sse_event(
        json.dumps(
            {"type": "response.function_call_arguments.delta", "item_id": "fc_1", "delta": '"a"'}
        ),
        state,
    ) == [ModelStreamFunctionArguments(call_id="call_fn", delta='"a"')]
    # 3. unknown item_id falls through as itself.
    assert parse_model_sse_event(
        json.dumps(
            {"type": "response.function_call_arguments.delta", "item_id": "fc_9", "delta": "}"}
        ),
        state,
    ) == [ModelStreamFunctionArguments(call_id="fc_9", delta="}")]
    # 4. no id at all -> nothing.
    assert (
        parse_model_sse_event(
            json.dumps({"type": "response.function_call_arguments.delta", "delta": "}"}), state
        )
        == []
    )


def test_parse_completed_emits_usage_then_exactly_one_done():
    state = ModelStreamState()
    deltas = parse_model_sse_event(
        json.dumps(
            {
                "type": "response.completed",
                "response": {
                    "status": "completed",
                    "usage": {"input_tokens": 2, "output_tokens": 3},
                },
            }
        ),
        state,
    )
    assert deltas == [
        ModelStreamUsage(usage=Usage(input_tokens=2, output_tokens=3)),
        ModelStreamDone(stop_reason=StopReason.end_turn),
    ]
    assert state.saw_terminal is True
    assert sum(isinstance(d, ModelStreamDone) for d in deltas) == 1


def test_parse_incomplete_is_a_terminal_mapping_to_max_tokens():
    state = ModelStreamState()
    deltas = parse_model_sse_event(
        json.dumps(
            {
                "type": "response.incomplete",
                "response": {
                    "status": "incomplete",
                    "usage": {"input_tokens": 6, "output_tokens": 7},
                    "incomplete_details": {"reason": "max_output_tokens"},
                },
            }
        ),
        state,
    )
    assert deltas[-1] == ModelStreamDone(stop_reason=StopReason.max_tokens)
    assert state.saw_terminal is True


def test_parse_completed_after_tool_call_reports_tool_use_and_omits_zero_usage():
    state = ModelStreamState()
    state.saw_tool_call = True
    deltas = parse_model_sse_event(
        json.dumps({"type": "response.completed", "response": {"status": "completed"}}), state
    )
    assert deltas == [ModelStreamDone(stop_reason=StopReason.tool_use)]


def test_parse_skips_noise_frames():
    state = ModelStreamState()
    assert parse_model_sse_event("", state) == []
    assert parse_model_sse_event("   ", state) == []
    assert parse_model_sse_event("[DONE]", state) == []
    assert parse_model_sse_event("{not json", state) == []
    assert parse_model_sse_event("[1, 2]", state) == []
    assert parse_model_sse_event(json.dumps({"type": "response.created"}), state) == []
    assert state.error is None
    assert state.saw_terminal is False


def test_parse_records_stream_errors_without_yielding_a_delta():
    top_level = ModelStreamState()
    assert parse_model_sse_event(json.dumps({"type": "error", "message": "boom"}), top_level) == []
    assert top_level.error == "boom"

    nested = ModelStreamState()
    parse_model_sse_event(
        json.dumps({"type": "response.failed", "response": {"error": {"message": "nested"}}}),
        nested,
    )
    assert nested.error == "nested"

    sibling = ModelStreamState()
    parse_model_sse_event(json.dumps({"type": "error", "error": {"message": "sibling"}}), sibling)
    assert sibling.error == "sibling"

    bare = ModelStreamState()
    parse_model_sse_event(json.dumps({"type": "error"}), bare)
    assert bare.error == "responses stream error"
