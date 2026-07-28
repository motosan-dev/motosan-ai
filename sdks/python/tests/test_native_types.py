from __future__ import annotations

import dataclasses

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
    ModelChatResponse,
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
    Role,
    StopReason,
    SystemBlock,
    Tool,
    ToolChoice,
    Usage,
)


def grammar_fixture() -> FreeformTool:
    return FreeformTool(
        name="exec",
        description="Run JavaScript",
        format=FreeformToolFormat(type="grammar", syntax="lark", definition="start: source"),
    )


def test_freeform_format_is_mandatory_and_frozen():
    tool = grammar_fixture()
    assert tool.format.type == "grammar"
    assert tool.format.syntax == "lark"
    assert tool.format.definition == "start: source"
    # `format` has no default: constructing without it is a TypeError.
    try:
        FreeformTool(name="exec", description="Run JavaScript")  # type: ignore[call-arg]
    except TypeError as exc:
        assert "format" in str(exc)
    else:  # pragma: no cover - the type must not gain a default
        raise AssertionError("FreeformTool.format must be mandatory")
    assert dataclasses.is_dataclass(tool)
    assert tool == grammar_fixture()


def test_freeform_call_preserves_raw_input_verbatim():
    raw = "const x = {a: 1};\nconsole.log(`raw ${x.a}`);\n"
    call = ModelToolCallFreeform(id="call_js", name="exec", input=raw)
    assert call.input == raw
    assert call.input.encode() == raw.encode()
    assert call == ModelToolCallFreeform(id="call_js", name="exec", input=raw)


def test_function_call_and_freeform_call_are_distinct_variants():
    fn = ModelToolCallFunction(id="call_fn", name="sum", arguments='{"a":1}')
    ff = ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);")
    assert isinstance(fn, ModelToolCallFunction)
    assert not isinstance(fn, ModelToolCallFreeform)
    assert isinstance(ff, ModelToolCallFreeform)
    assert fn.arguments == '{"a":1}'
    assert ff.input == "console.log(1);"


def test_custom_output_carries_optional_call_identity():
    output = ModelToolOutputCustom(
        call_id="call_js", output=FunctionCallOutputText(text="stdout: 42"), name="exec"
    )
    assert output.call_id == "call_js"
    assert output.name == "exec"
    assert output.output == FunctionCallOutputText(text="stdout: 42")
    assert ModelToolOutputCustom(call_id="c", output=FunctionCallOutputText(text="")).name is None
    assert (
        ModelToolOutputFunction(
            call_id="call_fn", output=FunctionCallOutputText(text='{"ok":true}')
        ).call_id
        == "call_fn"
    )


def test_function_call_output_content_items():
    content = FunctionCallOutputContent(
        items=[
            FunctionCallOutputInputText(text="see this"),
            FunctionCallOutputInputImage(image_url="https://x.test/i.png", detail=ImageDetail.high),
            FunctionCallOutputEncryptedContent(encrypted_content="enc"),
        ]
    )
    assert len(content.items) == 3
    assert content.items[1].detail is ImageDetail.high
    assert FunctionCallOutputInputImage(image_url="u").detail is None
    assert ImageDetail.auto == "auto"
    assert ImageDetail.original == "original"


def test_native_context_preserves_mixed_item_order():
    request = ModelChatRequest(
        model="gpt-5.5-codex",
        context=[
            ModelContextMessage(message=Message.user("run it")),
            ModelContextToolCall(
                call=ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);")
            ),
            ModelContextToolOutput(
                output=ModelToolOutputCustom(
                    call_id="call_js",
                    output=FunctionCallOutputText(text="1\n"),
                    name="exec",
                )
            ),
        ],
        tool_specs=[ModelToolSpecFreeform(tool=grammar_fixture())],
    )

    assert request.model == "gpt-5.5-codex"
    assert len(request.context) == 3
    assert isinstance(request.context[0], ModelContextMessage)
    assert request.context[0].message.role == Role.user
    assert isinstance(request.context[1], ModelContextToolCall)
    assert isinstance(request.context[1].call, ModelToolCallFreeform)
    assert isinstance(request.context[2], ModelContextToolOutput)
    assert isinstance(request.context[2].output, ModelToolOutputCustom)


def test_model_chat_request_omits_the_reject_only_fields():
    # D3: thinking / mcp_servers / mcp_tool_configs exist in Rust only so
    # validation can reject them. Python omits them outright.
    names = {f.name for f in dataclasses.fields(ModelChatRequest)}
    assert "thinking" not in names
    assert "mcp_servers" not in names
    assert "mcp_tool_configs" not in names
    assert names == {
        "context",
        "tool_specs",
        "model",
        "system",
        "system_blocks",
        "system_cache",
        "temperature",
        "max_tokens",
        "tool_choice",
        "provider_options",
        "stop_sequences",
    }


def test_model_chat_request_defaults_are_independent():
    a = ModelChatRequest()
    b = ModelChatRequest()
    a.context.append(ModelContextMessage(message=Message.user("hi")))
    a.tool_specs.append(ModelToolSpecFunction(tool=Tool(name="sum")))
    assert b.context == []
    assert b.tool_specs == []
    assert a.model is None
    assert a.system_cache is False


def test_native_response_carries_freeform_calls_and_thinking():
    response = ModelChatResponse(
        content="answer",
        thinking="private reasoning",
        tool_calls=[ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);")],
        model="gpt-5.5-codex",
        usage=Usage(input_tokens=10, output_tokens=5),
        stop_reason=StopReason.tool_use,
    )
    assert response.content == "answer"
    assert response.thinking == "private reasoning"
    assert len(response.tool_calls) == 1
    assert isinstance(response.tool_calls[0], ModelToolCallFreeform)
    assert response.session_id is None
    # Non-frozen: providers backfill `model` after collecting a stream.
    response.model = "backfilled"
    assert response.model == "backfilled"


def test_model_chat_response_defaults():
    response = ModelChatResponse()
    assert response.content == ""
    assert response.thinking is None
    assert response.tool_calls == []
    assert response.model == ""
    assert response.usage == Usage(0, 0)
    assert response.stop_reason == StopReason.end_turn


def test_model_stream_delta_variants():
    deltas = [
        ModelStreamText(delta="hi"),
        ModelStreamThinkingDelta(delta="think"),
        ModelStreamThinkingDone(thinking="think hard"),
        ModelStreamFunctionArguments(call_id="call_fn", delta='{"a"'),
        ModelStreamFreeformInput(call_id="call_js", delta="console."),
        ModelStreamToolCallDone(
            call=ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);")
        ),
        ModelStreamUsage(usage=Usage(2, 3)),
        ModelStreamDone(stop_reason=StopReason.tool_use),
    ]
    assert deltas[0].delta == "hi"
    assert deltas[2].thinking == "think hard"
    assert deltas[3].call_id == "call_fn"
    assert deltas[4].delta == "console."
    assert isinstance(deltas[5].call, ModelToolCallFreeform)
    assert deltas[6].usage.output_tokens == 3
    assert deltas[7].stop_reason == StopReason.tool_use
    assert len({type(d) for d in deltas}) == 8


def test_builder_populates_every_field():
    request = (
        ModelChatRequest.builder()
        .model("gpt-5.5-codex")
        .system("  be terse  ")
        .temperature(0.25)
        .max_tokens(512)
        .tool_choice(ToolChoice.required())
        .provider_options({"reasoning_effort": "high"})
        .stop("END")
        .stop("STOP")
        .tool_spec(ModelToolSpecFreeform(tool=grammar_fixture()))
        .message(Message.user("run js"))
        .tool_call(ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);"))
        .tool_output(
            ModelToolOutputCustom(
                call_id="call_js", output=FunctionCallOutputText(text="1\n"), name="exec"
            )
        )
        .build()
    )

    assert isinstance(request, ModelChatRequest)
    assert request.model == "gpt-5.5-codex"
    assert request.system == "  be terse  "  # trimming happens in the codec, not here
    assert request.temperature == 0.25
    assert request.max_tokens == 512
    assert request.tool_choice == ToolChoice.required()
    assert request.provider_options == {"reasoning_effort": "high"}
    assert request.stop_sequences == ["END", "STOP"]
    assert len(request.tool_specs) == 1
    assert isinstance(request.tool_specs[0], ModelToolSpecFreeform)
    assert [type(item) for item in request.context] == [
        ModelContextMessage,
        ModelContextToolCall,
        ModelContextToolOutput,
    ]


def test_builder_bulk_setters_replace_and_copy():
    items = [ModelContextMessage(message=Message.user("a"))]
    specs = [ModelToolSpecFunction(tool=Tool(name="sum"))]
    seqs = ["X"]
    blocks = [SystemBlock.new("sys")]

    request = (
        ModelChatRequest.builder()
        .context(items)
        .tool_specs(specs)
        .stop_sequences(seqs)
        .system_blocks(blocks)
        .build()
    )

    items.append(ModelContextMessage(message=Message.user("b")))
    specs.append(ModelToolSpecFunction(tool=Tool(name="other")))
    seqs.append("Y")
    blocks.append(SystemBlock.new("more"))

    assert len(request.context) == 1
    assert len(request.tool_specs) == 1
    assert request.stop_sequences == ["X"]
    assert request.system_blocks is not None
    assert len(request.system_blocks) == 1


def test_builder_context_item_and_system_cached_and_system_block():
    request = (
        ModelChatRequest.builder()
        .context_item(ModelContextMessage(message=Message.system("sys msg")))
        .system_cached("cached system")
        .system_block(SystemBlock.cached("block one"))
        .build()
    )
    assert len(request.context) == 1
    assert request.system == "cached system"
    assert request.system_cache is True
    assert request.system_blocks is not None
    assert request.system_blocks[0].cache_control is True


def test_builder_defaults_are_empty():
    request = ModelChatRequest.builder().build()
    assert request.context == []
    assert request.tool_specs == []
    assert request.model is None
    assert request.system is None
    assert request.system_blocks is None
    assert request.system_cache is False
    assert request.temperature is None
    assert request.max_tokens is None
    assert request.tool_choice is None
    assert request.provider_options is None
    assert request.stop_sequences is None
