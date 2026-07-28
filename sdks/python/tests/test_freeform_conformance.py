"""Freeform / native-model-API conformance gates.

Anchored to specs/types.md § Native Model API. Cross-SDK mirrors:
- sdks/rust/tests/freeform_conformance.rs
- sdks/typescript/tests/freeform-conformance.test.ts

Expected values come from the Rust tests that already pin this behaviour
(tests/core_types.rs, tests/openai_provider.rs, tests/chatgpt_codex.rs,
tests/native_collect_stream.rs). Do not invent new fixtures here.

Proving this suite still bites
------------------------------
A conformance suite passes by construction the day it is written, so passing
says nothing. Re-prove it after any refactor of the native surface by making
each mutation below in turn, running this file, and confirming the named test
fails — then reverting. Every one was verified against the suite as merged.

1. ``providers/responses.py`` — in ``encode_tool_call``, replace
   ``"input": call.input`` with a JSON round-trip such as
   ``json.dumps(json.loads(call.input))``.
   Fails: ``test_freeform_input_is_never_parsed_as_json_or_lowered_into_arguments``
   (and ``test_ordered_mixed_history_replays_in_order``).
2. ``_stream_collect.py`` — in ``collect_model_stream``, delete
   ``tool_calls.append(delta.call)`` from the ToolCallDone arm so the
   accumulated deltas would win instead.
   Fails: ``test_tool_call_done_is_authoritative``.
3. ``providers/chatgpt_codex.py`` — change the incomplete-stream payload from
   ``chatgpt-codex`` to ``chatgpt_codex`` (the legacy adapter's spelling).
   Fails: ``test_codex_eof_without_terminal_raises_incomplete_stream``.
4. ``provider_base.py`` — make ``ProviderCapabilities.full()`` return
   ``supports_freeform_tools=True``.
   Fails: ``test_capability_matrix_matches_the_spec``.

Deleting a whole module and watching the import fail is NOT such a check: an
empty file with zero assertions produces the identical collection error.
"""

from __future__ import annotations

import json
from collections.abc import AsyncIterator

import httpx
import pytest
import respx

from motosan_ai._stream_collect import collect_model_stream
from motosan_ai.error import IncompleteStreamError, StreamError, UnsupportedFeatureError
from motosan_ai.providers.chatgpt_codex import ChatGptCodexProvider
from motosan_ai.providers.openai import OpenAIProvider
from motosan_ai.providers.responses import decode_tool_call, encode_input, encode_tool_call
from motosan_ai.types import (
    FreeformTool,
    FreeformToolFormat,
    FunctionCallOutputText,
    Message,
    ModelChatRequest,
    ModelStreamDelta,
    ModelStreamDone,
    ModelStreamFreeformInput,
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
    StopReason,
    Usage,
)

_CODEX_URL = "https://chatgpt.com/backend-api/codex/responses"
_OPENAI_RESPONSES = "https://api.openai.com/v1/responses"

FREEFORM_SPEC = ModelToolSpecFreeform(
    tool=FreeformTool(
        name="exec",
        description="Run JavaScript",
        format=FreeformToolFormat(type="grammar", syntax="lark", definition="start: source"),
    )
)


def _sse(*payloads: dict) -> str:
    return "\n".join(f"data: {json.dumps(p)}" for p in payloads) + "\n"


def _native_request() -> ModelChatRequest:
    return (
        ModelChatRequest.builder().message(Message.user("run js")).tool_spec(FREEFORM_SPEC).build()
    )


async def _stream(*deltas: ModelStreamDelta) -> AsyncIterator[ModelStreamDelta]:
    for delta in deltas:
        yield delta


# --- Freeform input survives byte-for-byte -------------------------------


def test_freeform_input_is_never_parsed_as_json_or_lowered_into_arguments():
    raw = '{"this":"looks like json"}\nconsole.log(\'but is JavaScript\');'
    encoded = encode_tool_call(ModelToolCallFreeform(id="call_js", name="exec", input=raw))

    assert encoded["type"] == "custom_tool_call"
    assert encoded["input"] == raw
    assert encoded["input"].encode() == raw.encode()
    assert "arguments" not in encoded

    decoded = decode_tool_call(encoded)
    assert decoded == ModelToolCallFreeform(id="call_js", name="exec", input=raw)


def test_ordered_mixed_history_replays_in_order():
    raw = '{"not":"function args"}\nvalue.not;\n'
    request = (
        ModelChatRequest.builder()
        .message(Message.user("run js"))
        .tool_call(ModelToolCallFunction(id="call_fn", name="sum", arguments='{"a":1}'))
        .tool_output(
            ModelToolOutputFunction(call_id="call_fn", output=FunctionCallOutputText(text="1"))
        )
        .tool_call(ModelToolCallFreeform(id="call_js", name="exec", input=raw))
        .tool_output(
            ModelToolOutputCustom(
                call_id="call_js",
                output=FunctionCallOutputText(text="function args"),
                name="exec",
            )
        )
        .build()
    )

    encoded = encode_input(request.context)
    assert [item["type"] for item in encoded] == [
        "message",
        "function_call",
        "function_call_output",
        "custom_tool_call",
        "custom_tool_call_output",
    ]
    assert encoded[3]["input"].encode() == raw.encode()
    assert "arguments" not in encoded[3]


# --- Collector contract (specs/types.md § Stream termination (native)) ----


async def test_tool_call_done_is_authoritative():
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


async def test_usage_replaces_rather_than_merges():
    response = await collect_model_stream(
        _stream(
            ModelStreamUsage(usage=Usage(input_tokens=99, output_tokens=99)),
            ModelStreamUsage(usage=Usage(input_tokens=0, output_tokens=5)),
            ModelStreamDone(stop_reason=StopReason.end_turn),
        )
    )
    assert response.usage == Usage(input_tokens=0, output_tokens=5)


async def test_thinking_done_wins_over_accumulated_deltas():
    response = await collect_model_stream(
        _stream(
            ModelStreamThinkingDelta(delta="think "),
            ModelStreamThinkingDelta(delta="hard"),
            ModelStreamThinkingDone(thinking="think hard"),
            ModelStreamText(delta="answer"),
            ModelStreamDone(stop_reason=StopReason.end_turn),
        )
    )
    assert response.thinking == "think hard"
    assert response.content == "answer"


# --- Exactly one terminal per completed stream ---------------------------


@respx.mock
async def test_exactly_one_done_per_successfully_completed_stream():
    respx.post(_CODEX_URL).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {"type": "response.output_text.delta", "delta": "hi"},
                {
                    "type": "response.completed",
                    "response": {
                        "status": "completed",
                        "usage": {"input_tokens": 1, "output_tokens": 1},
                    },
                },
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    provider = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    deltas = [delta async for delta in provider.model_stream(_native_request())]
    assert sum(isinstance(d, ModelStreamDone) for d in deltas) == 1
    assert isinstance(deltas[-1], ModelStreamDone)


@respx.mock
async def test_response_incomplete_is_a_received_terminal():
    respx.post(_CODEX_URL).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {"type": "response.output_text.delta", "delta": "partial"},
                {
                    "type": "response.incomplete",
                    "response": {
                        "status": "incomplete",
                        "usage": {"input_tokens": 6, "output_tokens": 7},
                    },
                },
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    provider = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    response = await provider.model_chat(
        ModelChatRequest.builder().message(Message.user("short")).build()
    )
    assert response.content == "partial"
    assert response.stop_reason == StopReason.max_tokens
    assert response.usage.output_tokens == 7


# --- EOF without a terminal, both provider strings -----------------------


@respx.mock
async def test_codex_eof_without_terminal_raises_incomplete_stream():
    respx.post(_CODEX_URL).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {
                    "type": "response.custom_tool_call_input.delta",
                    "call_id": "call_js",
                    "delta": "console.",
                }
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    provider = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    with pytest.raises(IncompleteStreamError) as exc:
        await collect_model_stream(provider.model_stream(_native_request()))
    assert str(exc.value) == "incomplete stream: chatgpt-codex ended without a terminal event"


@respx.mock
async def test_openai_eof_without_terminal_raises_incomplete_stream():
    respx.post(_OPENAI_RESPONSES).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {"type": "response.output_text.delta", "delta": "hel"},
                {"type": "response.output_text.delta", "delta": "lo"},
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    provider = OpenAIProvider(api_key="k", model="gpt-5.5-codex", responses_api=True)
    with pytest.raises(IncompleteStreamError) as exc:
        await collect_model_stream(provider.model_stream(_native_request()))
    assert str(exc.value) == "incomplete stream: openai ended without a terminal event"


def test_incomplete_stream_error_is_a_stream_error():
    assert issubclass(IncompleteStreamError, StreamError)


# --- Pending deltas drain before a stored stream error surfaces ----------


@respx.mock
async def test_pending_deltas_drain_before_a_stream_error():
    respx.post(_CODEX_URL).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {"type": "response.output_text.delta", "delta": "before"},
                {"type": "response.failed", "response": {"error": {"message": "upstream died"}}},
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    provider = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    seen: list[ModelStreamDelta] = []
    with pytest.raises(StreamError, match="upstream died"):
        async for delta in provider.model_stream(_native_request()):
            seen.append(delta)
    assert seen == [ModelStreamText(delta="before")]


# --- Pre-network rejection ------------------------------------------------


@respx.mock
async def test_unsupported_provider_rejects_freeform_before_network():
    route = respx.post(host="api.openai.com").mock(return_value=httpx.Response(500))
    provider = OpenAIProvider(api_key="k")  # no Responses opt-in

    with pytest.raises(UnsupportedFeatureError, match="native freeform tools"):
        await provider.model_chat(_native_request())
    with pytest.raises(UnsupportedFeatureError, match="native freeform tools"):
        async for _ in provider.model_stream(_native_request()):
            pass
    assert route.call_count == 0
    assert isinstance(UnsupportedFeatureError("x"), Exception)


def test_capability_matrix_matches_the_spec():
    from motosan_ai.provider_base import ProviderCapabilities

    codex = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    assert codex.capabilities == ProviderCapabilities.with_freeform_tools()

    plain_openai = OpenAIProvider(api_key="k")
    assert plain_openai.capabilities == ProviderCapabilities.with_image()

    responses_openai = OpenAIProvider(api_key="k", responses_api=True)
    assert responses_openai.capabilities == ProviderCapabilities.with_image_and_freeform_tools()

    # full() deliberately leaves freeform false.
    assert ProviderCapabilities.full().supports_freeform_tools is False
