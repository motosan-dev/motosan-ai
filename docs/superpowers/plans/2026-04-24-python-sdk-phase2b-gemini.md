# Python SDK Phase 2b — Gemini HTTP Provider Implementation Plan

> **Status:** ✅ **COMPLETE (2026-04-24)** — shipped as `motosan-ai` v0.8.0; default model updated in v0.8.2 to `gemini-2.5-flash`.
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a native `GeminiProvider` that calls Google's Generative Language REST API at `generativelanguage.googleapis.com/v1beta`. Full feature coverage: text, vision, tools, streaming, stop sequences, tool choice. No PDF (Gemini doesn't support document blocks).

**Architecture:** New `motosan_ai/providers/gemini.py` module. Single class `GeminiProvider` inheriting `BaseProvider` with `capabilities = with_image()`. Wire format follows Google's `generateContent` / `streamGenerateContent` spec: messages map to `contents[].parts[]` with `role: "user" | "model"`, tool calls to `functionCall` / `functionResponse` parts, tools to `tools[].functionDeclarations[]`. `Client` dispatch gains a new `Provider.gemini` variant.

**Tech Stack:** Python 3.11+, `httpx`, `respx` for mocks, `pytest-asyncio`. No new dependencies.

**Ships as:** `motosan-ai` v0.8.0.

---

## Reference material

- **Rust implementation:** [sdks/rust/src/providers/gemini.rs](sdks/rust/src/providers/gemini.rs) — canonical wire format. Lines 80-239 is `build_request`; 241-313 is `parse_response`; 421-538 is the SSE adapter.
- **Gemini API surface:**
  - Non-stream endpoint: `POST /v1beta/models/{model}:generateContent`
  - Stream endpoint: `POST /v1beta/models/{model}:streamGenerateContent?alt=sse`
  - Auth header: `x-goog-api-key: <key>` (not `Authorization`)
  - Default model: `gemini-2.5-flash`
- **Role mapping gotcha:** Python SDK's `Role.assistant` → Gemini `"model"`; our `Role.tool` result → Gemini `"user"` role with a `functionResponse` part (Google merges tool results back into user turns).
- **Tool call ID convention:** Gemini does not assign IDs to function calls. The Python SDK generates UUIDs on receipt. For tool_result messages, `tool_call_id` holds the **function name** (matches Rust SDK convention — see gemini.rs line 139-140 comment).

---

## File Structure

| Path | Responsibility | Status |
|------|----------------|--------|
| `sdks/python/motosan_ai/providers/gemini.py` | Full `GeminiProvider` — request build, response parse, stream adapter | **Create** (~400 lines) |
| `sdks/python/motosan_ai/providers/__init__.py` | Export `GeminiProvider` | **Modify** |
| `sdks/python/motosan_ai/client.py` | Register `Provider.gemini`; env-var `GEMINI_API_KEY`; classmethod `Client.gemini()` | **Modify** |
| `sdks/python/motosan_ai/__init__.py` | Export `GeminiProvider` from top level | **Modify** |
| `sdks/python/tests/test_gemini_request.py` | Request body serialization (text, system, tools, tool_choice, content_blocks) | **Create** |
| `sdks/python/tests/test_gemini_response.py` | Non-stream response parsing (text, tool calls, usage, finishReason) | **Create** |
| `sdks/python/tests/test_gemini_stream.py` | SSE streaming (text delta, tool_call, usage, stop_reason) | **Create** |
| `sdks/python/tests/test_gemini_errors.py` | 401/429/5xx mapping + retry | **Create** |
| `sdks/python/tests/test_gemini_capabilities.py` | Rejects document blocks; accepts image + text | **Create** |
| `sdks/python/tests/test_gemini_client_dispatch.py` | `Client.gemini()` / `Client(provider=Provider.gemini)` wire through | **Create** |
| `sdks/python/tests/integration/test_gemini_live.py` | Live smoke tests (text, vision, tools) | **Create** |
| `sdks/python/CHANGELOG.md` | v0.8.0 entry | **Modify** |
| `sdks/python/pyproject.toml` | Version bump 0.7.0 → 0.8.0; add `gemini` optional dep extra | **Modify** |

Design principles:
- **Build before parse.** Serialize first (Tasks 2-7) with mock tests — confirms wire format matches Rust byte-for-byte before wiring HTTP.
- **Mock-first.** Every feature pinned with `respx` against actual Google API URL shapes.
- **Capability guard at entry.** `validate_request()` runs before HTTP; tests verify DocumentBlock raises before any network call.
- **No ad-hoc sync for Gemini.** Async-only, consistent with `CLAUDE.md` rules.

---

## Task 1: `GeminiProvider` skeleton

**Files:**
- Create: `sdks/python/motosan_ai/providers/gemini.py`
- Create: `sdks/python/tests/test_gemini_capabilities.py`

Provider class with constructor, auth header, URL helpers, capability declaration, and `validate_request()` wiring. No HTTP yet — just the shell.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_gemini_capabilities.py`:

```python
import pytest

from motosan_ai.error import InvalidRequestError
from motosan_ai.provider_base import ProviderCapabilities
from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.types import ChatRequest, Message


@pytest.fixture
def provider():
    return GeminiProvider(api_key="test-key")


def test_default_model_is_gemini_2_flash(provider):
    assert provider.model == "gemini-2.5-flash"


def test_capabilities_is_with_image(provider):
    assert provider.capabilities == ProviderCapabilities.with_image()


def test_generate_url_includes_model(provider):
    req = ChatRequest(messages=[Message.user("hi")])
    url = provider._generate_url(req)
    assert url.endswith("/v1beta/models/gemini-2.5-flash:generateContent")


def test_stream_url_has_alt_sse(provider):
    req = ChatRequest(messages=[Message.user("hi")])
    url = provider._stream_url(req)
    assert url.endswith(
        "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
    )


def test_per_request_model_overrides_default(provider):
    req = ChatRequest(messages=[Message.user("hi")], model="gemini-2.5-pro")
    url = provider._generate_url(req)
    assert "/gemini-2.5-pro:" in url


def test_auth_header_uses_x_goog_api_key(provider):
    headers = provider._headers()
    assert headers["x-goog-api-key"] == "test-key"
    assert "authorization" not in headers


@pytest.mark.asyncio
async def test_validate_rejects_pdf_document(provider):
    req = ChatRequest(messages=[Message.user_with_pdf_base64("read", "abc")])
    with pytest.raises(InvalidRequestError, match="document"):
        provider.validate_request(req)


@pytest.mark.asyncio
async def test_validate_accepts_image(provider):
    req = ChatRequest(messages=[Message.user_with_image("see", "abc", "image/png")])
    provider.validate_request(req)  # should not raise
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_gemini_capabilities.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'motosan_ai.providers.gemini'`.

- [ ] **Step 3: Create `gemini.py` skeleton**

Create `sdks/python/motosan_ai/providers/gemini.py`:

```python
from __future__ import annotations

import uuid
from collections.abc import AsyncIterator
from typing import Any

import httpx

from motosan_ai.provider_base import BaseProvider, ProviderCapabilities
from motosan_ai.types import ChatRequest, ChatResponse, StreamEvent

_DEFAULT_BASE_URL = "https://generativelanguage.googleapis.com/v1beta"
_DEFAULT_MODEL = "gemini-2.5-flash"
_DEFAULT_MAX_TOKENS = 8192


def _gen_tool_call_id() -> str:
    return f"call_{uuid.uuid4().hex[:12]}"


class GeminiProvider(BaseProvider):
    capabilities: ProviderCapabilities = ProviderCapabilities.with_image()

    def __init__(
        self,
        api_key: str,
        model: str | None = None,
        base_url: str | None = None,
    ) -> None:
        self.api_key = api_key
        self.model = model or _DEFAULT_MODEL
        self.base_url = (base_url or _DEFAULT_BASE_URL).rstrip("/")
        self._http = httpx.AsyncClient(timeout=120.0)

    def _model_for(self, request: ChatRequest) -> str:
        return request.model or self.model

    def _generate_url(self, request: ChatRequest) -> str:
        return f"{self.base_url}/models/{self._model_for(request)}:generateContent"

    def _stream_url(self, request: ChatRequest) -> str:
        return (
            f"{self.base_url}/models/{self._model_for(request)}"
            ":streamGenerateContent?alt=sse"
        )

    def _headers(self) -> dict[str, str]:
        return {
            "x-goog-api-key": self.api_key,
            "content-type": "application/json",
        }

    async def chat(self, request: ChatRequest) -> ChatResponse:
        self.validate_request(request)
        raise NotImplementedError("wired in Task 8")

    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        self.validate_request(request)
        raise NotImplementedError("wired in Task 10")
        yield  # pragma: no cover — makes this a generator
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_gemini_capabilities.py -v`
Expected: PASS — 8 tests.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini.py sdks/python/tests/test_gemini_capabilities.py
git commit -m "feat(python,gemini): add GeminiProvider skeleton with capability guard"
```

---

## Task 2: Serialize simple text messages (`_build_body`)

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini.py`
- Create: `sdks/python/tests/test_gemini_request.py`

User messages become `role: "user"`; assistant messages become `role: "model"`. System messages are extracted (handled in Task 3). Tool results handled in Task 7.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_gemini_request.py`:

```python
import pytest

from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.types import ChatRequest, Message


@pytest.fixture
def provider():
    return GeminiProvider(api_key="test-key")


def test_simple_user_message(provider):
    body = provider._build_body(ChatRequest(messages=[Message.user("Hello")]))
    assert body["contents"] == [{"role": "user", "parts": [{"text": "Hello"}]}]


def test_assistant_becomes_model_role(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user("hi"), Message.assistant("hello back")]
        )
    )
    assert body["contents"][0]["role"] == "user"
    assert body["contents"][1] == {"role": "model", "parts": [{"text": "hello back"}]}


def test_multi_turn_conversation(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[
                Message.user("q1"),
                Message.assistant("a1"),
                Message.user("q2"),
            ]
        )
    )
    roles = [c["role"] for c in body["contents"]]
    assert roles == ["user", "model", "user"]


def test_empty_content_still_produces_part(provider):
    """Gemini rejects empty parts — we emit an empty text part as placeholder."""
    body = provider._build_body(ChatRequest(messages=[Message.user("")]))
    assert body["contents"][0]["parts"] == [{"text": ""}]
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_gemini_request.py -v`
Expected: FAIL — `AttributeError: 'GeminiProvider' object has no attribute '_build_body'`.

- [ ] **Step 3: Add `_build_body` to `GeminiProvider`**

Add to `gemini.py`, inside the class:

```python
from motosan_ai.types import Role


def _build_body(self, request: ChatRequest) -> dict[str, Any]:
    contents: list[dict[str, Any]] = []

    for msg in request.messages:
        if msg.role == Role.system:
            # handled as systemInstruction in Task 3
            continue
        if msg.role == Role.user:
            parts: list[dict[str, Any]] = [{"text": msg.content}]
            contents.append({"role": "user", "parts": parts})
            continue
        if msg.role == Role.assistant:
            parts = [{"text": msg.content}]
            contents.append({"role": "model", "parts": parts})
            continue
        # tool results wired in Task 7

    body: dict[str, Any] = {"contents": contents}
    return body
```

Note: the method must be **unbound** — add it as a method of `GeminiProvider`. The `from motosan_ai.types import Role` import goes at the top of the file.

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_gemini_request.py -v`
Expected: PASS — 4 tests.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini.py sdks/python/tests/test_gemini_request.py
git commit -m "feat(python,gemini): serialize simple text user/assistant messages"
```

---

## Task 3: System prompt → `systemInstruction`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini.py` (`_build_body`)
- Modify: `sdks/python/tests/test_gemini_request.py`

Priority: `system_blocks` (joined with `\n` — Gemini ignores `cache_control`) > `system` string > extracted `role: system` messages. Result goes in top-level `systemInstruction: {"parts": [{"text": ...}]}`.

- [ ] **Step 1: Append failing tests**

```python
from motosan_ai.types import SystemBlock


def test_system_string_becomes_system_instruction(provider):
    body = provider._build_body(
        ChatRequest(messages=[Message.user("hi")], system="Be concise.")
    )
    assert body["systemInstruction"] == {"parts": [{"text": "Be concise."}]}
    # system message not in contents
    assert len(body["contents"]) == 1


def test_extracted_system_role_becomes_system_instruction(provider):
    body = provider._build_body(
        ChatRequest(messages=[Message.system("Be concise."), Message.user("hi")])
    )
    assert body["systemInstruction"] == {"parts": [{"text": "Be concise."}]}


def test_system_blocks_joined_with_newlines(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user("hi")],
            system_blocks=[SystemBlock.new("Block A"), SystemBlock.new("Block B")],
        )
    )
    assert body["systemInstruction"] == {"parts": [{"text": "Block A\nBlock B"}]}


def test_system_blocks_take_priority_over_system_string(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user("hi")],
            system="IGNORED",
            system_blocks=[SystemBlock.new("WINS")],
        )
    )
    assert body["systemInstruction"] == {"parts": [{"text": "WINS"}]}


def test_no_system_omits_instruction(provider):
    body = provider._build_body(ChatRequest(messages=[Message.user("hi")]))
    assert "systemInstruction" not in body
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_gemini_request.py -v -k system`
Expected: FAIL on first four; last passes.

- [ ] **Step 3: Extend `_build_body`**

Replace the loop header in `_build_body`:

```python
def _build_body(self, request: ChatRequest) -> dict[str, Any]:
    contents: list[dict[str, Any]] = []
    extracted_system: str | None = None

    for msg in request.messages:
        if msg.role == Role.system:
            if msg.content.strip():
                extracted_system = msg.content
            continue
        # ... existing user / assistant branches ...
```

After the loop, compose `systemInstruction`:

```python
    # Priority: system_blocks > system > extracted
    system_text = ""
    if request.system_blocks:
        system_text = "\n".join(b.text for b in request.system_blocks if b.text)
    if not system_text and request.system:
        system_text = request.system
    if not system_text and extracted_system:
        system_text = extracted_system

    body: dict[str, Any] = {"contents": contents}
    if system_text:
        body["systemInstruction"] = {"parts": [{"text": system_text}]}
    return body
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_gemini_request.py -v`
Expected: PASS — 9 tests total.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini.py sdks/python/tests/test_gemini_request.py
git commit -m "feat(python,gemini): serialize system prompts to systemInstruction"
```

---

## Task 4: `generationConfig` — temperature, max_tokens, stop_sequences

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini.py` (`_build_body`)
- Modify: `sdks/python/tests/test_gemini_request.py`

Gemini groups generation parameters under `generationConfig`. Field mapping: `temperature` → `temperature`, `max_tokens` → `maxOutputTokens` (default 8192), `stop_sequences` → `stopSequences`.

- [ ] **Step 1: Append failing tests**

```python
def test_generation_config_default_max_tokens(provider):
    body = provider._build_body(ChatRequest(messages=[Message.user("hi")]))
    assert body["generationConfig"]["maxOutputTokens"] == 8192


def test_generation_config_custom_values(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user("hi")],
            temperature=0.3,
            max_tokens=512,
            stop_sequences=["END"],
        )
    )
    cfg = body["generationConfig"]
    assert cfg["temperature"] == 0.3
    assert cfg["maxOutputTokens"] == 512
    assert cfg["stopSequences"] == ["END"]


def test_empty_stop_sequences_omitted(provider):
    body = provider._build_body(
        ChatRequest(messages=[Message.user("hi")], stop_sequences=[])
    )
    assert "stopSequences" not in body["generationConfig"]
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_gemini_request.py::test_generation_config_default_max_tokens -v`
Expected: FAIL — `generationConfig` missing.

- [ ] **Step 3: Add `generationConfig` in `_build_body`**

Before the final `return body`, compose the config:

```python
    gen_config: dict[str, Any] = {
        "maxOutputTokens": request.max_tokens or _DEFAULT_MAX_TOKENS,
    }
    if request.temperature is not None:
        gen_config["temperature"] = request.temperature
    if request.stop_sequences:
        gen_config["stopSequences"] = list(request.stop_sequences)
    body["generationConfig"] = gen_config
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_gemini_request.py -v`
Expected: PASS — 12 tests total.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini.py sdks/python/tests/test_gemini_request.py
git commit -m "feat(python,gemini): serialize generationConfig (temperature, maxOutputTokens, stopSequences)"
```

---

## Task 5: Content blocks → `inlineData` / `fileData`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini.py` (`_build_body`)
- Modify: `sdks/python/tests/test_gemini_request.py`

Image blocks with base64 source → `{"inlineData": {"mimeType": ..., "data": ...}}`. Image blocks with URL source → `{"fileData": {"fileUri": url}}`. Document blocks **must not appear** — `validate_request()` rejects them (guarded by `capabilities = with_image()`).

- [ ] **Step 1: Append failing tests**

```python
from motosan_ai.types import ImageBlock, ImageSourceBase64, ImageSourceUrl, TextBlock


def test_user_with_image_base64_becomes_inline_data(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user_with_image("describe", "JVBER", "image/png")]
        )
    )
    parts = body["contents"][0]["parts"]
    # Phase-1 factory builds [TextBlock, ImageBlock] — so we expect 2 parts
    assert parts == [
        {"text": "describe"},
        {"inlineData": {"mimeType": "image/png", "data": "JVBER"}},
    ]


def test_image_url_becomes_file_data(provider):
    msg = Message.user_with_blocks(
        [TextBlock(text="see"), ImageBlock(source=ImageSourceUrl(url="https://x.com/i.png"))]
    )
    body = provider._build_body(ChatRequest(messages=[msg]))
    parts = body["contents"][0]["parts"]
    assert parts == [
        {"text": "see"},
        {"fileData": {"fileUri": "https://x.com/i.png"}},
    ]


def test_user_message_without_blocks_only_emits_content_part(provider):
    """When content_blocks is empty, serialize just the plain content text."""
    body = provider._build_body(ChatRequest(messages=[Message.user("plain")]))
    assert body["contents"][0]["parts"] == [{"text": "plain"}]
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_gemini_request.py -v -k image or inline or file_data`
Expected: FAIL — content blocks ignored.

- [ ] **Step 3: Extend user-branch serialization**

Replace the `if msg.role == Role.user:` branch:

```python
if msg.role == Role.user:
    parts: list[dict[str, Any]] = []
    if msg.content_blocks:
        for block in msg.content_blocks:
            parts.extend(_part_for_block(block))
    else:
        parts.append({"text": msg.content})
    if not parts:
        parts.append({"text": ""})
    contents.append({"role": "user", "parts": parts})
    continue
```

Add module-level helper `_part_for_block`:

```python
from motosan_ai.types import (
    ImageBlock,
    ImageSourceBase64,
    ImageSourceUrl,
    TextBlock,
)


def _part_for_block(block: Any) -> list[dict[str, Any]]:
    if isinstance(block, TextBlock):
        return [{"text": block.text}]
    if isinstance(block, ImageBlock):
        src = block.source
        if isinstance(src, ImageSourceBase64):
            return [{"inlineData": {"mimeType": src.media_type, "data": src.data}}]
        if isinstance(src, ImageSourceUrl):
            return [{"fileData": {"fileUri": src.url}}]
    # DocumentBlock should have been rejected by validate_request(); defensive fallback:
    return []
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_gemini_request.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini.py sdks/python/tests/test_gemini_request.py
git commit -m "feat(python,gemini): serialize image content blocks as inlineData / fileData"
```

---

## Task 6: Tool declarations + `ToolChoice`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini.py` (`_build_body`)
- Modify: `sdks/python/tests/test_gemini_request.py`

Serialization:
- `tools = [Tool(name, description, input_schema)]` → `body["tools"] = [{"functionDeclarations": [{name, description, parameters}, ...]}]` (single array element wrapping all declarations).
- `tool_choice` → `body["toolConfig"] = {"functionCallingConfig": {"mode": "...", "allowedFunctionNames": [...]?}}`
  - `auto` → mode `AUTO`
  - `required` → mode `ANY`
  - `none` → remove tools entirely (no `toolConfig` either)
  - `tool(name)` → mode `ANY` + `allowedFunctionNames: [name]`

- [ ] **Step 1: Append failing tests**

```python
from motosan_ai.types import Tool, ToolChoice


def test_tools_wrap_in_function_declarations_array(provider):
    tools = [
        Tool(
            name="get_weather",
            description="Weather for a city",
            input_schema={"type": "object", "properties": {"city": {"type": "string"}}},
        )
    ]
    body = provider._build_body(
        ChatRequest(messages=[Message.user("?")], tools=tools)
    )
    assert body["tools"] == [
        {
            "functionDeclarations": [
                {
                    "name": "get_weather",
                    "description": "Weather for a city",
                    "parameters": {
                        "type": "object",
                        "properties": {"city": {"type": "string"}},
                    },
                }
            ]
        }
    ]


def test_tool_without_schema_omits_parameters(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user("?")], tools=[Tool(name="x", description="X")]
        )
    )
    decl = body["tools"][0]["functionDeclarations"][0]
    assert "parameters" not in decl


def test_tool_choice_auto_is_default_mode(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user("?")],
            tools=[Tool(name="x")],
            tool_choice=ToolChoice.auto(),
        )
    )
    assert body["toolConfig"]["functionCallingConfig"]["mode"] == "AUTO"


def test_tool_choice_required_is_any(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user("?")],
            tools=[Tool(name="x")],
            tool_choice=ToolChoice.required(),
        )
    )
    assert body["toolConfig"]["functionCallingConfig"]["mode"] == "ANY"


def test_tool_choice_none_removes_tools_and_toolconfig(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user("?")],
            tools=[Tool(name="x")],
            tool_choice=ToolChoice.none(),
        )
    )
    assert "tools" not in body
    assert "toolConfig" not in body


def test_tool_choice_specific_tool_restricts_allowed_names(provider):
    body = provider._build_body(
        ChatRequest(
            messages=[Message.user("?")],
            tools=[Tool(name="get_weather"), Tool(name="search")],
            tool_choice=ToolChoice.tool("get_weather"),
        )
    )
    cfg = body["toolConfig"]["functionCallingConfig"]
    assert cfg["mode"] == "ANY"
    assert cfg["allowedFunctionNames"] == ["get_weather"]
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_gemini_request.py -v -k tool`
Expected: FAIL.

- [ ] **Step 3: Extend `_build_body`**

Add after the `generationConfig` composition, before `return body`:

```python
    if request.tools:
        declarations: list[dict[str, Any]] = []
        for t in request.tools:
            decl: dict[str, Any] = {
                "name": t.name,
                "description": t.description or "",
            }
            if t.input_schema:
                decl["parameters"] = t.input_schema
            declarations.append(decl)
        body["tools"] = [{"functionDeclarations": declarations}]

        # tool_choice
        tc = request.tool_choice
        if tc is not None and tc.type == "none":
            body.pop("tools", None)
        else:
            mode = "AUTO"
            if tc is not None:
                if tc.type == "required":
                    mode = "ANY"
                elif tc.type == "tool":
                    mode = "ANY"
            fc_config: dict[str, Any] = {"mode": mode}
            if tc is not None and tc.type == "tool":
                fc_config["allowedFunctionNames"] = [tc.name]
            body["toolConfig"] = {"functionCallingConfig": fc_config}
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_gemini_request.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini.py sdks/python/tests/test_gemini_request.py
git commit -m "feat(python,gemini): serialize tool declarations and tool_choice"
```

---

## Task 7: Tool call + tool result parts (`functionCall` / `functionResponse`)

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini.py` (`_build_body`)
- Modify: `sdks/python/tests/test_gemini_request.py`

- Assistant messages with `tool_calls` → each `ToolCall` becomes `{"functionCall": {"name": ..., "args": ...}}` part appended alongside any text part.
- Tool result messages (`role: tool`) → `role: "user"` with `[{"functionResponse": {"name": <function>, "response": <parsed>}}]`. The function name comes from `tool_call_id` (our SDK's convention for Gemini).

- [ ] **Step 1: Append failing tests**

```python
from motosan_ai.types import ToolCall


def test_assistant_tool_call_becomes_function_call_part(provider):
    tc = ToolCall(id="ignored", name="get_weather", input={"city": "Taipei"})
    msg = Message.assistant_with_tool_calls("checking...", [tc])
    body = provider._build_body(ChatRequest(messages=[Message.user("weather?"), msg]))
    parts = body["contents"][1]["parts"]
    assert parts == [
        {"text": "checking..."},
        {"functionCall": {"name": "get_weather", "args": {"city": "Taipei"}}},
    ]


def test_assistant_tool_call_without_text_still_valid(provider):
    tc = ToolCall(id="x", name="get_weather", input={})
    msg = Message.assistant_with_tool_calls("", [tc])
    body = provider._build_body(ChatRequest(messages=[Message.user("?"), msg]))
    parts = body["contents"][1]["parts"]
    assert parts == [{"functionCall": {"name": "get_weather", "args": {}}}]


def test_tool_result_becomes_user_role_with_function_response(provider):
    """tool_call_id holds the function name in Gemini convention."""
    tool_msg = Message.tool_result("get_weather", '{"result": "sunny"}')
    body = provider._build_body(
        ChatRequest(messages=[Message.user("?"), tool_msg])
    )
    content = body["contents"][1]
    assert content["role"] == "user"
    assert content["parts"] == [
        {"functionResponse": {"name": "get_weather", "response": {"result": "sunny"}}}
    ]


def test_tool_result_with_non_json_content_wraps_in_result_field(provider):
    tool_msg = Message.tool_result("x", "just a plain string")
    body = provider._build_body(
        ChatRequest(messages=[Message.user("?"), tool_msg])
    )
    part = body["contents"][1]["parts"][0]
    assert part["functionResponse"]["response"] == {"result": "just a plain string"}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_gemini_request.py -v -k function or tool_call or tool_result`
Expected: FAIL — tool branches missing.

- [ ] **Step 3: Extend user / assistant / tool branches**

Replace the `assistant` branch in `_build_body`:

```python
if msg.role == Role.assistant:
    parts: list[dict[str, Any]] = []
    if msg.content:
        parts.append({"text": msg.content})
    for tc in msg.tool_calls:
        parts.append({"functionCall": {"name": tc.name, "args": tc.input}})
    if not parts:
        parts.append({"text": ""})
    contents.append({"role": "model", "parts": parts})
    continue
```

Add a `tool` branch at the end of the role dispatch:

```python
if msg.role == Role.tool:
    name = msg.tool_call_id or ""
    import json as _json
    try:
        response = _json.loads(msg.content)
    except (_json.JSONDecodeError, TypeError):
        response = {"result": msg.content}
    if not isinstance(response, dict):
        response = {"result": response}
    contents.append(
        {
            "role": "user",
            "parts": [{"functionResponse": {"name": name, "response": response}}],
        }
    )
    continue
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_gemini_request.py -v`
Expected: PASS — ~22 tests total.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini.py sdks/python/tests/test_gemini_request.py
git commit -m "feat(python,gemini): serialize functionCall / functionResponse tool parts"
```

---

## Task 8: Non-streaming `chat()` HTTP + response parsing

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini.py` (`chat`, `_parse_response`)
- Create: `sdks/python/tests/test_gemini_response.py`

Parse: `candidates[0].content.parts` — each `text` part appends to content; each `functionCall` part becomes a `ToolCall` with a freshly generated ID. `finishReason` → `StopReason` (`STOP` + tool_calls → `tool_use`, `STOP` → `end_turn`, `MAX_TOKENS` → `max_tokens`). `usageMetadata.promptTokenCount` + `candidatesTokenCount` → `Usage`. `modelVersion` → `ChatResponse.model`.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_gemini_response.py`:

```python
import httpx
import pytest
import respx

from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.types import ChatRequest, Message, StopReason


@pytest.fixture
def provider():
    return GeminiProvider(api_key="test-key")


def _url():
    return (
        "https://generativelanguage.googleapis.com/v1beta"
        "/models/gemini-2.5-flash:generateContent"
    )


@respx.mock
@pytest.mark.asyncio
async def test_chat_parses_text_response(provider):
    respx.post(_url()).mock(
        return_value=httpx.Response(
            200,
            json={
                "candidates": [
                    {
                        "content": {"parts": [{"text": "Hello!"}], "role": "model"},
                        "finishReason": "STOP",
                    }
                ],
                "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 2},
                "modelVersion": "gemini-2.5-flash-001",
            },
        )
    )
    resp = await provider.chat(ChatRequest(messages=[Message.user("Hi")]))

    assert resp.content == "Hello!"
    assert resp.stop_reason == StopReason.end_turn
    assert resp.usage.input_tokens == 5
    assert resp.usage.output_tokens == 2
    assert resp.model == "gemini-2.5-flash-001"


@respx.mock
@pytest.mark.asyncio
async def test_chat_parses_function_call_as_tool_call(provider):
    respx.post(_url()).mock(
        return_value=httpx.Response(
            200,
            json={
                "candidates": [
                    {
                        "content": {
                            "parts": [
                                {
                                    "functionCall": {
                                        "name": "get_weather",
                                        "args": {"city": "Taipei"},
                                    }
                                }
                            ],
                            "role": "model",
                        },
                        "finishReason": "STOP",
                    }
                ],
                "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 5},
                "modelVersion": "gemini-2.5-flash",
            },
        )
    )
    resp = await provider.chat(ChatRequest(messages=[Message.user("weather?")]))

    assert len(resp.tool_calls) == 1
    assert resp.tool_calls[0].name == "get_weather"
    assert resp.tool_calls[0].input == {"city": "Taipei"}
    assert resp.tool_calls[0].id  # some UUID-like id
    assert resp.stop_reason == StopReason.tool_use  # STOP + tool_calls → tool_use


@respx.mock
@pytest.mark.asyncio
async def test_chat_max_tokens_finish_reason(provider):
    respx.post(_url()).mock(
        return_value=httpx.Response(
            200,
            json={
                "candidates": [
                    {"content": {"parts": [{"text": "trun"}]}, "finishReason": "MAX_TOKENS"}
                ],
                "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 100},
            },
        )
    )
    resp = await provider.chat(ChatRequest(messages=[Message.user("long")]))
    assert resp.stop_reason == StopReason.max_tokens


@respx.mock
@pytest.mark.asyncio
async def test_chat_sends_x_goog_api_key_header(provider):
    route = respx.post(_url()).mock(
        return_value=httpx.Response(
            200,
            json={
                "candidates": [
                    {"content": {"parts": [{"text": "hi"}]}, "finishReason": "STOP"}
                ],
                "usageMetadata": {},
            },
        )
    )
    await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    assert route.calls[0].request.headers["x-goog-api-key"] == "test-key"
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_gemini_response.py -v`
Expected: FAIL — `chat()` raises `NotImplementedError`.

- [ ] **Step 3: Implement `chat()` and `_parse_response()`**

Add imports at the top:

```python
from motosan_ai.error import AuthError, NetworkError, ProviderError, RateLimitError
from motosan_ai.types import (
    ChatRequest,
    ChatResponse,
    Message,
    StopReason,
    StreamEvent,
    ToolCall,
    Usage,
)
```

Replace the `chat` stub:

```python
async def chat(self, request: ChatRequest) -> ChatResponse:
    self.validate_request(request)
    body = self._build_body(request)
    try:
        resp = await self._http.post(
            self._generate_url(request),
            headers=self._headers(),
            json=body,
        )
    except httpx.HTTPError as exc:
        raise NetworkError(str(exc)) from exc

    if not resp.is_success:
        raise self._map_http_error(resp.status_code, resp.text)

    payload = resp.json()
    return self._parse_response(payload, request)


@staticmethod
def _map_http_error(status: int, message: str) -> Exception:
    if status == 401:
        return AuthError(message)
    if status == 429:
        return RateLimitError(message)
    return ProviderError(message)


def _parse_response(
    self, payload: dict[str, Any], request: ChatRequest
) -> ChatResponse:
    candidates = payload.get("candidates") or []
    candidate = candidates[0] if candidates else {}
    parts = (candidate.get("content") or {}).get("parts") or []

    text = ""
    tool_calls: list[ToolCall] = []
    for part in parts:
        if "text" in part and part["text"]:
            text += part["text"]
        fc = part.get("functionCall")
        if fc:
            tool_calls.append(
                ToolCall(
                    id=_gen_tool_call_id(),
                    name=fc.get("name", ""),
                    input=fc.get("args") or {},
                )
            )

    finish_reason = candidate.get("finishReason", "STOP")
    if finish_reason == "MAX_TOKENS":
        stop_reason = StopReason.max_tokens
    elif tool_calls:
        stop_reason = StopReason.tool_use
    elif finish_reason == "STOP":
        stop_reason = StopReason.end_turn
    else:
        stop_reason = StopReason.other

    usage_meta = payload.get("usageMetadata") or {}
    usage = Usage(
        input_tokens=int(usage_meta.get("promptTokenCount", 0)),
        output_tokens=int(usage_meta.get("candidatesTokenCount", 0)),
    )

    model = payload.get("modelVersion") or self._model_for(request)

    return ChatResponse(
        content=text,
        tool_calls=tool_calls,
        model=model,
        usage=usage,
        stop_reason=stop_reason,
    )
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_gemini_response.py -v`
Expected: PASS — 4 tests.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini.py sdks/python/tests/test_gemini_response.py
git commit -m "feat(python,gemini): implement non-streaming chat with response parsing"
```

---

## Task 9: Error mapping + retry integration

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini.py` (already has `_map_http_error`; verify via tests)
- Create: `sdks/python/tests/test_gemini_errors.py`

`Client.chat()` already wraps providers with `with_retry` (see `client.py` retry path). This task confirms error shapes propagate correctly and retryable errors re-raise through the provider.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_gemini_errors.py`:

```python
import httpx
import pytest
import respx

from motosan_ai.error import AuthError, ProviderError, RateLimitError
from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.types import ChatRequest, Message


@pytest.fixture
def provider():
    return GeminiProvider(api_key="test-key")


def _url():
    return (
        "https://generativelanguage.googleapis.com/v1beta"
        "/models/gemini-2.5-flash:generateContent"
    )


@respx.mock
@pytest.mark.asyncio
async def test_401_raises_auth_error(provider):
    respx.post(_url()).mock(
        return_value=httpx.Response(401, json={"error": {"message": "bad key"}})
    )
    with pytest.raises(AuthError):
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))


@respx.mock
@pytest.mark.asyncio
async def test_429_raises_rate_limit(provider):
    respx.post(_url()).mock(
        return_value=httpx.Response(429, json={"error": {"message": "slow down"}})
    )
    with pytest.raises(RateLimitError):
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))


@respx.mock
@pytest.mark.asyncio
async def test_500_raises_provider_error(provider):
    respx.post(_url()).mock(
        return_value=httpx.Response(500, json={"error": {"message": "backend blew up"}})
    )
    with pytest.raises(ProviderError):
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))
```

- [ ] **Step 2: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_gemini_errors.py -v`
Expected: PASS immediately — `_map_http_error` implemented in Task 8 already handles all three.

- [ ] **Step 3: (No code change needed.) Commit the tests**

```bash
git add sdks/python/tests/test_gemini_errors.py
git commit -m "test(python,gemini): verify 401/429/5xx error mapping"
```

---

## Task 10: SSE streaming (`stream()`)

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini.py` (`stream`)
- Create: `sdks/python/tests/test_gemini_stream.py`

Gemini SSE: each `data: {...}` line is a JSON payload with `candidates[0]` containing a partial. A single event can carry text + functionCall + usageMetadata + finishReason simultaneously. Approach: for each event, yield text deltas first, then any tool call (start + args + end in one event since Gemini doesn't chunk `args`), then usage, then terminal `done` with stop_reason if `finishReason` present.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_gemini_stream.py`:

```python
import json

import httpx
import pytest
import respx

from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.types import ChatRequest, Message, StopReason


@pytest.fixture
def provider():
    return GeminiProvider(api_key="test-key")


def _stream_url():
    return (
        "https://generativelanguage.googleapis.com/v1beta"
        "/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
    )


def _sse(*events: dict) -> str:
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


@respx.mock
@pytest.mark.asyncio
async def test_stream_text_deltas(provider):
    sse = _sse(
        {"candidates": [{"content": {"parts": [{"text": "Hel"}]}}]},
        {"candidates": [{"content": {"parts": [{"text": "lo"}]}}]},
        {
            "candidates": [
                {"content": {"parts": [{"text": "!"}]}, "finishReason": "STOP"}
            ],
            "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 3},
        },
    )
    respx.post(_stream_url()).mock(
        return_value=httpx.Response(
            200, text=sse, headers={"content-type": "text/event-stream"}
        )
    )

    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    text_events = [e for e in events if e.event_type == "text" and not e.done]
    assert [e.content for e in text_events] == ["Hel", "lo", "!"]

    done = next(e for e in events if e.done)
    assert done.stop_reason == StopReason.end_turn


@respx.mock
@pytest.mark.asyncio
async def test_stream_function_call_emits_start_args_end(provider):
    sse = _sse(
        {
            "candidates": [
                {
                    "content": {
                        "parts": [
                            {
                                "functionCall": {
                                    "name": "get_weather",
                                    "args": {"city": "Taipei"},
                                }
                            }
                        ]
                    },
                    "finishReason": "STOP",
                }
            ]
        }
    )
    respx.post(_stream_url()).mock(
        return_value=httpx.Response(
            200, text=sse, headers={"content-type": "text/event-stream"}
        )
    )
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("w?")]))]

    starts = [e for e in events if e.event_type == "tool_call_start"]
    args = [e for e in events if e.event_type == "tool_call_args"]
    ends = [e for e in events if e.event_type == "tool_call_end"]

    assert len(starts) == 1
    assert starts[0].tool_call_name == "get_weather"
    assert len(args) == 1
    assert json.loads(args[0].tool_call_args_delta) == {"city": "Taipei"}
    assert len(ends) == 1
    # STOP + tool_call => tool_use
    done = next(e for e in events if e.done)
    assert done.stop_reason == StopReason.tool_use


@respx.mock
@pytest.mark.asyncio
async def test_stream_emits_usage_events(provider):
    sse = _sse(
        {
            "candidates": [{"content": {"parts": [{"text": "hi"}]}, "finishReason": "STOP"}],
            "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 2},
        }
    )
    respx.post(_stream_url()).mock(
        return_value=httpx.Response(
            200, text=sse, headers={"content-type": "text/event-stream"}
        )
    )
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    usage_events = [e for e in events if e.event_type == "usage"]
    assert len(usage_events) == 1
    assert usage_events[0].usage.input_tokens == 5
    assert usage_events[0].usage.output_tokens == 2


@respx.mock
@pytest.mark.asyncio
async def test_stream_ignores_empty_data_and_done_marker(provider):
    sse = "data: \n" + _sse(
        {"candidates": [{"content": {"parts": [{"text": "ok"}]}, "finishReason": "STOP"}]}
    ) + "data: [DONE]\n"
    respx.post(_stream_url()).mock(
        return_value=httpx.Response(
            200, text=sse, headers={"content-type": "text/event-stream"}
        )
    )
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    text_events = [e for e in events if e.event_type == "text" and not e.done]
    assert [e.content for e in text_events] == ["ok"]
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_gemini_stream.py -v`
Expected: FAIL — `stream` raises `NotImplementedError`.

- [ ] **Step 3: Implement `stream()`**

Replace the `stream` stub:

```python
async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
    self.validate_request(request)
    body = self._build_body(request)
    try:
        resp = await self._http.send(
            self._http.build_request(
                "POST",
                self._stream_url(request),
                headers=self._headers(),
                json=body,
            ),
            stream=True,
        )
    except httpx.HTTPError as exc:
        raise NetworkError(str(exc)) from exc

    if not resp.is_success:
        error_body = await resp.aread()
        raise self._map_http_error(resp.status_code, error_body.decode())

    async for line in resp.aiter_lines():
        if not line.startswith("data: "):
            continue
        data = line[len("data: ") :].strip()
        if not data or data == "[DONE]":
            continue
        import json as _json
        try:
            payload = _json.loads(data)
        except _json.JSONDecodeError:
            continue

        candidates = payload.get("candidates") or []
        if not candidates:
            continue
        candidate = candidates[0]
        parts = (candidate.get("content") or {}).get("parts") or []
        finish_reason = candidate.get("finishReason")

        has_tool_calls = False
        for part in parts:
            text = part.get("text")
            if text:
                yield StreamEvent(content=text, done=False)
            fc = part.get("functionCall")
            if fc:
                has_tool_calls = True
                call_id = _gen_tool_call_id()
                name = fc.get("name", "")
                args = fc.get("args") or {}
                yield StreamEvent(
                    content="",
                    done=False,
                    tool_call_id=call_id,
                    tool_call_name=name,
                    event_type="tool_call_start",
                )
                yield StreamEvent(
                    content="",
                    done=False,
                    tool_call_id=call_id,
                    tool_call_args_delta=_json.dumps(args),
                    event_type="tool_call_args",
                )
                yield StreamEvent(
                    content="",
                    done=False,
                    tool_call_id=call_id,
                    event_type="tool_call_end",
                )

        usage_meta = payload.get("usageMetadata")
        if usage_meta:
            yield StreamEvent(
                content="",
                done=False,
                event_type="usage",
                usage=Usage(
                    input_tokens=int(usage_meta.get("promptTokenCount", 0)),
                    output_tokens=int(usage_meta.get("candidatesTokenCount", 0)),
                ),
            )

        if finish_reason:
            if has_tool_calls:
                stop_reason = StopReason.tool_use
            elif finish_reason == "MAX_TOKENS":
                stop_reason = StopReason.max_tokens
            elif finish_reason == "STOP":
                stop_reason = StopReason.end_turn
            else:
                stop_reason = StopReason.other
            yield StreamEvent(content="", done=True, stop_reason=stop_reason)
            return
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_gemini_stream.py tests/test_gemini_response.py tests/test_gemini_request.py -v`
Expected: PASS on all Gemini tests.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini.py sdks/python/tests/test_gemini_stream.py
git commit -m "feat(python,gemini): implement SSE streaming with text/tool/usage events"
```

---

## Task 11: `Client` dispatch registration + env var

**Files:**
- Modify: `sdks/python/motosan_ai/client.py`
- Modify: `sdks/python/motosan_ai/providers/__init__.py`
- Modify: `sdks/python/motosan_ai/__init__.py`
- Create: `sdks/python/tests/test_gemini_client_dispatch.py`

Add `Provider.gemini`, env-var lookup (`GEMINI_API_KEY`), provider construction branch, and classmethod `Client.gemini()`.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_gemini_client_dispatch.py`:

```python
import os

import httpx
import pytest
import respx

from motosan_ai import Client, Provider
from motosan_ai.error import ConfigError
from motosan_ai.providers.gemini import GeminiProvider


def test_provider_enum_has_gemini():
    assert Provider.gemini == "gemini"


def test_client_gemini_classmethod(monkeypatch):
    client = Client.gemini(api_key="key", model="gemini-2.5-flash")
    assert client.provider == Provider.gemini
    assert isinstance(client._provider, GeminiProvider)


def test_client_loads_gemini_api_key_from_env(monkeypatch):
    monkeypatch.setenv("GEMINI_API_KEY", "env-key")
    client = Client(provider=Provider.gemini)
    assert client.api_key == "env-key"


def test_client_raises_config_error_when_no_key(monkeypatch):
    monkeypatch.delenv("GEMINI_API_KEY", raising=False)
    with pytest.raises(ConfigError):
        Client(provider=Provider.gemini)


@respx.mock
@pytest.mark.asyncio
async def test_client_chat_dispatches_to_gemini(monkeypatch):
    monkeypatch.setenv("GEMINI_API_KEY", "k")
    url = (
        "https://generativelanguage.googleapis.com/v1beta"
        "/models/gemini-2.5-flash:generateContent"
    )
    respx.post(url).mock(
        return_value=httpx.Response(
            200,
            json={
                "candidates": [
                    {"content": {"parts": [{"text": "ok"}]}, "finishReason": "STOP"}
                ],
                "usageMetadata": {},
            },
        )
    )
    client = Client(provider=Provider.gemini)
    resp = await client.chat([{"role": "user", "content": "hi"}])
    assert resp.content == "ok"
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_gemini_client_dispatch.py -v`
Expected: FAIL — `Provider` enum lacks `gemini`.

- [ ] **Step 3: Wire into `Client`**

Edit `sdks/python/motosan_ai/client.py`:

```python
class Provider(StrEnum):
    anthropic = "anthropic"
    openai = "openai"
    minimax = "minimax"
    ollama = "ollama"
    gemini = "gemini"
```

Update the import line:

```python
from motosan_ai.providers import (
    AnthropicProvider,
    GeminiProvider,
    MinimaxProvider,
    OpenAIProvider,
)
```

Extend `_load_api_key`:

```python
env_map = {
    Provider.anthropic: "ANTHROPIC_API_KEY",
    Provider.openai: "OPENAI_API_KEY",
    Provider.minimax: "MINIMAX_API_KEY",
    Provider.gemini: "GEMINI_API_KEY",
}
```

Extend the `__init__` dispatch (the else branch that handles Anthropic/OpenAI/MiniMax). Add a Gemini branch:

```python
elif provider_value == Provider.gemini:
    self._provider = GeminiProvider(
        api_key=self.api_key, model=model, base_url=base_url
    )
```

Add a classmethod:

```python
@classmethod
def gemini(
    cls,
    api_key: str | None = None,
    model: str | None = None,
    base_url: str | None = None,
    max_retries: int = 3,
) -> Client:
    return cls(
        provider=Provider.gemini,
        api_key=api_key,
        model=model,
        base_url=base_url,
        max_retries=max_retries,
    )
```

Edit `sdks/python/motosan_ai/providers/__init__.py`:

```python
from .anthropic import AnthropicProvider
from .claude_code import ClaudeCodeClient
from .gemini import GeminiProvider
from .minimax import MinimaxProvider
from .ollama import OllamaProvider
from .openai import OpenAIProvider

__all__ = [
    "AnthropicProvider",
    "ClaudeCodeClient",
    "GeminiProvider",
    "MinimaxProvider",
    "OllamaProvider",
    "OpenAIProvider",
]
```

Edit `sdks/python/motosan_ai/__init__.py` — add `GeminiProvider` to both the import block and `__all__`:

```python
from motosan_ai.providers import ClaudeCodeClient, GeminiProvider
```

(And add `"GeminiProvider"` to the `__all__` list in alphabetical position.)

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS — 5 dispatch tests + all existing.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/ sdks/python/tests/test_gemini_client_dispatch.py
git commit -m "feat(python,gemini): register Provider.gemini in Client dispatch"
```

---

## Task 12: Live integration tests

**Files:**
- Create: `sdks/python/tests/integration/test_gemini_live.py`

- [ ] **Step 1: Check existing integration test structure**

Run: `ls sdks/python/tests/integration/` and inspect one file for the skip-if-no-env-var pattern.

- [ ] **Step 2: Create live test file**

Create `sdks/python/tests/integration/test_gemini_live.py`:

```python
"""Live integration tests for GeminiProvider.

Requires `GEMINI_API_KEY` environment variable. Skipped otherwise.
"""

import base64
import os

import pytest

from motosan_ai import Client
from motosan_ai.types import ChatRequest, Message, Tool


@pytest.fixture
def client():
    key = os.getenv("GEMINI_API_KEY")
    if not key:
        pytest.skip("GEMINI_API_KEY not set")
    return Client.gemini(api_key=key)


@pytest.mark.asyncio
async def test_live_simple_chat(client):
    resp = await client.chat(
        [Message.user("Say exactly: pong")],
        max_tokens=32,
    )
    assert resp.content
    assert "pong" in resp.content.lower()


@pytest.mark.asyncio
async def test_live_vision(client):
    # Minimal 1x1 transparent PNG
    tiny_png = base64.b64encode(bytes.fromhex(
        "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c489"
        "0000000d49444154789c636000010000050001a5f645400000000049454e44ae42"
        "6082"
    )).decode()
    resp = await client.chat(
        [Message.user_with_image("What do you see? Reply in 5 words.", tiny_png, "image/png")],
        max_tokens=64,
    )
    assert resp.content


@pytest.mark.asyncio
async def test_live_tool_use(client):
    tools = [
        Tool(
            name="get_weather",
            description="Get weather for a city",
            input_schema={
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"],
            },
        )
    ]
    resp = await client.chat(
        [Message.user("What's the weather in Taipei?")],
        tools=tools,
        max_tokens=256,
    )
    # Gemini should either answer directly or invoke the tool
    assert resp.content or resp.tool_calls


@pytest.mark.asyncio
async def test_live_streaming(client):
    chunks = []
    async for ev in client.stream([Message.user("Count from 1 to 5.")], max_tokens=64):
        if ev.event_type == "text" and not ev.done:
            chunks.append(ev.content)
    full = "".join(chunks)
    assert any(str(n) in full for n in range(1, 6))
```

- [ ] **Step 3: Run live tests manually**

Run: `cd sdks/python && GEMINI_API_KEY=... uv run pytest tests/integration/test_gemini_live.py -v`
Expected: PASS (or skip when the env var is unset, which is the default in CI).

- [ ] **Step 4: Commit**

```bash
git add sdks/python/tests/integration/test_gemini_live.py
git commit -m "test(python,gemini): add live integration tests (chat, vision, tools, stream)"
```

---

## Task 13: Release — CHANGELOG + version bump to 0.8.0

**Files:**
- Modify: `sdks/python/CHANGELOG.md`
- Modify: `sdks/python/pyproject.toml`

- [ ] **Step 1: Bump version and add optional dep group**

Edit `sdks/python/pyproject.toml`:

```toml
version = "0.8.0"
```

And in `[project.optional-dependencies]`:

```toml
gemini = ["httpx>=0.27"]
full = ["motosan-ai[anthropic,openai,minimax,ollama,gemini]"]
```

- [ ] **Step 2: Prepend CHANGELOG entry**

Replace the date with the actual release day (YYYY-MM-DD) when cutting the release.

```markdown
## [0.8.0] - YYYY-MM-DD

### Added
- **`GeminiProvider`** — native HTTP client for Google's Generative Language REST API (`generativelanguage.googleapis.com/v1beta`).
  - `Provider.gemini` registered in `Client` dispatch.
  - `Client.gemini(api_key=..., model=..., base_url=...)` classmethod.
  - `GEMINI_API_KEY` env var loaded automatically.
  - Default model: `gemini-2.5-flash`.
  - Full feature coverage: text, vision (base64 + URL), tools (`functionDeclarations`), tool choice (AUTO / ANY / allowedFunctionNames), streaming (`streamGenerateContent?alt=sse`), system prompts (`systemInstruction`), stop sequences (`stopSequences`), usage reporting (`promptTokenCount` / `candidatesTokenCount`).
  - Capabilities: `with_image()` — document blocks raise `InvalidRequestError` before any HTTP call.
  - Tool call IDs are generated client-side (Gemini doesn't assign them). By convention, `Message.tool_result(tool_call_id=<function_name>, ...)` uses the function name as the ID for Gemini round-trips.
- **Live integration tests** (`tests/integration/test_gemini_live.py`): simple chat, vision, tool use, streaming.

### Notes
- Gemini does not support document (PDF) input; calls with `ContentBlock::Document` fail at validation time.
- No cache token accounting on Gemini — `Usage.cache_creation_input_tokens` / `cache_read_input_tokens` always `None`.
- See `docs/superpowers/plans/2026-04-24-python-sdk-catchup-roadmap.md` for the full catch-up roadmap.
```

- [ ] **Step 3: Run the full gate**

Run: `cd /Users/daiwanwei/Projects/wade/motosan-ai && check-python`
Expected: ruff + format + pytest all pass.

- [ ] **Step 4: Commit**

```bash
git add sdks/python/pyproject.toml sdks/python/CHANGELOG.md
git commit -m "chore(python): release v0.8.0 — Gemini HTTP provider"
```

---

## Final Self-Review Checklist

Before declaring Phase 2b done, verify:

- [ ] `cd sdks/python && uv run pytest tests/ -v` — all tests pass (target: 240+ passing).
- [ ] `cd /Users/daiwanwei/Projects/wade/motosan-ai && check-python` — full gate passes.
- [ ] Live tests green with `GEMINI_API_KEY` set: simple chat, vision, tool use, streaming.
- [ ] Wire format matches Rust byte-for-byte: run `cargo test -p motosan-ai --test gemini_test` + compare emitted JSON against Python body dump.
- [ ] `Provider.gemini` appears in the top-level `__all__` and `Client(provider="gemini")` resolves to `GeminiProvider`.
- [ ] `GEMINI_API_KEY` env var loaded; `ConfigError` raised when missing.
- [ ] DocumentBlock rejected at `validate_request()` — no HTTP call made.
- [ ] Version in `pyproject.toml` is `0.8.0` and `CHANGELOG.md` has a matching entry.
- [ ] `gemini` optional dep extra added; `full` extra updated to include `gemini`.
- [ ] No `TODO` / `FIXME` / placeholder strings introduced.

If any box is unchecked, fix before tagging/publishing.

---

## What Phase 2b does NOT do

- ❌ `Gemini Code Assist` (OAuth + `cloudcode-pa.googleapis.com`) — Phase 3.
- ❌ Gemini CLI subprocess backend — Phase 3.
- ❌ OpenAI / MiniMax / Ollama vision wire-up — deferred. Their `capabilities` are already declared; serialization catches up opportunistically.
- ❌ `Client.chat_with()` / `stream_with()` — Phase 4.
- ❌ `stream_collect()` helpers — Phase 4.
- ❌ Surface `safetyRatings` or `citationMetadata` from Gemini responses — parity with Rust SDK is intentionally narrow; richer metadata pipes through `provider_options` passthrough if needed.
- ❌ Gemini prompt caching — API shape is different enough (explicit cache creation via `cachedContents`) that it warrants its own dedicated plan if/when demand exists.

All non-goals are tracked in the roadmap doc and get their own plans when queued.
