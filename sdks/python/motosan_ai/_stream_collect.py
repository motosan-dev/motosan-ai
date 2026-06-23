"""Drive async StreamEvent iterators to completion and assemble ChatResponse."""

from __future__ import annotations

import json
from collections.abc import AsyncIterator
from typing import Any

from motosan_ai.types import ChatResponse, StopReason, StreamEvent, ToolCall, Usage


async def collect_stream(events: AsyncIterator[StreamEvent]) -> ChatResponse:
    """Collect a stream into one ChatResponse.

    Handles text, thinking, tool-call start/args/end, usage, and terminal
    stop_reason events. Malformed streamed tool arguments are treated as an
    empty object to match provider collector behavior. A mid-stream provider
    error (StreamError / NetworkError / ProviderError) raised by ``events``
    propagates out of this function uncollected.
    """
    content = ""
    thinking = ""
    tool_calls: list[ToolCall] = []
    usage = Usage(0, 0)
    stop_reason = StopReason.end_turn

    current_tc_id = ""
    current_tc_name = ""
    current_tc_args = ""

    async for event in events:
        if event.event_type == "text" and event.content:
            content += event.content
        elif event.event_type == "thinking" and event.content:
            thinking += event.content
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

    return ChatResponse(
        content=content,
        tool_calls=tool_calls,
        model="",
        usage=usage,
        stop_reason=stop_reason,
        thinking=thinking or None,
    )
