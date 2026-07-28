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
from typing import Any

from motosan_ai.types import (
    FreeformTool,
    FunctionCallOutputContentItem,
    FunctionCallOutputInputImage,
    FunctionCallOutputInputText,
    FunctionCallOutputPayload,
    FunctionCallOutputText,
    ImageBlock,
    ImageSourceBase64,
    ImageSourceUrl,
    Message,
    ModelContextItem,
    ModelContextMessage,
    ModelContextToolCall,
    ModelToolCall,
    ModelToolCallFreeform,
    ModelToolCallFunction,
    ModelToolOutput,
    ModelToolOutputCustom,
    ModelToolOutputFunction,
    ModelToolSpec,
    ModelToolSpecFreeform,
    Role,
    TextBlock,
    ToolChoice,
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
