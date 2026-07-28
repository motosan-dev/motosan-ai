"""Shared OpenAI Responses codec for the native model API.

Port of Rust ``sdks/rust/src/providers/responses.rs``. Pure encoding and
decoding — no HTTP, no provider state. Consumed by
``motosan_ai/providers/openai.py`` (behind the Responses opt-in) and
``motosan_ai/providers/chatgpt_codex.py`` (native by default).

Normative contract: ``specs/types.md`` § Native Model API.
"""

from __future__ import annotations

import json
from collections.abc import Sequence
from dataclasses import dataclass, field
from typing import Any

from motosan_ai.types import (
    FreeformTool,
    FunctionCallOutputContent,
    FunctionCallOutputContentItem,
    FunctionCallOutputEncryptedContent,
    FunctionCallOutputInputImage,
    FunctionCallOutputInputText,
    FunctionCallOutputPayload,
    FunctionCallOutputText,
    ImageBlock,
    ImageDetail,
    ImageSourceBase64,
    ImageSourceUrl,
    Message,
    ModelChatRequest,
    ModelChatResponse,
    ModelContextItem,
    ModelContextMessage,
    ModelContextToolCall,
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
    ModelToolCallFunction,
    ModelToolOutput,
    ModelToolOutputCustom,
    ModelToolOutputFunction,
    ModelToolSpec,
    ModelToolSpecFreeform,
    Role,
    StopReason,
    TextBlock,
    ToolChoice,
    Usage,
)


def encode_freeform_tool(tool: FreeformTool) -> dict[str, Any]:
    """Wire shape for a Freeform tool spec.

    The ``type: "custom"`` key is injected here — ``FreeformTool`` never
    stores it.
    """
    return {
        "type": "custom",
        "name": tool.name,
        "description": tool.description,
        "format": {
            "type": tool.format.type,
            "syntax": tool.format.syntax,
            "definition": tool.format.definition,
        },
    }


def encode_tool_spec(spec: ModelToolSpec) -> dict[str, Any]:
    if isinstance(spec, ModelToolSpecFreeform):
        return encode_freeform_tool(spec.tool)
    return {
        "type": "function",
        "name": spec.tool.name,
        "description": spec.tool.description,
        # Wire key differs from the field name: input_schema -> parameters.
        "parameters": spec.tool.input_schema,
    }


def encode_tools(specs: Sequence[ModelToolSpec]) -> list[dict[str, Any]]:
    return [encode_tool_spec(spec) for spec in specs]


def encode_tool_call(call: ModelToolCall) -> dict[str, Any]:
    """Wire shape for a tool call. ``id`` is spelled ``call_id`` on the wire."""
    if isinstance(call, ModelToolCallFreeform):
        return {
            "type": "custom_tool_call",
            "call_id": call.id,
            "name": call.name,
            # Verbatim. Never parsed as JSON, never written to "arguments".
            "input": call.input,
        }
    return {
        "type": "function_call",
        "call_id": call.id,
        "name": call.name,
        "arguments": call.arguments,
    }


def encode_function_call_output_content_item(
    item: FunctionCallOutputContentItem,
) -> dict[str, Any]:
    if isinstance(item, FunctionCallOutputInputText):
        return {"type": "input_text", "text": item.text}
    if isinstance(item, FunctionCallOutputInputImage):
        encoded: dict[str, Any] = {"type": "input_image", "image_url": item.image_url}
        if item.detail is not None:
            encoded["detail"] = item.detail.value
        return encoded
    return {"type": "encrypted_content", "encrypted_content": item.encrypted_content}


def encode_function_call_output_payload(payload: FunctionCallOutputPayload) -> Any:
    """Text payloads encode to a bare string; content payloads to a list."""
    if isinstance(payload, FunctionCallOutputText):
        return payload.text
    return [encode_function_call_output_content_item(item) for item in payload.items]


def encode_tool_output(output: ModelToolOutput) -> dict[str, Any]:
    """Wire shape for a tool output item.

    TRAP: the custom arm deliberately DROPS ``name``. Rust's codec
    ``encode_tool_output`` emits only ``type`` / ``call_id`` / ``output``, and
    ``responses_codec_encodes_function_and_custom_outputs`` asserts the wire
    item has no ``name``. Use ``tool_output_to_dict`` when call identity must
    survive a round trip.
    """
    encoded_output = encode_function_call_output_payload(output.output)
    if isinstance(output, ModelToolOutputCustom):
        return {
            "type": "custom_tool_call_output",
            "call_id": output.call_id,
            "output": encoded_output,
        }
    return {
        "type": "function_call_output",
        "call_id": output.call_id,
        "output": encoded_output,
    }


def tool_output_to_dict(output: ModelToolOutput) -> dict[str, Any]:
    """Identity-preserving encoding: keeps ``name`` on custom outputs.

    Mirrors Rust's ``impl Serialize for ModelToolOutput`` rather than the
    codec's ``encode_tool_output``, and round-trips through
    ``decode_tool_output``. Not used to build request bodies.
    """
    encoded = encode_tool_output(output)
    if isinstance(output, ModelToolOutputCustom) and output.name is not None:
        encoded["name"] = output.name
    return encoded


def encode_user_content(message: Message) -> list[dict[str, Any]]:
    if not message.content_blocks:
        return [{"type": "input_text", "text": message.content}]

    content: list[dict[str, Any]] = []
    for block in message.content_blocks:
        if isinstance(block, TextBlock):
            content.append({"type": "input_text", "text": block.text})
        elif isinstance(block, ImageBlock):
            source = block.source
            if isinstance(source, ImageSourceBase64):
                content.append(
                    {
                        "type": "input_image",
                        "image_url": f"data:{source.media_type};base64,{source.data}",
                    }
                )
            elif isinstance(source, ImageSourceUrl):
                content.append({"type": "input_image", "image_url": source.url})
        # Document blocks have no Responses representation and are skipped.

    if not content:
        content.append({"type": "input_text", "text": message.content})
    return content


def encode_message(message: Message) -> list[dict[str, Any]]:
    """Expand one Message into zero or more Responses input items.

    System messages encode to nothing — ``build_model_request_body`` hoists
    them into ``instructions`` instead.
    """
    if message.role == Role.system:
        return []
    if message.role == Role.user:
        return [{"type": "message", "role": "user", "content": encode_user_content(message)}]
    if message.role == Role.assistant:
        items: list[dict[str, Any]] = []
        if message.content:
            items.append(
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": message.content}],
                }
            )
        items.extend(
            encode_tool_call(
                ModelToolCallFunction(
                    id=tool_call.id,
                    name=tool_call.name,
                    arguments=json.dumps(tool_call.input),
                )
            )
            for tool_call in message.tool_calls
        )
        return items
    if message.tool_call_id is None:
        return []
    return [
        encode_tool_output(
            ModelToolOutputFunction(
                call_id=message.tool_call_id,
                output=FunctionCallOutputText(text=message.content),
            )
        )
    ]


def encode_context_item(item: ModelContextItem) -> list[dict[str, Any]]:
    if isinstance(item, ModelContextMessage):
        return encode_message(item.message)
    if isinstance(item, ModelContextToolCall):
        return [encode_tool_call(item.call)]
    return [encode_tool_output(item.output)]


def encode_input(context: Sequence[ModelContextItem]) -> list[dict[str, Any]]:
    encoded: list[dict[str, Any]] = []
    for item in context:
        encoded.extend(encode_context_item(item))
    return encoded


def encode_tool_choice(choice: ToolChoice) -> Any:
    if choice.type == "tool":
        return {"type": "function", "name": choice.name}
    return choice.type


def decode_function_call_output_payload(value: Any) -> FunctionCallOutputPayload:
    if isinstance(value, list):
        items: list[FunctionCallOutputContentItem] = []
        for raw in value:
            if not isinstance(raw, dict):
                continue
            kind = raw.get("type")
            if kind == "input_text":
                items.append(FunctionCallOutputInputText(text=str(raw.get("text", ""))))
            elif kind == "input_image":
                detail = raw.get("detail")
                items.append(
                    FunctionCallOutputInputImage(
                        image_url=str(raw.get("image_url", "")),
                        detail=ImageDetail(detail) if isinstance(detail, str) else None,
                    )
                )
            elif kind == "encrypted_content":
                items.append(
                    FunctionCallOutputEncryptedContent(
                        encrypted_content=str(raw.get("encrypted_content", ""))
                    )
                )
        return FunctionCallOutputContent(items=items)
    if isinstance(value, str):
        return FunctionCallOutputText(text=value)
    return FunctionCallOutputText(text=json.dumps(value))


def decode_tool_call(item: Any) -> ModelToolCall | None:
    """Decode one Responses output item into a native tool call.

    Accepts ``call_id`` OR ``id`` as the call identity, in that order.
    Returns None for items that are not tool calls.
    """
    if not isinstance(item, dict):
        return None
    kind = item.get("type")
    if kind not in ("function_call", "custom_tool_call"):
        return None
    call_id = item.get("call_id")
    if not isinstance(call_id, str):
        call_id = item.get("id")
    if not isinstance(call_id, str):
        return None
    name = item.get("name")
    if not isinstance(name, str):
        return None
    if kind == "custom_tool_call":
        raw_input = item.get("input")
        return ModelToolCallFreeform(
            id=call_id,
            name=name,
            # Verbatim: never json.loads'd, however JSON-shaped it looks.
            input=raw_input if isinstance(raw_input, str) else "",
        )
    arguments = item.get("arguments")
    return ModelToolCallFunction(
        id=call_id,
        name=name,
        arguments=arguments if isinstance(arguments, str) else "",
    )


def decode_tool_output(item: Any) -> ModelToolOutput | None:
    if not isinstance(item, dict):
        return None
    kind = item.get("type")
    if kind not in ("function_call_output", "custom_tool_call_output"):
        return None
    call_id = item.get("call_id")
    if not isinstance(call_id, str) or "output" not in item:
        return None
    payload = decode_function_call_output_payload(item["output"])
    if kind == "custom_tool_call_output":
        name = item.get("name")
        return ModelToolOutputCustom(
            call_id=call_id,
            output=payload,
            name=name if isinstance(name, str) else None,
        )
    return ModelToolOutputFunction(call_id=call_id, output=payload)


def decode_usage(value: Any) -> Usage:
    """Accepts both the Responses and the Chat Completions token spellings."""
    if not isinstance(value, dict):
        return Usage(input_tokens=0, output_tokens=0)

    raw_input = value.get("input_tokens")
    if raw_input is None:
        raw_input = value.get("prompt_tokens")
    raw_output = value.get("output_tokens")
    if raw_output is None:
        raw_output = value.get("completion_tokens")

    cached: int | None = None
    details = value.get("input_tokens_details")
    if isinstance(details, dict):
        raw_cached = details.get("cached_tokens")
        if isinstance(raw_cached, int) and raw_cached > 0:
            cached = raw_cached

    return Usage(
        input_tokens=int(raw_input or 0),
        output_tokens=int(raw_output or 0),
        cache_creation_input_tokens=None,
        cache_read_input_tokens=cached,
    )


def stop_reason_from_status(status: str | None, has_tool_calls: bool) -> StopReason:
    if has_tool_calls:
        return StopReason.tool_use
    if status == "incomplete":
        return StopReason.max_tokens
    if status is None or status == "completed":
        return StopReason.end_turn
    return StopReason.other


def decode_output_text(item: Any) -> str | None:
    """Concatenate the ``output_text`` parts of a ``message`` output item."""
    if not isinstance(item, dict) or item.get("type") != "message":
        return None
    content = item.get("content")
    if not isinstance(content, list):
        return None
    parts: list[str] = []
    for part in content:
        if not isinstance(part, dict) or part.get("type") != "output_text":
            continue
        text = part.get("text")
        if isinstance(text, str):
            parts.append(text)
    return "".join(parts) or None


def model_chat_response_from_output(payload: Any, default_model: str) -> ModelChatResponse:
    """Decode a non-streaming Responses payload into a ModelChatResponse."""
    payload = payload if isinstance(payload, dict) else {}
    content = ""
    thinking: str | None = None
    tool_calls: list[ModelToolCall] = []

    output_text = payload.get("output_text")
    if isinstance(output_text, str):
        content += output_text

    output_items = payload.get("output")
    if isinstance(output_items, list):
        for item in output_items:
            text = decode_output_text(item)
            if text is not None:
                content += text
            if isinstance(item, dict) and item.get("type") == "reasoning":
                summary = item.get("summary")
                if isinstance(summary, list):
                    summary_parts: list[str] = []
                    for part in summary:
                        if not isinstance(part, dict):
                            continue
                        value = part.get("text")
                        if not isinstance(value, str):
                            value = part.get("content")
                        if isinstance(value, str):
                            summary_parts.append(value)
                    joined = "".join(summary_parts)
                    if joined:
                        thinking = joined
            call = decode_tool_call(item)
            if call is not None:
                tool_calls.append(call)

    status = payload.get("status")
    model = payload.get("model")
    return ModelChatResponse(
        content=content,
        thinking=thinking,
        tool_calls=tool_calls,
        model=model if isinstance(model, str) else default_model,
        usage=decode_usage(payload.get("usage")),
        stop_reason=stop_reason_from_status(
            status if isinstance(status, str) else None, bool(tool_calls)
        ),
        session_id=None,
    )


def build_model_request_body(
    request: ModelChatRequest,
    default_model: str,
    *,
    stream: bool,
    default_instructions: str | None = None,
) -> dict[str, Any]:
    """Encode a ModelChatRequest into an OpenAI Responses request body.

    Two rules here are load-bearing and easy to miss:

    1. ``Role.system`` messages inside ``context`` are hoisted into
       ``instructions`` AND removed from ``input``.
    2. ``provider_options`` is shallow-merged LAST, so it can override
       anything this encoder produced. Callers that must win over
       ``provider_options`` (ChatGPT Codex does) have to apply their
       overrides after calling this function.
    """
    model = request.model or default_model

    instructions_parts: list[str] = []
    if request.system_blocks is not None:
        for block in request.system_blocks:
            trimmed = block.text.strip()
            if trimmed:
                instructions_parts.append(trimmed)
    elif request.system is not None:
        trimmed = request.system.strip()
        if trimmed:
            instructions_parts.append(trimmed)

    input_context: list[ModelContextItem] = []
    for item in request.context:
        if isinstance(item, ModelContextMessage) and item.message.role == Role.system:
            trimmed = item.message.content.strip()
            if trimmed:
                instructions_parts.append(trimmed)
            continue
        input_context.append(item)

    body: dict[str, Any] = {"model": model, "input": encode_input(input_context)}

    if stream:
        body["stream"] = True
    if request.tool_specs:
        body["tools"] = encode_tools(request.tool_specs)

    instructions = "\n\n".join(instructions_parts) if instructions_parts else default_instructions
    if instructions is not None:
        body["instructions"] = instructions

    if request.temperature is not None:
        body["temperature"] = request.temperature
    if request.max_tokens is not None:
        # Wire key differs from the field name.
        body["max_output_tokens"] = request.max_tokens
    if request.tool_choice is not None:
        body["tool_choice"] = encode_tool_choice(request.tool_choice)
    if request.stop_sequences:
        body["stop"] = list(request.stop_sequences)

    # Shallow merge, LAST.
    if request.provider_options:
        body.update(request.provider_options)

    return body


@dataclass
class ModelStreamState:
    """Per-stream adapter state for ``parse_model_sse_event``.

    ``item_to_call_id`` maps Responses output-item ids (``fc_*``) to public
    call ids (``call_*``), because argument/input deltas arrive keyed by
    ``item_id``. ``saw_terminal`` lets the transport tell truncation from
    completion at EOF.
    """

    item_to_call_id: dict[str, str] = field(default_factory=dict)
    saw_tool_call: bool = False
    saw_terminal: bool = False
    error: str | None = None


def _remember_output_item(item: Any, state: ModelStreamState) -> None:
    if not isinstance(item, dict):
        return
    if item.get("type") not in ("function_call", "custom_tool_call"):
        return
    call_id = item.get("call_id")
    if not isinstance(call_id, str):
        return
    state.saw_tool_call = True
    item_id = item.get("id")
    if isinstance(item_id, str) and item_id:
        state.item_to_call_id[item_id] = call_id


def _call_id_from_event(chunk: dict[str, Any], state: ModelStreamState) -> str | None:
    """Event ``call_id`` -> mapped ``item_id`` -> raw ``item_id``."""
    call_id = chunk.get("call_id")
    if isinstance(call_id, str):
        return call_id
    item_id = chunk.get("item_id")
    if isinstance(item_id, str):
        return state.item_to_call_id.get(item_id, item_id)
    return None


def _stream_error_message(chunk: dict[str, Any]) -> str:
    message = chunk.get("message")
    if isinstance(message, str) and message:
        return message
    response = chunk.get("response")
    if isinstance(response, dict):
        error = response.get("error")
        if isinstance(error, dict) and isinstance(error.get("message"), str):
            nested = error["message"]
            if nested:
                return str(nested)
    error = chunk.get("error")
    if isinstance(error, dict) and isinstance(error.get("message"), str):
        sibling = error["message"]
        if sibling:
            return str(sibling)
    return "responses stream error"


def parse_model_sse_event(data: str, state: ModelStreamState) -> list[ModelStreamDelta]:
    """Map one Responses SSE ``data`` payload to zero or more ModelStreamDeltas.

    Pure apart from mutating ``state``. Port of Rust's
    ``ResponsesModelStreamAdapter::handle_event``. A fatal ``error`` /
    ``response.failed`` frame sets ``state.error`` and returns ``[]`` — the
    transport raises StreamError after draining the pending deltas.
    """
    text = data.strip()
    if not text or text == "[DONE]":
        return []
    try:
        chunk = json.loads(text)
    except json.JSONDecodeError:
        return []
    if not isinstance(chunk, dict):
        return []

    event_type = chunk.get("type")
    out: list[ModelStreamDelta] = []

    if event_type == "response.output_text.delta":
        delta = chunk.get("delta")
        if isinstance(delta, str) and delta:
            out.append(ModelStreamText(delta=delta))

    elif event_type in (
        "response.reasoning_text.delta",
        "response.reasoning_summary_text.delta",
    ):
        delta = chunk.get("delta")
        if isinstance(delta, str) and delta:
            out.append(ModelStreamThinkingDelta(delta=delta))

    elif event_type in (
        "response.reasoning_text.done",
        "response.reasoning_summary_text.done",
    ):
        thinking = chunk.get("text")
        if not isinstance(thinking, str):
            thinking = chunk.get("delta")
        if isinstance(thinking, str):
            out.append(ModelStreamThinkingDone(thinking=thinking))

    elif event_type == "response.output_item.added":
        _remember_output_item(chunk.get("item"), state)

    elif event_type == "response.function_call_arguments.delta":
        call_id = _call_id_from_event(chunk, state)
        delta = chunk.get("delta")
        if call_id is not None and isinstance(delta, str):
            out.append(ModelStreamFunctionArguments(call_id=call_id, delta=delta))

    elif event_type == "response.custom_tool_call_input.delta":
        call_id = _call_id_from_event(chunk, state)
        delta = chunk.get("delta")
        if call_id is not None and isinstance(delta, str):
            out.append(ModelStreamFreeformInput(call_id=call_id, delta=delta))

    elif event_type == "response.output_item.done":
        item = chunk.get("item")
        _remember_output_item(item, state)
        call = decode_tool_call(item)
        if call is not None:
            state.saw_tool_call = True
            out.append(ModelStreamToolCallDone(call=call))

    elif event_type in ("response.completed", "response.incomplete"):
        response = chunk.get("response")
        response = response if isinstance(response, dict) else {}
        usage = decode_usage(response.get("usage"))
        if (
            usage.input_tokens != 0
            or usage.output_tokens != 0
            or usage.cache_creation_input_tokens is not None
            or usage.cache_read_input_tokens is not None
        ):
            out.append(ModelStreamUsage(usage=usage))
        status = response.get("status")
        out.append(
            ModelStreamDone(
                stop_reason=stop_reason_from_status(
                    status if isinstance(status, str) else None, state.saw_tool_call
                )
            )
        )
        state.saw_terminal = True

    elif event_type in ("error", "response.failed"):
        state.error = _stream_error_message(chunk)

    return out
