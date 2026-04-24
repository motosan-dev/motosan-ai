# Python SDK Phase 1 — Type Foundation Implementation Plan

> **Status:** ✅ **COMPLETE (2026-04-24)** — shipped as `motosan-ai` v0.6.0.
>
> | Metric | Result |
> |--------|--------|
> | Tests | 168 passed, 7 skipped (live-only) |
> | Lint (`ruff check`) | clean |
> | Format (`ruff format --check`) | clean |
> | New files | `provider_base.py` + 6 test files (451 lines of tests) |
> | Modified | `types.py` (83 → 496 lines), `__init__.py`, 5 providers, `CHANGELOG.md`, `pyproject.toml` |
> | Checkboxes below | Retained for historical TDD trace — all steps executed. |
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port all Rust v0.14.0 SDK types to Python v0.6.0, additively, without changing any existing wire-format behavior.

**Architecture:** Python discriminated unions via `@dataclass` + `Literal` tags (3.11+). A new `provider_base` module introduces `ProviderCapabilities` and a `BaseProvider` ABC carrying default `validate_request()` behavior. Each existing provider declares its capabilities; no serialization changes yet — those land in Phase 2.

**Tech Stack:** Python 3.11+, `dataclasses`, `typing.Literal`, `enum.StrEnum`, `pytest`, `pytest-asyncio`, `respx`.

---

## File Structure

| Path | Responsibility | Status |
|------|----------------|--------|
| `sdks/python/motosan_ai/types.py` | All data types (messages, content blocks, tool choice, request/response, stream events, MCP config) | **Modify** (grows ~600 lines) |
| `sdks/python/motosan_ai/provider_base.py` | `ProviderCapabilities`, `ProviderImpl` Protocol, `BaseProvider` ABC with default `validate_request()` | **Create** |
| `sdks/python/motosan_ai/providers/anthropic.py` | Inherit `BaseProvider`, declare `capabilities=full()` | **Modify** (minor) |
| `sdks/python/motosan_ai/providers/openai.py` | Inherit `BaseProvider`, declare `capabilities=with_image()` | **Modify** |
| `sdks/python/motosan_ai/providers/minimax.py` | Inherit `BaseProvider`, declare `capabilities=with_image()` | **Modify** |
| `sdks/python/motosan_ai/providers/ollama.py` | Inherit `BaseProvider`, declare `capabilities=text_only()` | **Modify** |
| `sdks/python/motosan_ai/providers/claude_code.py` | Inherit `BaseProvider`, declare `capabilities=text_only()` | **Modify** |
| `sdks/python/motosan_ai/__init__.py` | Export new public types | **Modify** |
| `sdks/python/tests/test_types_content_blocks.py` | `ContentBlock` / `ImageSource` / `DocumentSource` serialization + constructors | **Create** |
| `sdks/python/tests/test_types_system_blocks.py` | `SystemBlock` + `Message.cache` + `Tool.cache` | **Create** |
| `sdks/python/tests/test_types_tool_choice.py` | `ToolChoice` factories + serialization | **Create** |
| `sdks/python/tests/test_types_builder.py` | `ChatRequest.builder()` fluent API | **Create** |
| `sdks/python/tests/test_types_mcp.py` | `McpServerConfig` / `McpToolConfig` | **Create** |
| `sdks/python/tests/test_provider_capabilities.py` | Capability-based validation | **Create** |
| `sdks/python/tests/test_types.py` | Existing type smoke tests — extend slightly | **Modify** |
| `sdks/python/CHANGELOG.md` | v0.6.0 entry | **Modify** |
| `sdks/python/pyproject.toml` | Version bump 0.5.0 → 0.6.0 | **Modify** |

Design principles:
- **Additive only.** Every new dataclass field defaults to `None`, `False`, or an empty collection so existing callers keep compiling.
- **Wire format untouched.** No provider `_build_body` change in Phase 1 — Phase 2 flips serialization.
- **SSOT alignment.** Every new shape is cross-checked against `specs/types.md` and the Rust `types.rs`.

---

## Task 1: `ImageSource` discriminated union

**Files:**
- Modify: `sdks/python/motosan_ai/types.py`
- Create: `sdks/python/tests/test_types_content_blocks.py`

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_types_content_blocks.py`:

```python
from __future__ import annotations

import base64

from motosan_ai.types import (
    ImageSourceBase64,
    ImageSourceUrl,
    image_source_to_dict,
)


def test_image_source_base64_to_dict():
    src = ImageSourceBase64(media_type="image/png", data="JVBER")
    assert image_source_to_dict(src) == {
        "type": "base64",
        "media_type": "image/png",
        "data": "JVBER",
    }


def test_image_source_url_to_dict():
    src = ImageSourceUrl(url="https://example.com/pic.png")
    assert image_source_to_dict(src) == {
        "type": "url",
        "url": "https://example.com/pic.png",
    }
```

- [ ] **Step 2: Run tests and verify they fail**

Run: `cd sdks/python && uv run pytest tests/test_types_content_blocks.py -v`
Expected: FAIL — `ImportError: cannot import name 'ImageSourceBase64'`.

- [ ] **Step 3: Implement `ImageSource` types**

Append to `sdks/python/motosan_ai/types.py`:

```python
from typing import Literal


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
```

- [ ] **Step 4: Run tests and verify they pass**

Run: `cd sdks/python && uv run pytest tests/test_types_content_blocks.py -v`
Expected: PASS — 2 tests.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/types.py sdks/python/tests/test_types_content_blocks.py
git commit -m "feat(python): add ImageSource discriminated union types"
```

---

## Task 2: `DocumentSource` discriminated union

**Files:**
- Modify: `sdks/python/motosan_ai/types.py`
- Modify: `sdks/python/tests/test_types_content_blocks.py`

- [ ] **Step 1: Append failing tests**

Append to `sdks/python/tests/test_types_content_blocks.py`:

```python
from motosan_ai.types import (
    DocumentSourceBase64,
    DocumentSourceUrl,
    document_source_to_dict,
)


def test_document_source_base64_to_dict():
    src = DocumentSourceBase64(media_type="application/pdf", data="JVBERi0xLjQK")
    assert document_source_to_dict(src) == {
        "type": "base64",
        "media_type": "application/pdf",
        "data": "JVBERi0xLjQK",
    }


def test_document_source_url_to_dict():
    src = DocumentSourceUrl(url="https://example.com/doc.pdf")
    assert document_source_to_dict(src) == {
        "type": "url",
        "url": "https://example.com/doc.pdf",
    }
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_types_content_blocks.py::test_document_source_base64_to_dict -v`
Expected: FAIL — `ImportError`.

- [ ] **Step 3: Implement `DocumentSource`**

Append to `sdks/python/motosan_ai/types.py`:

```python
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
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_types_content_blocks.py -v`
Expected: PASS — 4 tests total.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/types.py sdks/python/tests/test_types_content_blocks.py
git commit -m "feat(python): add DocumentSource discriminated union types"
```

---

## Task 3: `ContentBlock` discriminated union

**Files:**
- Modify: `sdks/python/motosan_ai/types.py`
- Modify: `sdks/python/tests/test_types_content_blocks.py`

- [ ] **Step 1: Append failing tests**

```python
from motosan_ai.types import (
    DocumentBlock,
    ImageBlock,
    TextBlock,
    content_block_to_dict,
)


def test_text_block_to_dict():
    block = TextBlock(text="hello")
    assert content_block_to_dict(block) == {"type": "text", "text": "hello"}


def test_image_block_to_dict_base64():
    block = ImageBlock(source=ImageSourceBase64(media_type="image/png", data="abc"))
    assert content_block_to_dict(block) == {
        "type": "image",
        "source": {"type": "base64", "media_type": "image/png", "data": "abc"},
    }


def test_document_block_to_dict_url():
    block = DocumentBlock(source=DocumentSourceUrl(url="https://x.com/d.pdf"))
    assert content_block_to_dict(block) == {
        "type": "document",
        "source": {"type": "url", "url": "https://x.com/d.pdf"},
    }
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_types_content_blocks.py -v`
Expected: FAIL — `ImportError: cannot import name 'TextBlock'`.

- [ ] **Step 3: Implement `ContentBlock`**

Append to `sdks/python/motosan_ai/types.py`:

```python
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
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_types_content_blocks.py -v`
Expected: PASS — 7 tests.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/types.py sdks/python/tests/test_types_content_blocks.py
git commit -m "feat(python): add ContentBlock discriminated union"
```

---

## Task 4: Extend `Message` with `content_blocks` + multimodal factories

**Files:**
- Modify: `sdks/python/motosan_ai/types.py:30-55` (Message class)
- Modify: `sdks/python/tests/test_types_content_blocks.py`

- [ ] **Step 1: Append failing tests**

```python
import base64

from motosan_ai.types import Message, Role


def test_user_with_image_sets_content_blocks():
    msg = Message.user_with_image("look at this", "JVBER", "image/png")
    assert msg.role == Role.user
    assert msg.content == "look at this"
    assert len(msg.content_blocks) == 2
    assert isinstance(msg.content_blocks[0], TextBlock)
    assert msg.content_blocks[0].text == "look at this"
    assert isinstance(msg.content_blocks[1], ImageBlock)
    assert isinstance(msg.content_blocks[1].source, ImageSourceBase64)
    assert msg.content_blocks[1].source.media_type == "image/png"


def test_user_with_pdf_base64():
    msg = Message.user_with_pdf_base64("summarize", "JVBERi0xLjQK")
    assert len(msg.content_blocks) == 2
    doc = msg.content_blocks[1]
    assert isinstance(doc, DocumentBlock)
    assert isinstance(doc.source, DocumentSourceBase64)
    assert doc.source.media_type == "application/pdf"
    assert doc.source.data == "JVBERi0xLjQK"


def test_user_with_pdf_url():
    msg = Message.user_with_pdf_url("analyze", "https://example.com/d.pdf")
    doc = msg.content_blocks[1]
    assert isinstance(doc, DocumentBlock)
    assert isinstance(doc.source, DocumentSourceUrl)
    assert doc.source.url == "https://example.com/d.pdf"


def test_user_with_pdf_bytes_auto_encodes():
    raw = b"%PDF-1.4\n"
    msg = Message.user_with_pdf_bytes("read", raw)
    doc = msg.content_blocks[1]
    assert isinstance(doc, DocumentBlock)
    assert isinstance(doc.source, DocumentSourceBase64)
    decoded = base64.b64decode(doc.source.data)
    assert decoded == raw


def test_user_with_blocks_extracts_text():
    blocks = [
        TextBlock(text="describe"),
        ImageBlock(source=ImageSourceUrl(url="https://x.com/i.png")),
    ]
    msg = Message.user_with_blocks(blocks)
    assert msg.content == "describe"
    assert msg.content_blocks == blocks


def test_message_default_content_blocks_is_empty_list():
    msg = Message.user("hello")
    assert msg.content_blocks == []
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_types_content_blocks.py -v`
Expected: FAIL — `AttributeError: type object 'Message' has no attribute 'user_with_image'`.

- [ ] **Step 3: Extend `Message`**

Edit `sdks/python/motosan_ai/types.py`, replace the `Message` dataclass (current lines 30-55) with:

```python
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
        for b in blocks:
            if isinstance(b, TextBlock):
                text = b.text
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
            content_blocks=[
                TextBlock(text=text),
                DocumentBlock(source=DocumentSourceUrl(url=url)),
            ],
        )

    @classmethod
    def user_with_pdf_bytes(cls, text: str, data: bytes) -> Message:
        import base64 as _b64

        encoded = _b64.b64encode(data).decode("ascii")
        return cls.user_with_pdf_base64(text, encoded)

    def with_cache(self) -> Message:
        self.cache = True
        return self
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_types_content_blocks.py tests/test_types.py tests/test_anthropic.py -v`
Expected: PASS — new tests green, **existing tests still green** (this is the main regression gate).

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/types.py sdks/python/tests/test_types_content_blocks.py
git commit -m "feat(python): extend Message with content_blocks and multimodal factories"
```

---

## Task 5: `SystemBlock` type

**Files:**
- Modify: `sdks/python/motosan_ai/types.py`
- Create: `sdks/python/tests/test_types_system_blocks.py`

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_types_system_blocks.py`:

```python
from motosan_ai.types import SystemBlock, system_block_to_dict


def test_system_block_new_defaults_cache_false():
    block = SystemBlock.new("Hello")
    assert block.text == "Hello"
    assert block.cache_control is False


def test_system_block_cached_sets_cache_true():
    block = SystemBlock.cached("Cached prompt")
    assert block.text == "Cached prompt"
    assert block.cache_control is True


def test_system_block_to_dict_plain_omits_cache_control():
    block = SystemBlock.new("plain")
    assert system_block_to_dict(block) == {"type": "text", "text": "plain"}


def test_system_block_to_dict_cached_includes_ephemeral():
    block = SystemBlock.cached("cached")
    assert system_block_to_dict(block) == {
        "type": "text",
        "text": "cached",
        "cache_control": {"type": "ephemeral"},
    }
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_types_system_blocks.py -v`
Expected: FAIL — `ImportError`.

- [ ] **Step 3: Implement `SystemBlock`**

Append to `sdks/python/motosan_ai/types.py`:

```python
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
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_types_system_blocks.py -v`
Expected: PASS — 4 tests.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/types.py sdks/python/tests/test_types_system_blocks.py
git commit -m "feat(python): add SystemBlock for Anthropic prompt caching"
```

---

## Task 6: `ToolChoice` factory dataclass

**Files:**
- Modify: `sdks/python/motosan_ai/types.py`
- Create: `sdks/python/tests/test_types_tool_choice.py`

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_types_tool_choice.py`:

```python
import pytest

from motosan_ai.types import ToolChoice, tool_choice_to_dict


def test_tool_choice_auto():
    tc = ToolChoice.auto()
    assert tc.type == "auto"
    assert tc.name is None
    assert tool_choice_to_dict(tc) == {"type": "auto"}


def test_tool_choice_required():
    tc = ToolChoice.required()
    assert tc.type == "required"
    assert tool_choice_to_dict(tc) == {"type": "required"}


def test_tool_choice_none():
    tc = ToolChoice.none()
    assert tc.type == "none"
    assert tool_choice_to_dict(tc) == {"type": "none"}


def test_tool_choice_tool():
    tc = ToolChoice.tool("get_weather")
    assert tc.type == "tool"
    assert tc.name == "get_weather"
    assert tool_choice_to_dict(tc) == {"type": "tool", "name": "get_weather"}


def test_tool_choice_tool_requires_name():
    with pytest.raises(ValueError, match="tool name required"):
        ToolChoice(type="tool", name=None)
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_types_tool_choice.py -v`
Expected: FAIL — `ImportError`.

- [ ] **Step 3: Implement `ToolChoice`**

Append to `sdks/python/motosan_ai/types.py`:

```python
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
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_types_tool_choice.py -v`
Expected: PASS — 5 tests.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/types.py sdks/python/tests/test_types_tool_choice.py
git commit -m "feat(python): add ToolChoice factory dataclass"
```

---

## Task 7: `Tool.cache` field + `ThinkingConfig`

**Files:**
- Modify: `sdks/python/motosan_ai/types.py:58-62` (Tool dataclass)
- Modify: `sdks/python/tests/test_types.py`

- [ ] **Step 1: Append failing tests**

Append to `sdks/python/tests/test_types.py`:

```python
from motosan_ai.types import ThinkingConfig, Tool


def test_tool_cache_defaults_false():
    t = Tool(name="x")
    assert t.cache is False


def test_tool_cache_explicit_true():
    t = Tool(name="x", cache=True)
    assert t.cache is True


def test_thinking_config_budget():
    cfg = ThinkingConfig(budget_tokens=4096)
    assert cfg.budget_tokens == 4096
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_types.py::test_tool_cache_defaults_false tests/test_types.py::test_thinking_config_budget -v`
Expected: FAIL — `TypeError: Tool.__init__() got an unexpected keyword argument 'cache'` / `ImportError`.

- [ ] **Step 3: Extend `Tool` and add `ThinkingConfig`**

Modify the `Tool` dataclass and append `ThinkingConfig` to `sdks/python/motosan_ai/types.py`:

```python
@dataclass
class Tool:
    name: str
    description: str | None = None
    input_schema: dict[str, Any] | None = None
    cache: bool = False


@dataclass(frozen=True)
class ThinkingConfig:
    budget_tokens: int
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS — all tests green, no regressions.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/types.py sdks/python/tests/test_types.py
git commit -m "feat(python): add Tool.cache and ThinkingConfig"
```

---

## Task 8: `McpServerConfig` + `McpToolConfig`

**Files:**
- Modify: `sdks/python/motosan_ai/types.py`
- Create: `sdks/python/tests/test_types_mcp.py`

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_types_mcp.py`:

```python
from motosan_ai.types import (
    McpServerConfig,
    McpToolConfigAll,
    McpToolConfigAllowed,
    McpToolConfigDenied,
    mcp_server_config_to_dict,
    mcp_tool_config_to_dict,
)


def test_mcp_server_config_minimal():
    cfg = McpServerConfig(url="https://mcp.example.com", name="example")
    assert mcp_server_config_to_dict(cfg) == {
        "type": "url",
        "url": "https://mcp.example.com",
        "name": "example",
    }


def test_mcp_server_config_with_auth():
    cfg = McpServerConfig(
        url="https://mcp.example.com",
        name="example",
        authorization_token="secret",
    )
    assert mcp_server_config_to_dict(cfg) == {
        "type": "url",
        "url": "https://mcp.example.com",
        "name": "example",
        "authorization_token": "secret",
    }


def test_mcp_tool_config_all():
    cfg = McpToolConfigAll(mcp_server_name="example")
    assert mcp_tool_config_to_dict(cfg) == {
        "type": "mcp_toolset",
        "mcp_server_name": "example",
    }


def test_mcp_tool_config_allowed():
    cfg = McpToolConfigAllowed(mcp_server_name="example", allowed_tools=["read", "write"])
    assert mcp_tool_config_to_dict(cfg) == {
        "type": "mcp_toolset",
        "mcp_server_name": "example",
        "allowed_tools": ["read", "write"],
    }


def test_mcp_tool_config_denied():
    cfg = McpToolConfigDenied(mcp_server_name="example", denied_tools=["delete"])
    assert mcp_tool_config_to_dict(cfg) == {
        "type": "mcp_toolset",
        "mcp_server_name": "example",
        "denied_tools": ["delete"],
    }
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_types_mcp.py -v`
Expected: FAIL — `ImportError`.

- [ ] **Step 3: Implement MCP types**

Append to `sdks/python/motosan_ai/types.py`:

```python
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
    base = {"type": "mcp_toolset", "mcp_server_name": cfg.mcp_server_name}
    if isinstance(cfg, McpToolConfigAllowed):
        base["allowed_tools"] = list(cfg.allowed_tools)
    elif isinstance(cfg, McpToolConfigDenied):
        base["denied_tools"] = list(cfg.denied_tools)
    return base
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_types_mcp.py -v`
Expected: PASS — 5 tests.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/types.py sdks/python/tests/test_types_mcp.py
git commit -m "feat(python): add McpServerConfig and McpToolConfig types"
```

---

## Task 9: Extend `Usage` with cache token fields

**Files:**
- Modify: `sdks/python/motosan_ai/types.py:65-68` (Usage dataclass)
- Modify: `sdks/python/tests/test_types.py`

- [ ] **Step 1: Append failing test**

```python
from motosan_ai.types import Usage


def test_usage_cache_fields_default_none():
    u = Usage(input_tokens=10, output_tokens=5)
    assert u.cache_creation_input_tokens is None
    assert u.cache_read_input_tokens is None


def test_usage_cache_fields_explicit():
    u = Usage(
        input_tokens=10,
        output_tokens=5,
        cache_creation_input_tokens=100,
        cache_read_input_tokens=50,
    )
    assert u.cache_creation_input_tokens == 100
    assert u.cache_read_input_tokens == 50
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_types.py::test_usage_cache_fields_default_none -v`
Expected: FAIL — `TypeError: Usage.__init__() got an unexpected keyword argument 'cache_creation_input_tokens'`.

- [ ] **Step 3: Extend `Usage`**

Modify the `Usage` dataclass in `sdks/python/motosan_ai/types.py`:

```python
@dataclass
class Usage:
    input_tokens: int
    output_tokens: int
    cache_creation_input_tokens: int | None = None
    cache_read_input_tokens: int | None = None
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS — no regressions.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/types.py sdks/python/tests/test_types.py
git commit -m "feat(python): add cache token fields to Usage"
```

---

## Task 10: Add `StopReason.stop_sequence` + `StreamEventType` enum

**Files:**
- Modify: `sdks/python/motosan_ai/types.py:15-20` (StopReason enum) + StreamEvent
- Modify: `sdks/python/tests/test_types.py`

- [ ] **Step 1: Append failing tests**

```python
from motosan_ai.types import StopReason, StreamEvent, StreamEventType


def test_stop_reason_stop_sequence_exists():
    assert StopReason.stop_sequence == "stop_sequence"


def test_stream_event_type_values():
    assert StreamEventType.text == "text"
    assert StreamEventType.tool_call_start == "tool_call_start"
    assert StreamEventType.tool_call_args == "tool_call_args"
    assert StreamEventType.tool_call_end == "tool_call_end"
    assert StreamEventType.usage == "usage"


def test_stream_event_new_optional_fields_default_none():
    ev = StreamEvent(content="hi", done=False)
    assert ev.stop_reason is None
    assert ev.usage is None
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_types.py::test_stop_reason_stop_sequence_exists -v`
Expected: FAIL — `AttributeError: stop_sequence`.

- [ ] **Step 3: Extend `StopReason`, add `StreamEventType`, extend `StreamEvent`**

In `sdks/python/motosan_ai/types.py`:

Change `StopReason`:

```python
class StopReason(StrEnum):
    end_turn = "end_turn"
    max_tokens = "max_tokens"
    tool_use = "tool_use"
    stop = "stop"
    stop_sequence = "stop_sequence"
    other = "other"
```

Add `StreamEventType`:

```python
class StreamEventType(StrEnum):
    text = "text"
    tool_call_start = "tool_call_start"
    tool_call_args = "tool_call_args"
    tool_call_end = "tool_call_end"
    usage = "usage"
```

Extend `StreamEvent` (keep `event_type: str` default for backward compat with existing providers that pass string literals):

```python
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
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS — all existing provider tests (which pass `event_type` as string literal) still green.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/types.py sdks/python/tests/test_types.py
git commit -m "feat(python): add StopReason.stop_sequence, StreamEventType, and StreamEvent.usage/stop_reason"
```

---

## Task 11: Extend `ChatResponse` with `thinking`

**Files:**
- Modify: `sdks/python/motosan_ai/types.py:75-82` (ChatResponse dataclass)
- Modify: `sdks/python/tests/test_types.py`

- [ ] **Step 1: Append failing test**

```python
from motosan_ai.types import ChatResponse


def test_chat_response_thinking_defaults_none():
    r = ChatResponse(content="hi")
    assert r.thinking is None


def test_chat_response_thinking_explicit():
    r = ChatResponse(content="hi", thinking="reasoning trace")
    assert r.thinking == "reasoning trace"
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_types.py::test_chat_response_thinking_defaults_none -v`
Expected: FAIL — `TypeError`.

- [ ] **Step 3: Extend `ChatResponse`**

Modify in `sdks/python/motosan_ai/types.py`:

```python
@dataclass
class ChatResponse:
    content: str
    thinking: str | None = None
    tool_calls: list[ToolCall] = field(default_factory=list)
    model: str = ""
    usage: Usage = field(default_factory=lambda: Usage(0, 0))
    stop_reason: StopReason = StopReason.end_turn
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/types.py sdks/python/tests/test_types.py
git commit -m "feat(python): add ChatResponse.thinking"
```

---

## Task 12: Extend `ChatRequest` with new optional fields

**Files:**
- Modify: `sdks/python/motosan_ai/types.py:71-80` (ChatRequest dataclass)
- Modify: `sdks/python/tests/test_types.py`

- [ ] **Step 1: Append failing tests**

```python
from motosan_ai.types import (
    ChatRequest,
    McpServerConfig,
    McpToolConfigAll,
    Message,
    SystemBlock,
    ThinkingConfig,
    ToolChoice,
)


def test_chat_request_new_fields_default_none():
    req = ChatRequest(messages=[Message.user("hi")])
    assert req.system_blocks is None
    assert req.system_cache is False
    assert req.tool_choice is None
    assert req.mcp_servers is None
    assert req.mcp_tool_configs is None
    assert req.thinking is None
    assert req.stop_sequences is None


def test_chat_request_all_new_fields_settable():
    req = ChatRequest(
        messages=[Message.user("hi")],
        system_blocks=[SystemBlock.new("a")],
        system_cache=True,
        tool_choice=ToolChoice.required(),
        mcp_servers=[McpServerConfig(url="https://x", name="x")],
        mcp_tool_configs=[McpToolConfigAll(mcp_server_name="x")],
        thinking=ThinkingConfig(budget_tokens=1024),
        stop_sequences=["STOP"],
    )
    assert req.system_cache is True
    assert req.thinking.budget_tokens == 1024
    assert req.stop_sequences == ["STOP"]
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_types.py::test_chat_request_new_fields_default_none -v`
Expected: FAIL — `AttributeError: 'ChatRequest' object has no attribute 'system_blocks'`.

- [ ] **Step 3: Extend `ChatRequest`**

Replace the `ChatRequest` dataclass in `sdks/python/motosan_ai/types.py`:

```python
@dataclass
class ChatRequest:
    messages: list[Message]
    model: str | None = None
    system: str | None = None
    system_blocks: list[SystemBlock] | None = None
    system_cache: bool = False
    temperature: float | None = None
    max_tokens: int | None = None
    tools: list[Tool] | None = None
    tool_choice: ToolChoice | None = None
    provider_options: dict[str, Any] | None = None
    mcp_servers: list[McpServerConfig] | None = None
    mcp_tool_configs: list[McpToolConfig] | None = None
    thinking: ThinkingConfig | None = None
    stop_sequences: list[str] | None = None
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS — existing provider tests must still pass (they construct `ChatRequest(messages=...)` positionally-compatible or keyword-compatible).

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/types.py sdks/python/tests/test_types.py
git commit -m "feat(python): extend ChatRequest with system_blocks, tool_choice, mcp, thinking, stop_sequences"
```

---

## Task 13: `ChatRequestBuilder` fluent API

**Files:**
- Modify: `sdks/python/motosan_ai/types.py`
- Create: `sdks/python/tests/test_types_builder.py`

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_types_builder.py`:

```python
from motosan_ai.types import (
    ChatRequest,
    McpServerConfig,
    McpToolConfigAll,
    McpToolConfigAllowed,
    Message,
    SystemBlock,
    Tool,
    ToolChoice,
)


def test_builder_minimal():
    req = ChatRequest.builder().message(Message.user("hi")).build()
    assert len(req.messages) == 1
    assert req.messages[0].content == "hi"


def test_builder_system_cached():
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .system_cached("You are a helper.")
        .build()
    )
    assert req.system == "You are a helper."
    assert req.system_cache is True


def test_builder_system_block_appends():
    req = (
        ChatRequest.builder()
        .system_block(SystemBlock.cached("A"))
        .system_block(SystemBlock.new("B"))
        .message(Message.user("hi"))
        .build()
    )
    assert len(req.system_blocks) == 2
    assert req.system_blocks[0].cache_control is True
    assert req.system_blocks[1].cache_control is False


def test_builder_tools_cached_marks_last():
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .tools_cached([Tool(name="a"), Tool(name="b")])
        .build()
    )
    assert req.tools[0].cache is False
    assert req.tools[1].cache is True


def test_builder_tool_choice():
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .tool_choice(ToolChoice.tool("get_weather"))
        .build()
    )
    assert req.tool_choice.type == "tool"
    assert req.tool_choice.name == "get_weather"


def test_builder_mcp_server_auto_adds_all_config():
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .mcp_server(McpServerConfig(url="https://x", name="srv"))
        .build()
    )
    assert len(req.mcp_servers) == 1
    assert len(req.mcp_tool_configs) == 1
    cfg = req.mcp_tool_configs[0]
    assert isinstance(cfg, McpToolConfigAll)
    assert cfg.mcp_server_name == "srv"


def test_builder_mcp_tool_config_replaces_same_server():
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .mcp_server(McpServerConfig(url="https://x", name="srv"))
        .mcp_tool_config(McpToolConfigAllowed(mcp_server_name="srv", allowed_tools=["r"]))
        .build()
    )
    assert len(req.mcp_tool_configs) == 1
    cfg = req.mcp_tool_configs[0]
    assert isinstance(cfg, McpToolConfigAllowed)
    assert cfg.allowed_tools == ["r"]


def test_builder_thinking_and_stop():
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .thinking(2048)
        .stop("END")
        .stop("STOP")
        .build()
    )
    assert req.thinking.budget_tokens == 2048
    assert req.stop_sequences == ["END", "STOP"]
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_types_builder.py -v`
Expected: FAIL — `AttributeError: type object 'ChatRequest' has no attribute 'builder'`.

- [ ] **Step 3: Implement `ChatRequestBuilder`**

Append to `sdks/python/motosan_ai/types.py`:

```python
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
        tools = list(tools)
        if tools:
            tools[-1] = replace(tools[-1], cache=True)
        self._tools = tools
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
        self._mcp_tool_configs.append(McpToolConfigAll(mcp_server_name=server.name))
        self._mcp_servers.append(server)
        return self

    def mcp_servers(self, servers: list[McpServerConfig]) -> ChatRequestBuilder:
        self._mcp_servers = list(servers)
        self._mcp_tool_configs = [
            McpToolConfigAll(mcp_server_name=s.name) for s in servers
        ]
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
```

Add at the top of `types.py` next to the existing `from dataclasses import dataclass, field` line:

```python
from dataclasses import dataclass, field, replace
```

Add to the `ChatRequest` class:

```python
    @classmethod
    def builder(cls) -> ChatRequestBuilder:
        return ChatRequestBuilder()
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_types_builder.py tests/ -v`
Expected: PASS — 8 builder tests + all existing.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/types.py sdks/python/tests/test_types_builder.py
git commit -m "feat(python): add ChatRequest.builder() fluent API"
```

---

## Task 14: `ProviderCapabilities` + `BaseProvider` ABC

**Files:**
- Create: `sdks/python/motosan_ai/provider_base.py`
- Create: `sdks/python/tests/test_provider_capabilities.py`

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_provider_capabilities.py`:

```python
from __future__ import annotations

from collections.abc import AsyncIterator

import pytest

from motosan_ai.error import MotosanError
from motosan_ai.provider_base import BaseProvider, ProviderCapabilities
from motosan_ai.types import ChatRequest, ChatResponse, Message, StreamEvent


class _TextOnlyProvider(BaseProvider):
    capabilities = ProviderCapabilities.text_only()

    async def chat(self, req: ChatRequest) -> ChatResponse:  # pragma: no cover - unused
        raise NotImplementedError

    async def stream(self, req: ChatRequest) -> AsyncIterator[StreamEvent]:  # pragma: no cover
        if False:
            yield StreamEvent(content="", done=True)
        raise NotImplementedError


class _FullProvider(BaseProvider):
    capabilities = ProviderCapabilities.full()

    async def chat(self, req: ChatRequest) -> ChatResponse:  # pragma: no cover
        raise NotImplementedError

    async def stream(self, req: ChatRequest) -> AsyncIterator[StreamEvent]:  # pragma: no cover
        if False:
            yield StreamEvent(content="", done=True)
        raise NotImplementedError


def test_text_only_capabilities():
    caps = ProviderCapabilities.text_only()
    assert caps.supports_image is False
    assert caps.supports_document is False


def test_with_image_capabilities():
    caps = ProviderCapabilities.with_image()
    assert caps.supports_image is True
    assert caps.supports_document is False


def test_full_capabilities():
    caps = ProviderCapabilities.full()
    assert caps.supports_image is True
    assert caps.supports_document is True


def test_text_only_rejects_image():
    p = _TextOnlyProvider()
    req = ChatRequest(messages=[Message.user_with_image("x", "abc", "image/png")])
    with pytest.raises(MotosanError, match="image"):
        p.validate_request(req)


def test_text_only_rejects_document():
    p = _TextOnlyProvider()
    req = ChatRequest(messages=[Message.user_with_pdf_base64("x", "abc")])
    with pytest.raises(MotosanError, match="document"):
        p.validate_request(req)


def test_full_accepts_image_and_document():
    p = _FullProvider()
    img = ChatRequest(messages=[Message.user_with_image("x", "abc", "image/png")])
    doc = ChatRequest(messages=[Message.user_with_pdf_base64("x", "abc")])
    p.validate_request(img)
    p.validate_request(doc)


def test_plain_text_accepted_by_text_only():
    p = _TextOnlyProvider()
    p.validate_request(ChatRequest(messages=[Message.user("plain")]))
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_provider_capabilities.py -v`
Expected: FAIL — `ModuleNotFoundError: motosan_ai.provider_base`.

- [ ] **Step 3: Implement `provider_base.py`**

Create `sdks/python/motosan_ai/provider_base.py`:

```python
from __future__ import annotations

from abc import ABC, abstractmethod
from collections.abc import AsyncIterator
from dataclasses import dataclass

from motosan_ai.error import InvalidRequestError
from motosan_ai.types import (
    ChatRequest,
    ChatResponse,
    DocumentBlock,
    ImageBlock,
    StreamEvent,
)


@dataclass(frozen=True)
class ProviderCapabilities:
    supports_image: bool
    supports_document: bool

    @classmethod
    def text_only(cls) -> ProviderCapabilities:
        return cls(supports_image=False, supports_document=False)

    @classmethod
    def with_image(cls) -> ProviderCapabilities:
        return cls(supports_image=True, supports_document=False)

    @classmethod
    def full(cls) -> ProviderCapabilities:
        return cls(supports_image=True, supports_document=True)


class BaseProvider(ABC):
    capabilities: ProviderCapabilities = ProviderCapabilities.text_only()

    def validate_request(self, request: ChatRequest) -> None:
        caps = self.capabilities
        for msg in request.messages:
            for block in msg.content_blocks:
                if isinstance(block, ImageBlock) and not caps.supports_image:
                    raise InvalidRequestError(
                        "provider does not support image input"
                    )
                if isinstance(block, DocumentBlock) and not caps.supports_document:
                    raise InvalidRequestError(
                        "provider does not support document input"
                    )

    @abstractmethod
    async def chat(self, request: ChatRequest) -> ChatResponse: ...

    @abstractmethod
    def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]: ...
```

Note: we map Rust `UnsupportedFeature` onto existing `InvalidRequestError` to avoid adding a new error class in Phase 1. Phase 2 can promote to a dedicated `UnsupportedFeatureError` if we decide it matters for API consumers.

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_provider_capabilities.py -v`
Expected: PASS — 7 tests.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/provider_base.py sdks/python/tests/test_provider_capabilities.py
git commit -m "feat(python): add ProviderCapabilities and BaseProvider ABC"
```

---

## Task 15: Wire capabilities into existing providers

**Files:**
- Modify: `sdks/python/motosan_ai/providers/anthropic.py:26`
- Modify: `sdks/python/motosan_ai/providers/openai.py`
- Modify: `sdks/python/motosan_ai/providers/minimax.py`
- Modify: `sdks/python/motosan_ai/providers/ollama.py`
- Modify: `sdks/python/motosan_ai/providers/claude_code.py`

Each provider gains a `capabilities` class attribute. **No inheritance change yet** — adding it as a Protocol-style attribute avoids disrupting the existing class hierarchies and keeps `_http` / init logic untouched. `BaseProvider` can be adopted incrementally in Phase 2.

- [ ] **Step 1: Write regression test covering each provider**

Append to `sdks/python/tests/test_provider_capabilities.py`:

```python
from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.providers.claude_code import ClaudeCodeClient
from motosan_ai.providers.minimax import MinimaxProvider
from motosan_ai.providers.ollama import OllamaProvider
from motosan_ai.providers.openai import OpenAIProvider


def test_anthropic_is_full_capability():
    p = AnthropicProvider(api_key="test")
    assert p.capabilities == ProviderCapabilities.full()


def test_openai_is_with_image():
    p = OpenAIProvider(api_key="test")
    assert p.capabilities == ProviderCapabilities.with_image()


def test_minimax_is_with_image():
    p = MinimaxProvider(api_key="test")
    assert p.capabilities == ProviderCapabilities.with_image()


def test_ollama_native_is_text_only():
    p = OllamaProvider(model="llama3.2")
    assert p.capabilities == ProviderCapabilities.text_only()


def test_claude_code_is_text_only():
    p = ClaudeCodeClient()
    assert p.capabilities == ProviderCapabilities.text_only()
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_provider_capabilities.py -v -k capability`
Expected: FAIL — `AttributeError: 'AnthropicProvider' object has no attribute 'capabilities'`.

- [ ] **Step 3: Add `capabilities` attribute to each provider**

For `sdks/python/motosan_ai/providers/anthropic.py`, add the import and attribute near the class definition (line 26):

```python
from motosan_ai.provider_base import ProviderCapabilities


class AnthropicProvider:
    capabilities: ProviderCapabilities = ProviderCapabilities.full()

    def __init__(
        self,
        ...
```

Repeat the same 2-line change in:

- `sdks/python/motosan_ai/providers/openai.py` — `capabilities = ProviderCapabilities.with_image()`
- `sdks/python/motosan_ai/providers/minimax.py` — `capabilities = ProviderCapabilities.with_image()`
- `sdks/python/motosan_ai/providers/ollama.py` — `capabilities = ProviderCapabilities.text_only()` (on `OllamaProvider`)
- `sdks/python/motosan_ai/providers/claude_code.py` — `capabilities = ProviderCapabilities.text_only()` (on `ClaudeCodeClient`)

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS — all 5 new capability tests green, no regressions.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/ sdks/python/tests/test_provider_capabilities.py
git commit -m "feat(python): declare ProviderCapabilities on each provider"
```

---

## Task 16: Export new public types from `motosan_ai/__init__.py`

**Files:**
- Modify: `sdks/python/motosan_ai/__init__.py`
- Modify: `sdks/python/tests/test_types.py`

- [ ] **Step 1: Append import smoke test**

Append to `sdks/python/tests/test_types.py`:

```python
def test_public_api_exports_new_types():
    import motosan_ai as m

    # New types from Phase 1
    assert m.ContentBlock is not None
    assert m.TextBlock is not None
    assert m.ImageBlock is not None
    assert m.DocumentBlock is not None
    assert m.ImageSourceBase64 is not None
    assert m.ImageSourceUrl is not None
    assert m.DocumentSourceBase64 is not None
    assert m.DocumentSourceUrl is not None
    assert m.SystemBlock is not None
    assert m.ToolChoice is not None
    assert m.ThinkingConfig is not None
    assert m.McpServerConfig is not None
    assert m.McpToolConfigAll is not None
    assert m.McpToolConfigAllowed is not None
    assert m.McpToolConfigDenied is not None
    assert m.ProviderCapabilities is not None
    assert m.StreamEventType is not None
```

- [ ] **Step 2: Run test to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_types.py::test_public_api_exports_new_types -v`
Expected: FAIL — `AttributeError: module 'motosan_ai' has no attribute 'ContentBlock'`.

- [ ] **Step 3: Export new types**

Rewrite `sdks/python/motosan_ai/__init__.py`:

```python
from motosan_ai.client import Client, Provider
from motosan_ai.error import (
    AuthError,
    ConfigError,
    InvalidRequestError,
    MotosanError,
    NetworkError,
    ProviderError,
    RateLimitError,
    StreamError,
)
from motosan_ai.provider_base import ProviderCapabilities
from motosan_ai.providers.claude_code import ClaudeCodeClient
from motosan_ai.types import (
    ChatRequest,
    ChatRequestBuilder,
    ChatResponse,
    ContentBlock,
    DocumentBlock,
    DocumentSource,
    DocumentSourceBase64,
    DocumentSourceUrl,
    ImageBlock,
    ImageSource,
    ImageSourceBase64,
    ImageSourceUrl,
    McpServerConfig,
    McpToolConfig,
    McpToolConfigAll,
    McpToolConfigAllowed,
    McpToolConfigDenied,
    Message,
    Role,
    StopReason,
    StreamEvent,
    StreamEventType,
    SystemBlock,
    TextBlock,
    ThinkingConfig,
    Tool,
    ToolCall,
    ToolChoice,
    Usage,
)

__all__ = [
    "AuthError",
    "ChatRequest",
    "ChatRequestBuilder",
    "ChatResponse",
    "ClaudeCodeClient",
    "Client",
    "ConfigError",
    "ContentBlock",
    "DocumentBlock",
    "DocumentSource",
    "DocumentSourceBase64",
    "DocumentSourceUrl",
    "ImageBlock",
    "ImageSource",
    "ImageSourceBase64",
    "ImageSourceUrl",
    "InvalidRequestError",
    "McpServerConfig",
    "McpToolConfig",
    "McpToolConfigAll",
    "McpToolConfigAllowed",
    "McpToolConfigDenied",
    "Message",
    "MotosanError",
    "NetworkError",
    "Provider",
    "ProviderCapabilities",
    "ProviderError",
    "RateLimitError",
    "Role",
    "StopReason",
    "StreamError",
    "StreamEvent",
    "StreamEventType",
    "SystemBlock",
    "TextBlock",
    "ThinkingConfig",
    "Tool",
    "ToolCall",
    "ToolChoice",
    "Usage",
]
```

- [ ] **Step 4: Run test to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS — all tests green.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/__init__.py sdks/python/tests/test_types.py
git commit -m "feat(python): export Phase 1 types from top-level module"
```

---

## Task 17: Release — CHANGELOG + version bump to 0.6.0

**Files:**
- Modify: `sdks/python/CHANGELOG.md`
- Modify: `sdks/python/pyproject.toml:3`

- [ ] **Step 1: Bump version**

Edit `sdks/python/pyproject.toml`:

```toml
version = "0.6.0"
```

- [ ] **Step 2: Prepend CHANGELOG entry**

At the top of `sdks/python/CHANGELOG.md`, below the title, prepend:

```markdown
## [0.6.0] - 2026-04-24

### Added
- **Type foundation for Rust SDK parity** — additive-only. No wire-format changes yet.
- `ContentBlock` discriminated union (`TextBlock` / `ImageBlock` / `DocumentBlock`).
- `ImageSource` (`ImageSourceBase64` / `ImageSourceUrl`) and `DocumentSource` (`DocumentSourceBase64` / `DocumentSourceUrl`).
- `Message.user_with_image()`, `Message.user_with_blocks()`, `Message.user_with_pdf_base64()`, `Message.user_with_pdf_url()`, `Message.user_with_pdf_bytes()`.
- `Message.cache` field + `Message.user_with_cache()` + `Message.with_cache()`.
- `SystemBlock` with `SystemBlock.new()` / `SystemBlock.cached()` factories.
- `Tool.cache` field.
- `ToolChoice` with `auto()` / `required()` / `none()` / `tool(name)` factories.
- `ThinkingConfig` (budget_tokens) for extended thinking.
- `McpServerConfig` and `McpToolConfig*` (All / Allowed / Denied) for server-side MCP.
- `ChatRequest` fields: `system_blocks`, `system_cache`, `tool_choice`, `mcp_servers`, `mcp_tool_configs`, `thinking`, `stop_sequences`.
- `ChatRequest.builder()` returning `ChatRequestBuilder` (fluent API parity with Rust SDK).
- `ChatResponse.thinking` field.
- `Usage.cache_creation_input_tokens` and `Usage.cache_read_input_tokens`.
- `StopReason.stop_sequence` variant.
- `StreamEventType` enum; `StreamEvent.usage` and `StreamEvent.stop_reason` fields.
- `ProviderCapabilities` (`text_only` / `with_image` / `full`) declared on each provider.
- `BaseProvider` ABC with default `validate_request()` enforcing capabilities.

### Changed
- Capability declarations per provider: `Anthropic` = `full`, `OpenAI` = `with_image`, `Minimax` = `with_image`, `Ollama` = `text_only`, `ClaudeCodeClient` = `text_only`.

### Notes
- **No new wire-format behavior in 0.6.0.** Providers still serialize request bodies as before. Phase 2 (v0.7.0+) will wire `content_blocks`, `system_blocks`, `tool_choice`, `thinking`, and MCP config into the Anthropic and Gemini providers.
- See `docs/superpowers/plans/2026-04-24-python-sdk-catchup-roadmap.md` for the full catch-up roadmap.
```

- [ ] **Step 3: Run the full gate**

Run: `cd /Users/daiwanwei/Projects/wade/motosan-ai && check-python`
Expected: ruff + format + pytest all pass.

- [ ] **Step 4: Commit**

```bash
git add sdks/python/pyproject.toml sdks/python/CHANGELOG.md
git commit -m "chore(python): release v0.6.0 — type foundation for Rust parity"
```

---

## Final Self-Review Checklist

Verified 2026-04-24 — all items ✅:

- [x] `cd sdks/python && uv run pytest tests/ -v` — **168 passed, 7 skipped (live-only)**.
- [x] `uv run ruff check .` + `uv run ruff format --check .` — clean.
- [ ] `check-rust` — not re-run (Phase 1 touched no Rust code; existing CI covers).
- [x] Every type in `specs/types.md` has a Python equivalent — verified in `types.py` (ContentBlock, SystemBlock, ToolChoice, ThinkingConfig, McpServerConfig, McpToolConfig, ProviderCapabilities, Usage cache fields, StreamEventType, StreamEvent.stop_reason/usage).
- [x] `import motosan_ai` exposes every new type via `__all__` — confirmed in [motosan_ai/__init__.py](sdks/python/motosan_ai/__init__.py).
- [x] No existing provider's wire format changed — capability declarations are class attributes only; `_build_body` untouched.
- [x] Version in `pyproject.toml` is `0.6.0` and `CHANGELOG.md` has a matching entry.
- [x] No `TODO` / `FIXME` / placeholder strings introduced.

---

## What Phase 1 does NOT do

- ❌ Send images, PDFs, system_blocks, tool_choice, thinking, or MCP over the wire.
- ❌ Add the Gemini / Codex CLI / Gemini CLI / Gemini Code Assist providers.
- ❌ Add `chat_with()` / `stream_with()` / `stream_collect()` methods.
- ❌ Change the `Client` dispatch surface.
- ❌ Remove `chat_sync()` (stays for now, deprecated in Phase 4).

All of the above are tracked by the roadmap doc and get dedicated plans before implementation.
