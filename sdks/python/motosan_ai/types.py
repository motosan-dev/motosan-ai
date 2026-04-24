from __future__ import annotations

from dataclasses import dataclass, field, replace
from enum import StrEnum
from typing import Any, Literal


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
    stop_sequence = "stop_sequence"
    other = "other"


class StreamEventType(StrEnum):
    text = "text"
    tool_call_start = "tool_call_start"
    tool_call_args = "tool_call_args"
    tool_call_end = "tool_call_end"
    usage = "usage"


@dataclass(frozen=True)
class ImageSourceBase64:
    media_type: str
    data: str
    type: Literal["base64"] = "base64"


@dataclass(frozen=True)
class ImageSourceUrl:
    url: str
    type: Literal["url"] = "url"


ImageSource = ImageSourceBase64 | ImageSourceUrl


def image_source_to_dict(source: ImageSource) -> dict[str, str]:
    if isinstance(source, ImageSourceBase64):
        return {"type": "base64", "media_type": source.media_type, "data": source.data}
    return {"type": "url", "url": source.url}


@dataclass(frozen=True)
class DocumentSourceBase64:
    media_type: str
    data: str
    type: Literal["base64"] = "base64"


@dataclass(frozen=True)
class DocumentSourceUrl:
    url: str
    type: Literal["url"] = "url"


DocumentSource = DocumentSourceBase64 | DocumentSourceUrl


def document_source_to_dict(source: DocumentSource) -> dict[str, str]:
    if isinstance(source, DocumentSourceBase64):
        return {"type": "base64", "media_type": source.media_type, "data": source.data}
    return {"type": "url", "url": source.url}


@dataclass(frozen=True)
class TextBlock:
    text: str
    type: Literal["text"] = "text"


@dataclass(frozen=True)
class ImageBlock:
    source: ImageSource
    type: Literal["image"] = "image"


@dataclass(frozen=True)
class DocumentBlock:
    source: DocumentSource
    type: Literal["document"] = "document"


ContentBlock = TextBlock | ImageBlock | DocumentBlock


def content_block_to_dict(block: ContentBlock) -> dict[str, Any]:
    if isinstance(block, TextBlock):
        return {"type": "text", "text": block.text}
    if isinstance(block, ImageBlock):
        return {"type": "image", "source": image_source_to_dict(block.source)}
    return {"type": "document", "source": document_source_to_dict(block.source)}


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
    content_blocks: list[ContentBlock] = field(default_factory=list)
    cache: bool = False

    @classmethod
    def user(cls, content: str) -> Message:
        return cls(role=Role.user, content=content)

    @classmethod
    def user_with_cache(cls, content: str) -> Message:
        return cls(role=Role.user, content=content, cache=True)

    @classmethod
    def assistant(cls, content: str) -> Message:
        return cls(role=Role.assistant, content=content)

    @classmethod
    def assistant_with_tool_calls(cls, content: str, tool_calls: list[ToolCall]) -> Message:
        return cls(role=Role.assistant, content=content, tool_calls=tool_calls)

    @classmethod
    def system(cls, content: str) -> Message:
        return cls(role=Role.system, content=content)

    @classmethod
    def tool_result(cls, tool_call_id: str, content: str) -> Message:
        return cls(role=Role.tool, content=content, tool_call_id=tool_call_id)

    @classmethod
    def user_with_image(cls, text: str, base64_data: str, media_type: str) -> Message:
        return cls(
            role=Role.user,
            content=text,
            content_blocks=[
                TextBlock(text=text),
                ImageBlock(source=ImageSourceBase64(media_type=media_type, data=base64_data)),
            ],
        )

    @classmethod
    def user_with_blocks(cls, blocks: list[ContentBlock]) -> Message:
        text = ""
        for block in blocks:
            if isinstance(block, TextBlock):
                text = block.text
                break
        return cls(role=Role.user, content=text, content_blocks=list(blocks))

    @classmethod
    def user_with_pdf_base64(cls, text: str, base64_data: str) -> Message:
        return cls(
            role=Role.user,
            content=text,
            content_blocks=[
                TextBlock(text=text),
                DocumentBlock(
                    source=DocumentSourceBase64(media_type="application/pdf", data=base64_data)
                ),
            ],
        )

    @classmethod
    def user_with_pdf_url(cls, text: str, url: str) -> Message:
        return cls(
            role=Role.user,
            content=text,
            content_blocks=[TextBlock(text=text), DocumentBlock(source=DocumentSourceUrl(url=url))],
        )

    @classmethod
    def user_with_pdf_bytes(cls, text: str, data: bytes) -> Message:
        import base64 as _b64

        encoded = _b64.b64encode(data).decode("ascii")
        return cls.user_with_pdf_base64(text, encoded)

    def with_cache(self) -> Message:
        self.cache = True
        return self


@dataclass
class SystemBlock:
    text: str
    cache_control: bool = False

    @classmethod
    def new(cls, text: str) -> SystemBlock:
        return cls(text=text, cache_control=False)

    @classmethod
    def cached(cls, text: str) -> SystemBlock:
        return cls(text=text, cache_control=True)


def system_block_to_dict(block: SystemBlock) -> dict[str, Any]:
    out: dict[str, Any] = {"type": "text", "text": block.text}
    if block.cache_control:
        out["cache_control"] = {"type": "ephemeral"}
    return out


@dataclass
class Tool:
    name: str
    description: str | None = None
    input_schema: dict[str, Any] | None = None
    cache: bool = False


@dataclass(frozen=True)
class ToolChoice:
    type: Literal["auto", "required", "none", "tool"]
    name: str | None = None

    def __post_init__(self) -> None:
        if self.type == "tool" and not self.name:
            raise ValueError("tool name required when ToolChoice.type == 'tool'")

    @classmethod
    def auto(cls) -> ToolChoice:
        return cls(type="auto")

    @classmethod
    def required(cls) -> ToolChoice:
        return cls(type="required")

    @classmethod
    def none(cls) -> ToolChoice:
        return cls(type="none")

    @classmethod
    def tool(cls, name: str) -> ToolChoice:
        return cls(type="tool", name=name)


def tool_choice_to_dict(choice: ToolChoice) -> dict[str, str]:
    out: dict[str, str] = {"type": choice.type}
    if choice.name is not None:
        out["name"] = choice.name
    return out


@dataclass(frozen=True)
class ThinkingConfig:
    budget_tokens: int


@dataclass(frozen=True)
class McpServerConfig:
    url: str
    name: str
    authorization_token: str | None = None
    type: Literal["url"] = "url"


def mcp_server_config_to_dict(cfg: McpServerConfig) -> dict[str, str]:
    out: dict[str, str] = {"type": cfg.type, "url": cfg.url, "name": cfg.name}
    if cfg.authorization_token is not None:
        out["authorization_token"] = cfg.authorization_token
    return out


@dataclass(frozen=True)
class McpToolConfigAll:
    mcp_server_name: str


@dataclass(frozen=True)
class McpToolConfigAllowed:
    mcp_server_name: str
    allowed_tools: list[str]


@dataclass(frozen=True)
class McpToolConfigDenied:
    mcp_server_name: str
    denied_tools: list[str]


McpToolConfig = McpToolConfigAll | McpToolConfigAllowed | McpToolConfigDenied


def mcp_tool_config_to_dict(cfg: McpToolConfig) -> dict[str, Any]:
    base: dict[str, Any] = {"type": "mcp_toolset", "mcp_server_name": cfg.mcp_server_name}
    if isinstance(cfg, McpToolConfigAllowed):
        base["allowed_tools"] = list(cfg.allowed_tools)
    elif isinstance(cfg, McpToolConfigDenied):
        base["denied_tools"] = list(cfg.denied_tools)
    return base


@dataclass
class Usage:
    input_tokens: int
    output_tokens: int
    cache_creation_input_tokens: int | None = None
    cache_read_input_tokens: int | None = None


@dataclass
class ChatRequest:
    messages: list[Message]
    model: str | None = None
    system: str | None = None
    temperature: float | None = None
    max_tokens: int | None = None
    tools: list[Tool] | None = None
    provider_options: dict[str, Any] | None = None
    system_blocks: list[SystemBlock] | None = None
    system_cache: bool = False
    tool_choice: ToolChoice | None = None
    mcp_servers: list[McpServerConfig] | None = None
    mcp_tool_configs: list[McpToolConfig] | None = None
    thinking: ThinkingConfig | None = None
    stop_sequences: list[str] | None = None

    @classmethod
    def builder(cls) -> ChatRequestBuilder:
        return ChatRequestBuilder()


@dataclass
class ChatResponse:
    content: str
    tool_calls: list[ToolCall] = field(default_factory=list)
    model: str = ""
    usage: Usage = field(default_factory=lambda: Usage(0, 0))
    stop_reason: StopReason = StopReason.end_turn
    thinking: str | None = None


@dataclass
class StreamEvent:
    content: str
    done: bool
    tool_call_id: str | None = None
    tool_call_name: str | None = None
    tool_call_args_delta: str | None = None
    event_type: str = "text"
    usage: Usage | None = None
    stop_reason: StopReason | None = None


class ChatRequestBuilder:
    def __init__(self) -> None:
        self._messages: list[Message] = []
        self._model: str | None = None
        self._system: str | None = None
        self._system_blocks: list[SystemBlock] | None = None
        self._system_cache: bool = False
        self._temperature: float | None = None
        self._max_tokens: int | None = None
        self._tools: list[Tool] | None = None
        self._tool_choice: ToolChoice | None = None
        self._provider_options: dict[str, Any] | None = None
        self._mcp_servers: list[McpServerConfig] | None = None
        self._mcp_tool_configs: list[McpToolConfig] | None = None
        self._thinking: ThinkingConfig | None = None
        self._stop_sequences: list[str] | None = None

    def messages(self, messages: list[Message]) -> ChatRequestBuilder:
        self._messages = list(messages)
        return self

    def message(self, message: Message) -> ChatRequestBuilder:
        self._messages.append(message)
        return self

    def model(self, model: str) -> ChatRequestBuilder:
        self._model = model
        return self

    def system(self, system: str) -> ChatRequestBuilder:
        self._system = system
        return self

    def system_cached(self, system: str) -> ChatRequestBuilder:
        self._system = system
        self._system_cache = True
        return self

    def system_block(self, block: SystemBlock) -> ChatRequestBuilder:
        if self._system_blocks is None:
            self._system_blocks = []
        self._system_blocks.append(block)
        return self

    def system_blocks(self, blocks: list[SystemBlock]) -> ChatRequestBuilder:
        self._system_blocks = list(blocks)
        return self

    def temperature(self, temperature: float) -> ChatRequestBuilder:
        self._temperature = temperature
        return self

    def max_tokens(self, max_tokens: int) -> ChatRequestBuilder:
        self._max_tokens = max_tokens
        return self

    def tools(self, tools: list[Tool]) -> ChatRequestBuilder:
        self._tools = list(tools)
        return self

    def tools_cached(self, tools: list[Tool]) -> ChatRequestBuilder:
        copied = list(tools)
        if copied:
            copied[-1] = replace(copied[-1], cache=True)
        self._tools = copied
        return self

    def tool_choice(self, choice: ToolChoice) -> ChatRequestBuilder:
        self._tool_choice = choice
        return self

    def provider_options(self, options: dict[str, Any]) -> ChatRequestBuilder:
        self._provider_options = dict(options)
        return self

    def mcp_server(self, server: McpServerConfig) -> ChatRequestBuilder:
        if self._mcp_servers is None:
            self._mcp_servers = []
        if self._mcp_tool_configs is None:
            self._mcp_tool_configs = []
        if not any(config.mcp_server_name == server.name for config in self._mcp_tool_configs):
            self._mcp_tool_configs.append(McpToolConfigAll(mcp_server_name=server.name))
        self._mcp_servers.append(server)
        return self

    def mcp_servers(self, servers: list[McpServerConfig]) -> ChatRequestBuilder:
        self._mcp_servers = list(servers)
        self._mcp_tool_configs = [McpToolConfigAll(mcp_server_name=s.name) for s in servers]
        return self

    def mcp_tool_config(self, config: McpToolConfig) -> ChatRequestBuilder:
        if self._mcp_tool_configs is None:
            self._mcp_tool_configs = []
        name = config.mcp_server_name
        for i, existing in enumerate(self._mcp_tool_configs):
            if existing.mcp_server_name == name:
                self._mcp_tool_configs[i] = config
                return self
        self._mcp_tool_configs.append(config)
        return self

    def mcp_tool_configs(self, configs: list[McpToolConfig]) -> ChatRequestBuilder:
        self._mcp_tool_configs = list(configs)
        return self

    def thinking(self, budget_tokens: int) -> ChatRequestBuilder:
        self._thinking = ThinkingConfig(budget_tokens=budget_tokens)
        return self

    def stop(self, sequence: str) -> ChatRequestBuilder:
        if self._stop_sequences is None:
            self._stop_sequences = []
        self._stop_sequences.append(sequence)
        return self

    def stop_sequences(self, sequences: list[str]) -> ChatRequestBuilder:
        self._stop_sequences = list(sequences)
        return self

    def build(self) -> ChatRequest:
        return ChatRequest(
            messages=self._messages,
            model=self._model,
            system=self._system,
            system_blocks=self._system_blocks,
            system_cache=self._system_cache,
            temperature=self._temperature,
            max_tokens=self._max_tokens,
            tools=self._tools,
            tool_choice=self._tool_choice,
            provider_options=self._provider_options,
            mcp_servers=self._mcp_servers,
            mcp_tool_configs=self._mcp_tool_configs,
            thinking=self._thinking,
            stop_sequences=self._stop_sequences,
        )
