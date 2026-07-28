"""Drive async StreamEvent iterators to completion and assemble ChatResponse."""

from __future__ import annotations

import json
from collections.abc import AsyncIterator
from typing import Any

from motosan_ai.types import (
    ChatResponse,
    ModelChatResponse,
    ModelStreamDelta,
    ModelStreamDone,
    ModelStreamFreeformInput,
    ModelStreamFunctionArguments,
    ModelStreamText,
    ModelStreamThinkingDelta,
    ModelStreamThinkingDone,
    ModelStreamToolCallDone,
    ModelStreamUsage,
    ModelToolCall,
    ModelToolCallFreeform,
    StopReason,
    StreamEvent,
    ToolCall,
    Usage,
)


async def collect_stream(events: AsyncIterator[StreamEvent]) -> ChatResponse:
    """Collect a stream into one ChatResponse.

    Handles text, thinking_delta/thinking_done, tool-call start/args/end,
    usage, and terminal stop_reason events. thinking_done text takes
    priority over concatenated thinking_delta fallback. Malformed streamed
    tool arguments are treated as an empty object to match provider
    collector behavior. A mid-stream provider error (StreamError /
    NetworkError / ProviderError) raised by ``events`` propagates out of
    this function uncollected.
    """
    content = ""
    tool_calls: list[ToolCall] = []
    usage = Usage(0, 0)
    stop_reason: StopReason | None = None

    current_tc_id = ""
    current_tc_name = ""
    current_tc_args = ""
    session_id: str | None = None

    # Thinking accumulation (mirrors Rust stream.rs). thinking_delta_buf
    # collects every thinking_delta as a fallback in case the provider
    # does not emit thinking_done. thinking_done_buf holds the explicit
    # final text from the most recent thinking_done and takes priority
    # on assembly.
    thinking_delta_buf = ""
    thinking_done_buf: str | None = None

    async for event in events:
        # last-wins on session_id (HTTP providers never set it, so usually None);
        # CLI providers emit exactly one session event per stream.
        if event.session_id is not None:
            session_id = event.session_id
        if event.event_type == "text" and event.content:
            content += event.content
        elif event.event_type == "thinking_delta" and event.content:
            thinking_delta_buf += event.content
        elif event.event_type == "thinking_done":
            # thinking_done carries the full text; it wins over the delta
            # accumulator. Clear deltas so a second block starts fresh.
            thinking_done_buf = event.content
            thinking_delta_buf = ""
        elif event.event_type == "tool_call_start":
            current_tc_id = event.tool_call_id or ""
            current_tc_name = event.tool_call_name or ""
            current_tc_args = ""
        elif event.event_type == "tool_call_args":
            current_tc_args += event.tool_call_args_delta or ""
        elif event.event_type == "tool_call_end":
            parsed: dict[str, Any]
            try:
                candidate = json.loads(current_tc_args) if current_tc_args else {}
                parsed = candidate if isinstance(candidate, dict) else {}
            except json.JSONDecodeError:
                parsed = {}
            tool_calls.append(ToolCall(id=current_tc_id, name=current_tc_name, input=parsed))
            current_tc_id = ""
            current_tc_name = ""
            current_tc_args = ""
        elif event.event_type == "usage" and event.usage is not None:
            usage = Usage(
                input_tokens=event.usage.input_tokens or usage.input_tokens,
                output_tokens=event.usage.output_tokens or usage.output_tokens,
                cache_creation_input_tokens=(
                    event.usage.cache_creation_input_tokens
                    if event.usage.cache_creation_input_tokens is not None
                    else usage.cache_creation_input_tokens
                ),
                cache_read_input_tokens=(
                    event.usage.cache_read_input_tokens
                    if event.usage.cache_read_input_tokens is not None
                    else usage.cache_read_input_tokens
                ),
            )

        if event.done and event.stop_reason is not None:
            stop_reason = event.stop_reason

    if stop_reason is None:
        stop_reason = StopReason.tool_use if tool_calls else StopReason.end_turn

    if thinking_done_buf is not None:
        # Explicit empty thinking block -> treat as none (Rust parity).
        thinking = thinking_done_buf or None
    else:
        thinking = thinking_delta_buf or None

    return ChatResponse(
        content=content,
        tool_calls=tool_calls,
        model="",
        usage=usage,
        stop_reason=stop_reason,
        thinking=thinking,
        session_id=session_id,
    )


async def collect_model_stream(deltas: AsyncIterator[ModelStreamDelta]) -> ModelChatResponse:
    """Collect a native model stream into one ModelChatResponse.

    Unlike ``collect_stream``, this carries native function and freeform tool
    calls without lowering freeform input into JSON arguments.

    Three rules differ from the legacy collector and are contract, not taste:
    ``ModelStreamToolCallDone`` is authoritative (accumulated deltas are
    discarded, never merged); ``ModelStreamUsage`` REPLACES rather than
    merging field-by-field; and ``model`` is left empty for the caller to
    backfill. A mid-stream provider error raised by ``deltas`` propagates out
    uncollected.
    """
    content = ""
    thinking_delta_buf = ""
    thinking_done_buf: str | None = None
    function_arguments: dict[str, str] = {}
    freeform_inputs: dict[str, str] = {}
    tool_calls: list[ModelToolCall] = []
    usage = Usage(0, 0)
    explicit_stop_reason: StopReason | None = None

    async for delta in deltas:
        if isinstance(delta, ModelStreamText):
            content += delta.delta
        elif isinstance(delta, ModelStreamThinkingDelta):
            thinking_delta_buf += delta.delta
        elif isinstance(delta, ModelStreamThinkingDone):
            # Carries the full text; wins over the accumulator. Clearing the
            # delta buffer lets a second block start fresh.
            thinking_done_buf = delta.thinking
            thinking_delta_buf = ""
        elif isinstance(delta, ModelStreamFunctionArguments):
            function_arguments[delta.call_id] = (
                function_arguments.get(delta.call_id, "") + delta.delta
            )
        elif isinstance(delta, ModelStreamFreeformInput):
            freeform_inputs[delta.call_id] = freeform_inputs.get(delta.call_id, "") + delta.delta
        elif isinstance(delta, ModelStreamToolCallDone):
            # Authoritative: drop the bookkeeping, keep what the provider sent.
            if isinstance(delta.call, ModelToolCallFreeform):
                freeform_inputs.pop(delta.call.id, None)
            else:
                function_arguments.pop(delta.call.id, None)
            tool_calls.append(delta.call)
        elif isinstance(delta, ModelStreamUsage):
            # Replaces, never merges.
            usage = delta.usage
        elif isinstance(delta, ModelStreamDone):
            explicit_stop_reason = delta.stop_reason
            break

    if thinking_done_buf is not None:
        thinking = thinking_done_buf or None
    else:
        thinking = thinking_delta_buf or None

    if explicit_stop_reason is not None:
        stop_reason = explicit_stop_reason
    else:
        stop_reason = StopReason.tool_use if tool_calls else StopReason.end_turn

    return ModelChatResponse(
        content=content,
        thinking=thinking,
        tool_calls=tool_calls,
        model="",
        usage=usage,
        stop_reason=stop_reason,
        session_id=None,
    )
