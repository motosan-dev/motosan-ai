from __future__ import annotations

from collections.abc import AsyncIterator

import pytest

from motosan_ai._stream_collect import collect_model_stream
from motosan_ai.error import StreamError
from motosan_ai.types import (
    ModelStreamDelta,
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
    StopReason,
    Usage,
)


async def _stream(*deltas: ModelStreamDelta) -> AsyncIterator[ModelStreamDelta]:
    for delta in deltas:
        yield delta


async def test_preserves_freeform_tool_call_and_usage():
    response = await collect_model_stream(
        _stream(
            ModelStreamFreeformInput(call_id="call_js", delta="console."),
            ModelStreamFreeformInput(call_id="call_js", delta="log(1);"),
            ModelStreamToolCallDone(
                call=ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);")
            ),
            ModelStreamUsage(usage=Usage(input_tokens=2, output_tokens=3)),
            ModelStreamDone(stop_reason=StopReason.tool_use),
        )
    )

    assert response.tool_calls == [
        ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);")
    ]
    assert response.stop_reason == StopReason.tool_use
    assert response.usage.output_tokens == 3
    assert response.model == ""
    assert response.content == ""
    assert response.thinking is None


async def test_tool_call_done_is_authoritative_over_accumulated_deltas():
    response = await collect_model_stream(
        _stream(
            ModelStreamFreeformInput(call_id="call_js", delta="WRONG"),
            ModelStreamFunctionArguments(call_id="call_fn", delta="ALSO WRONG"),
            ModelStreamToolCallDone(
                call=ModelToolCallFreeform(id="call_js", name="exec", input="RIGHT")
            ),
            ModelStreamToolCallDone(
                call=ModelToolCallFunction(id="call_fn", name="sum", arguments='{"a":1}')
            ),
            ModelStreamDone(stop_reason=StopReason.tool_use),
        )
    )

    assert response.tool_calls == [
        ModelToolCallFreeform(id="call_js", name="exec", input="RIGHT"),
        ModelToolCallFunction(id="call_fn", name="sum", arguments='{"a":1}'),
    ]


async def test_usage_replaces_rather_than_merges():
    response = await collect_model_stream(
        _stream(
            ModelStreamUsage(
                usage=Usage(input_tokens=100, output_tokens=100, cache_read_input_tokens=7)
            ),
            ModelStreamUsage(usage=Usage(input_tokens=0, output_tokens=5)),
            ModelStreamDone(stop_reason=StopReason.end_turn),
        )
    )
    assert response.usage == Usage(input_tokens=0, output_tokens=5)
    assert response.usage.cache_read_input_tokens is None


async def test_text_and_thinking_assembly():
    response = await collect_model_stream(
        _stream(
            ModelStreamThinkingDelta(delta="think "),
            ModelStreamThinkingDelta(delta="hard"),
            ModelStreamThinkingDone(thinking="think hard"),
            ModelStreamText(delta="ans"),
            ModelStreamText(delta="wer"),
            ModelStreamDone(stop_reason=StopReason.end_turn),
        )
    )
    assert response.content == "answer"
    assert response.thinking == "think hard"


async def test_thinking_done_does_not_duplicate_accumulated_deltas():
    response = await collect_model_stream(
        _stream(
            ModelStreamThinkingDelta(delta="same"),
            ModelStreamThinkingDone(thinking="same"),
            ModelStreamDone(stop_reason=StopReason.end_turn),
        )
    )
    assert response.thinking == "same"


async def test_thinking_falls_back_to_deltas_and_empty_done_means_none():
    from_deltas = await collect_model_stream(
        _stream(
            ModelStreamThinkingDelta(delta="A "),
            ModelStreamThinkingDelta(delta="B"),
            ModelStreamDone(stop_reason=StopReason.end_turn),
        )
    )
    assert from_deltas.thinking == "A B"

    empty_done = await collect_model_stream(
        _stream(
            ModelStreamThinkingDelta(delta="discarded"),
            ModelStreamThinkingDone(thinking=""),
            ModelStreamDone(stop_reason=StopReason.end_turn),
        )
    )
    assert empty_done.thinking is None


async def test_stop_reason_heuristic_only_applies_without_a_terminal():
    no_terminal = await collect_model_stream(_stream(ModelStreamText(delta="hi")))
    assert no_terminal.stop_reason == StopReason.end_turn

    tool_no_terminal = await collect_model_stream(
        _stream(
            ModelStreamToolCallDone(call=ModelToolCallFunction(id="c", name="n", arguments="{}"))
        )
    )
    assert tool_no_terminal.stop_reason == StopReason.tool_use

    explicit = await collect_model_stream(
        _stream(
            ModelStreamToolCallDone(call=ModelToolCallFunction(id="c", name="n", arguments="{}")),
            ModelStreamDone(stop_reason=StopReason.max_tokens),
        )
    )
    assert explicit.stop_reason == StopReason.max_tokens


async def test_stream_errors_propagate_uncollected():
    async def _boom() -> AsyncIterator[ModelStreamDelta]:
        yield ModelStreamText(delta="partial")
        raise StreamError("boom")

    with pytest.raises(StreamError, match="boom"):
        await collect_model_stream(_boom())
