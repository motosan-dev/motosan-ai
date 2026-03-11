from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum
from typing import Any


class Role(StrEnum):
    user = "user"
    assistant = "assistant"
    system = "system"
    tool = "tool"


class StopReason(StrEnum):
    end_turn = "end_turn"
    max_tokens = "max_tokens"
    tool_use = "tool_use"
    stop = "stop"
    other = "other"


@dataclass
class ToolCall:
    id: str
    name: str
    input: dict[str, Any]


@dataclass
class Message:
    role: Role
    content: str
    tool_call_id: str | None = None
    tool_calls: list[ToolCall] = field(default_factory=list)

    @classmethod
    def user(cls, content: str) -> "Message":
        return cls(role=Role.user, content=content)

    @classmethod
    def assistant(cls, content: str) -> "Message":
        return cls(role=Role.assistant, content=content)

    @classmethod
    def assistant_with_tool_calls(cls, content: str, tool_calls: list[ToolCall]) -> "Message":
        return cls(role=Role.assistant, content=content, tool_calls=tool_calls)

    @classmethod
    def system(cls, content: str) -> "Message":
        return cls(role=Role.system, content=content)

    @classmethod
    def tool_result(cls, tool_call_id: str, content: str) -> "Message":
        return cls(role=Role.tool, content=content, tool_call_id=tool_call_id)


@dataclass
class Tool:
    name: str
    description: str | None = None
    input_schema: dict[str, Any] | None = None


@dataclass
class Usage:
    input_tokens: int
    output_tokens: int


@dataclass
class ChatRequest:
    messages: list[Message]
    model: str | None = None
    system: str | None = None
    temperature: float | None = None
    max_tokens: int | None = None
    tools: list[Tool] | None = None
    provider_options: dict[str, Any] | None = None


@dataclass
class ChatResponse:
    content: str
    tool_calls: list[ToolCall] = field(default_factory=list)
    model: str = ""
    usage: Usage = field(default_factory=lambda: Usage(0, 0))
    stop_reason: StopReason = StopReason.end_turn


@dataclass
class StreamEvent:
    content: str
    done: bool
