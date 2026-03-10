from __future__ import annotations
from dataclasses import dataclass, field
from enum import Enum
from typing import Any


class Role(str, Enum):
    USER = "user"
    ASSISTANT = "assistant"
    SYSTEM = "system"


class StopReason(str, Enum):
    END_TURN = "end_turn"
    MAX_TOKENS = "max_tokens"
    TOOL_USE = "tool_use"
    STOP = "stop"
    OTHER = "other"


@dataclass
class Message:
    role: Role | str
    content: str

    @staticmethod
    def user(content: str) -> "Message":
        return Message(role=Role.USER, content=content)

    @staticmethod
    def assistant(content: str) -> "Message":
        return Message(role=Role.ASSISTANT, content=content)

    @staticmethod
    def system(content: str) -> "Message":
        return Message(role=Role.SYSTEM, content=content)


@dataclass
class Tool:
    name: str
    description: str
    parameters: dict[str, Any]


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
class Usage:
    input_tokens: int
    output_tokens: int


@dataclass
class ChatResponse:
    content: str
    model: str
    usage: Usage
    stop_reason: StopReason = StopReason.END_TURN


@dataclass
class StreamEvent:
    content: str
    done: bool = False
