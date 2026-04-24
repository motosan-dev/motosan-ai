# Python SDK Phase 2a — Anthropic Wire-Format Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Light up every Phase 1 type on the wire for Anthropic. Vision, PDF, prompt caching, tool choice, extended thinking, MCP server-side tools, stop sequences, and rich streaming (usage / stop_reason events) all functional against the real API.

**Architecture:** Extend `AnthropicProvider` in place — no new modules. Flip four serialization sites (`_serialize_messages`, `_build_system`, `_build_body` tools section, `_build_body` top-level) to honor the new `ChatRequest` fields. Extend `_parse_response` and the SSE stream loop to surface `thinking`, cache-usage tokens, and typed `stop_reason` on the terminal `done` event. Wire `validate_request()` into `chat()` and `stream()` entry points. OAuth and standard-key paths share a single serializer — avoid Rust's duplication.

**Tech Stack:** Python 3.11+, `httpx`, `respx` for mocks, `pytest-asyncio`.

**Ships as:** `motosan-ai` v0.7.0.

---

## Reference material

- **Rust implementation:** [sdks/rust/src/providers/anthropic.rs](sdks/rust/src/providers/anthropic.rs) — the canonical wire format. Lines 213-409 are the request builder; 482-575 is `_parse_response`; 808-1045 is the SSE adapter.
- **Current Python:** [sdks/python/motosan_ai/providers/anthropic.py](sdks/python/motosan_ai/providers/anthropic.py) — 334 lines. Serialization is in `_serialize_messages` (60-118), `_build_system` (120-132), `_build_body` (134-163), `_parse_response` (229-256), and `stream` (258-333).
- **Anthropic beta identifiers needed:**
  - `mcp-client-2025-11-20` — when `mcp_servers` or `mcp_tool_configs` present.
  - `claude-code-20250219,oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14,interleaved-thinking-2025-05-14` — when OAuth token.
  - Combine with comma — OAuth + MCP → 5 identifiers.

---

## File Structure

| Path | Responsibility | Status |
|------|----------------|--------|
| `sdks/python/motosan_ai/providers/anthropic.py` | All wire-format changes land here | **Modify** (grows ~200 lines) |
| `sdks/python/tests/test_anthropic.py` | Existing smoke tests — must stay green | **Unchanged** |
| `sdks/python/tests/test_anthropic_content_blocks.py` | Vision + PDF serialization over the wire | **Create** |
| `sdks/python/tests/test_anthropic_caching.py` | `Message.cache`, `SystemBlock`, `Tool.cache`, `system_cache`, usage roundtrip | **Create** |
| `sdks/python/tests/test_anthropic_tool_choice.py` | `ToolChoice` serialization per variant | **Create** |
| `sdks/python/tests/test_anthropic_thinking.py` | `ThinkingConfig` body + temperature override + thinking block parsing | **Create** |
| `sdks/python/tests/test_anthropic_mcp.py` | MCP server + tool configs + beta header | **Create** |
| `sdks/python/tests/test_anthropic_stop_sequences.py` | `stop_sequences` in body + `stop_sequence` parsing | **Create** |
| `sdks/python/tests/test_anthropic_stream_usage.py` | StreamEvent.usage from `message_start` / `message_delta` + terminal `stop_reason` | **Create** |
| `sdks/python/tests/test_anthropic_validation.py` | `validate_request()` invoked at entry; capability mismatch raises before HTTP call | **Create** |
| `sdks/python/tests/integration/test_anthropic_live.py` | Add live tests: vision, thinking, caching, MCP | **Modify** (if present) |
| `sdks/python/CHANGELOG.md` | v0.7.0 entry | **Modify** |
| `sdks/python/pyproject.toml` | Version bump 0.6.0 → 0.7.0 | **Modify** |

Design principles:
- **Mock-first.** Every feature gets a `respx`-based unit test that pins the outgoing JSON body byte-equivalent to the Rust wire format. Live tests are supplementary.
- **No OAuth duplication.** Unlike Rust, the Python serializer lives in one path. OAuth wraps with Claude Code prefix + bearer auth; the rest is shared.
- **Fail fast on capability mismatch.** `validate_request()` runs before any HTTP call, returning `InvalidRequestError` with a clear message.
- **No regression.** Existing `test_anthropic.py` tests (tool-use streaming, OAuth handshake, 401/429 mapping) must stay byte-identical in behavior.

---

## Task 1: Wire `validate_request()` into `chat()` and `stream()` entry points

**Files:**
- Modify: `sdks/python/motosan_ai/providers/anthropic.py` (the `chat` and `stream` methods)
- Create: `sdks/python/tests/test_anthropic_validation.py`

Phase 1 added capability declarations but never called `validate_request()`. This task wires the validator in, making every subsequent serialization task safe from malformed requests.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_anthropic_validation.py`:

```python
import pytest

from motosan_ai.error import InvalidRequestError
from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


@pytest.mark.asyncio
async def test_chat_rejects_unsupported_image_before_http(provider):
    # Anthropic supports image, so we force capabilities to text_only to exercise the guard
    from motosan_ai.provider_base import ProviderCapabilities

    provider.capabilities = ProviderCapabilities.text_only()
    req = ChatRequest(messages=[Message.user_with_image("x", "abc", "image/png")])
    with pytest.raises(InvalidRequestError, match="image"):
        await provider.chat(req)


@pytest.mark.asyncio
async def test_stream_rejects_unsupported_document_before_http(provider):
    from motosan_ai.provider_base import ProviderCapabilities

    provider.capabilities = ProviderCapabilities.with_image()  # no document
    req = ChatRequest(messages=[Message.user_with_pdf_base64("x", "abc")])
    with pytest.raises(InvalidRequestError, match="document"):
        async for _ in provider.stream(req):
            pass


@pytest.mark.asyncio
async def test_full_capabilities_accept_image_and_document(provider):
    # Default AnthropicProvider.capabilities == full(); both should pass validation.
    # We don't actually hit the network — respx will intercept, but we only care that
    # validate_request() doesn't raise.
    import httpx
    import respx

    with respx.mock:
        respx.post("https://mock.anthropic.com/v1/messages").mock(
            return_value=httpx.Response(
                200,
                json={
                    "model": "claude-sonnet-4-6",
                    "stop_reason": "end_turn",
                    "usage": {"input_tokens": 1, "output_tokens": 1},
                    "content": [{"type": "text", "text": "ok"}],
                },
            )
        )
        req = ChatRequest(messages=[Message.user_with_image("x", "abc", "image/png")])
        # Should NOT raise
        resp = await provider.chat(req)
        assert resp.content == "ok"
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_anthropic_validation.py -v`
Expected: FAIL — text-only rejection tests receive HTTP calls instead of `InvalidRequestError`.

- [ ] **Step 3: Call `validate_request()` at entry**

Import at the top of `sdks/python/motosan_ai/providers/anthropic.py`:

```python
from motosan_ai.provider_base import BaseProvider, ProviderCapabilities
```

Make `AnthropicProvider` inherit from `BaseProvider` (keeps the existing `capabilities` class attribute valid and exposes `validate_request()`):

```python
class AnthropicProvider(BaseProvider):
    capabilities: ProviderCapabilities = ProviderCapabilities.full()

    def __init__(
        ...
```

At the top of `async def chat(self, request: ChatRequest)` (currently line 173):

```python
async def chat(self, request: ChatRequest) -> ChatResponse:
    self.validate_request(request)
    # ... existing body
```

At the top of `async def stream(self, request: ChatRequest)` (currently line 258):

```python
async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
    self.validate_request(request)
    # ... existing body
```

Since `stream()` is an async generator, the `validate_request()` call executes when the caller first iterates. For true fail-fast behavior, wrap in an outer sync function that returns the generator. Keep the existing shape — the call happens before any HTTP work, which is what the tests verify.

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS — 3 new tests green, all Phase 1 + existing tests still green.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/anthropic.py sdks/python/tests/test_anthropic_validation.py
git commit -m "feat(python,anthropic): wire validate_request into chat/stream entry"
```

---

## Task 2: Serialize `content_blocks` on user messages (vision + PDF)

**Files:**
- Modify: `sdks/python/motosan_ai/providers/anthropic.py` (`_serialize_messages`)
- Create: `sdks/python/tests/test_anthropic_content_blocks.py`

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_anthropic_content_blocks.py`:

```python
import json

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, DocumentBlock, DocumentSourceUrl, Message


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


def _ok_response(text: str = "ok") -> httpx.Response:
    return httpx.Response(
        200,
        json={
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 1},
            "content": [{"type": "text", "text": text}],
        },
    )


@respx.mock
@pytest.mark.asyncio
async def test_user_with_image_serializes_as_blocks(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=_ok_response()
    )
    req = ChatRequest(messages=[Message.user_with_image("describe", "JVBER", "image/png")])
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["messages"] == [
        {
            "role": "user",
            "content": [
                {"type": "text", "text": "describe"},
                {
                    "type": "image",
                    "source": {"type": "base64", "media_type": "image/png", "data": "JVBER"},
                },
            ],
        }
    ]


@respx.mock
@pytest.mark.asyncio
async def test_user_with_pdf_base64_serializes_as_document_block(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=_ok_response()
    )
    req = ChatRequest(messages=[Message.user_with_pdf_base64("summarize", "JVBERi0xLjQK")])
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["messages"][0]["content"][1] == {
        "type": "document",
        "source": {
            "type": "base64",
            "media_type": "application/pdf",
            "data": "JVBERi0xLjQK",
        },
    }


@respx.mock
@pytest.mark.asyncio
async def test_user_with_pdf_url_serializes_as_url_document(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=_ok_response()
    )
    req = ChatRequest(messages=[Message.user_with_pdf_url("x", "https://x.com/d.pdf")])
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["messages"][0]["content"][1] == {
        "type": "document",
        "source": {"type": "url", "url": "https://x.com/d.pdf"},
    }


@respx.mock
@pytest.mark.asyncio
async def test_plain_text_user_message_unchanged_regression(provider):
    """Backward compat — no content_blocks → plain string content, same as before Phase 2a."""
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=_ok_response()
    )
    await provider.chat(ChatRequest(messages=[Message.user("hi")]))

    body = json.loads(route.calls[0].request.content)
    assert body["messages"] == [{"role": "user", "content": "hi"}]
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_anthropic_content_blocks.py -v`
Expected: FAIL — first three tests fail with `content` being `"describe"` / `"summarize"` / `"x"` plain strings instead of blocks. The regression test passes immediately.

- [ ] **Step 3: Extend `_serialize_messages` to honor `content_blocks`**

Add this helper at module scope in `anthropic.py`, near `_CLAUDE_CODE_PREFIX`:

```python
def _serialize_content_block(block: Any) -> dict[str, Any]:
    """Serialize a ContentBlock to Anthropic JSON format."""
    from motosan_ai.types import DocumentBlock, ImageBlock, TextBlock, content_block_to_dict
    return content_block_to_dict(block)
```

Modify the `User` branch of `_serialize_messages` (currently lines 75-82):

```python
if message.role == Role.user:
    if message.content_blocks:
        blocks = [_serialize_content_block(b) for b in message.content_blocks]
        outgoing.append({"role": "user", "content": blocks})
    elif oauth:
        outgoing.append(
            {"role": "user", "content": [{"type": "text", "text": message.content}]}
        )
    else:
        outgoing.append({"role": "user", "content": message.content})
    continue
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS — 4 new content-block tests green, `test_anthropic.py` tool-use/OAuth tests still green.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/anthropic.py sdks/python/tests/test_anthropic_content_blocks.py
git commit -m "feat(python,anthropic): serialize content_blocks on user messages"
```

---

## Task 3: Serialize `Message.cache` flag (prompt caching on user/assistant messages)

**Files:**
- Modify: `sdks/python/motosan_ai/providers/anthropic.py` (`_serialize_messages`)
- Create: `sdks/python/tests/test_anthropic_caching.py`

When `Message.cache is True`, the last content block must carry `cache_control: {"type": "ephemeral"}`. Plain-text messages get wrapped in a single text block so the flag has a home.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_anthropic_caching.py`:

```python
import json

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message, ToolCall


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


def _ok() -> httpx.Response:
    return httpx.Response(
        200,
        json={
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 1},
            "content": [{"type": "text", "text": "ok"}],
        },
    )


@respx.mock
@pytest.mark.asyncio
async def test_cached_plain_user_wraps_in_block_with_cache_control(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    await provider.chat(ChatRequest(messages=[Message.user_with_cache("cache me")]))

    body = json.loads(route.calls[0].request.content)
    assert body["messages"][0] == {
        "role": "user",
        "content": [
            {"type": "text", "text": "cache me", "cache_control": {"type": "ephemeral"}}
        ],
    }


@respx.mock
@pytest.mark.asyncio
async def test_cached_user_with_image_tags_last_block(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    msg = Message.user_with_image("look", "abc", "image/png").with_cache()
    await provider.chat(ChatRequest(messages=[msg]))

    body = json.loads(route.calls[0].request.content)
    blocks = body["messages"][0]["content"]
    assert blocks[0] == {"type": "text", "text": "look"}  # not cached
    assert blocks[1]["cache_control"] == {"type": "ephemeral"}  # image block cached


@respx.mock
@pytest.mark.asyncio
async def test_cached_assistant_with_tool_calls_tags_last_block(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    tc = ToolCall(id="toolu_1", name="x", input={})
    msg = Message.assistant_with_tool_calls("thinking", [tc])
    msg.cache = True
    await provider.chat(ChatRequest(messages=[msg]))

    body = json.loads(route.calls[0].request.content)
    blocks = body["messages"][0]["content"]
    assert blocks[-1]["type"] == "tool_use"
    assert blocks[-1]["cache_control"] == {"type": "ephemeral"}


@respx.mock
@pytest.mark.asyncio
async def test_uncached_message_has_no_cache_control(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    await provider.chat(ChatRequest(messages=[Message.user("plain")]))

    body = json.loads(route.calls[0].request.content)
    # Plain text stays as string — no cache_control anywhere
    assert "cache_control" not in json.dumps(body)
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_anthropic_caching.py -v`
Expected: FAIL — first three tests; the uncached test passes.

- [ ] **Step 3: Extend `_serialize_messages` to emit `cache_control`**

Replace the User and Assistant branches of `_serialize_messages` with cache-aware versions:

```python
if message.role == Role.user:
    if message.content_blocks:
        blocks = [_serialize_content_block(b) for b in message.content_blocks]
        if message.cache and blocks:
            blocks[-1] = {**blocks[-1], "cache_control": {"type": "ephemeral"}}
        outgoing.append({"role": "user", "content": blocks})
    elif message.cache:
        outgoing.append(
            {
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": message.content,
                        "cache_control": {"type": "ephemeral"},
                    }
                ],
            }
        )
    elif oauth:
        outgoing.append(
            {"role": "user", "content": [{"type": "text", "text": message.content}]}
        )
    else:
        outgoing.append({"role": "user", "content": message.content})
    continue

if message.role == Role.assistant:
    if message.tool_calls:
        blocks: list[dict[str, Any]] = []
        if message.content:
            blocks.append({"type": "text", "text": message.content})
        for tc in message.tool_calls:
            blocks.append(
                {"type": "tool_use", "id": tc.id, "name": tc.name, "input": tc.input}
            )
        if message.cache and blocks:
            blocks[-1] = {**blocks[-1], "cache_control": {"type": "ephemeral"}}
        outgoing.append({"role": "assistant", "content": blocks})
    elif message.cache:
        outgoing.append(
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": message.content,
                        "cache_control": {"type": "ephemeral"},
                    }
                ],
            }
        )
    else:
        outgoing.append({"role": "assistant", "content": message.content})
    continue
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS — 4 new tests green, no regressions.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/anthropic.py sdks/python/tests/test_anthropic_caching.py
git commit -m "feat(python,anthropic): serialize Message.cache as cache_control on last block"
```

---

## Task 4: Serialize `SystemBlock[]` and honor `system_cache` flag

**Files:**
- Modify: `sdks/python/motosan_ai/providers/anthropic.py` (`_build_system` and/or `_build_body`)
- Modify: `sdks/python/tests/test_anthropic_caching.py`

Priority: `ChatRequest.system_blocks` > `ChatRequest.system` + `system_cache` > plain `ChatRequest.system` > extracted from messages. OAuth path wraps everything inside the Claude Code prefix array (already present).

- [ ] **Step 1: Append failing tests**

Append to `sdks/python/tests/test_anthropic_caching.py`:

```python
from motosan_ai.types import SystemBlock


@respx.mock
@pytest.mark.asyncio
async def test_system_blocks_serialized_as_array(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        system_blocks=[SystemBlock.cached("Base"), SystemBlock.new("Dynamic")],
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["system"] == [
        {"type": "text", "text": "Base", "cache_control": {"type": "ephemeral"}},
        {"type": "text", "text": "Dynamic"},
    ]


@respx.mock
@pytest.mark.asyncio
async def test_system_cache_wraps_plain_string(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        system="You are helpful.",
        system_cache=True,
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["system"] == [
        {
            "type": "text",
            "text": "You are helpful.",
            "cache_control": {"type": "ephemeral"},
        }
    ]


@respx.mock
@pytest.mark.asyncio
async def test_plain_system_unchanged_regression(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(messages=[Message.user("hi")], system="You are helpful.")
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["system"] == "You are helpful."


@respx.mock
@pytest.mark.asyncio
async def test_system_blocks_take_priority_over_plain_system(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        system="IGNORED",
        system_blocks=[SystemBlock.new("WINS")],
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["system"] == [{"type": "text", "text": "WINS"}]
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_anthropic_caching.py -v -k system`
Expected: FAIL on `system_blocks`, `system_cache`, and priority tests.

- [ ] **Step 3: Extend `_build_system` / `_build_body`**

Replace `_build_system` and the `system` wiring in `_build_body` with a unified path. The cleanest structure is to have `_build_body` compose `system` directly, since it already holds the full request context. Remove the current `_build_system` method and inline the logic:

```python
def _build_body(self, request: ChatRequest, *, stream: bool = False) -> dict[str, Any]:
    messages, extracted_system = self._serialize_messages(
        request.messages,
        oauth=self._is_oauth,
    )

    body: dict[str, Any] = {
        "model": request.model or self.model,
        "messages": messages,
        "max_tokens": request.max_tokens or 4096,
    }
    if stream:
        body["stream"] = True

    # Priority: system_blocks > system + system_cache > plain system > extracted
    plain_system = request.system or extracted_system
    if self._is_oauth:
        # OAuth: always wrap in array with Claude Code prefix.
        oauth_blocks: list[dict[str, Any]] = [
            {
                "type": "text",
                "text": _CLAUDE_CODE_PREFIX,
                "cache_control": {"type": "ephemeral"},
            },
        ]
        if request.system_blocks:
            for b in request.system_blocks:
                obj: dict[str, Any] = {"type": "text", "text": b.text}
                if b.cache_control:
                    obj["cache_control"] = {"type": "ephemeral"}
                oauth_blocks.append(obj)
        elif plain_system:
            obj = {"type": "text", "text": plain_system}
            if request.system_cache:
                obj["cache_control"] = {"type": "ephemeral"}
            oauth_blocks.append(obj)
        body["system"] = oauth_blocks
    elif request.system_blocks:
        body["system"] = [
            (
                {"type": "text", "text": b.text, "cache_control": {"type": "ephemeral"}}
                if b.cache_control
                else {"type": "text", "text": b.text}
            )
            for b in request.system_blocks
        ]
    elif plain_system and request.system_cache:
        body["system"] = [
            {
                "type": "text",
                "text": plain_system,
                "cache_control": {"type": "ephemeral"},
            }
        ]
    elif plain_system:
        body["system"] = plain_system

    if request.temperature is not None:
        body["temperature"] = request.temperature
    if request.tools:
        body["tools"] = [
            {
                "name": t.name,
                "description": t.description or "",
                "input_schema": t.input_schema or {"type": "object", "properties": {}},
            }
            for t in request.tools
        ]
    if request.provider_options:
        body.update(request.provider_options)
    return body
```

Delete the now-unused `_build_system` method.

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS — system-block tests green, existing OAuth tests (which rely on the Claude Code prefix wrapping) still green.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/anthropic.py sdks/python/tests/test_anthropic_caching.py
git commit -m "feat(python,anthropic): serialize system_blocks and system_cache"
```

---

## Task 5: Serialize `Tool.cache` flag

**Files:**
- Modify: `sdks/python/motosan_ai/providers/anthropic.py` (`_build_body` tools section)
- Modify: `sdks/python/tests/test_anthropic_caching.py`

Per Anthropic API: `cache_control` on a tool definition covers that tool + any subsequent tool in the array. Convention: mark only the last tool (ChatRequestBuilder.tools_cached from Phase 1 does this). This task just honors the flag — doesn't impose last-only.

- [ ] **Step 1: Append failing test**

```python
from motosan_ai.types import Tool


@respx.mock
@pytest.mark.asyncio
async def test_tool_cache_flag_emits_cache_control(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    tools = [
        Tool(name="a", description="A"),
        Tool(name="b", description="B", cache=True),
    ]
    req = ChatRequest(messages=[Message.user("hi")], tools=tools)
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert "cache_control" not in body["tools"][0]
    assert body["tools"][1]["cache_control"] == {"type": "ephemeral"}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_anthropic_caching.py::test_tool_cache_flag_emits_cache_control -v`
Expected: FAIL — `cache_control` missing from body.

- [ ] **Step 3: Extend tools-section serialization**

In `_build_body`, replace the `if request.tools:` block:

```python
if request.tools:
    tool_blocks: list[dict[str, Any]] = []
    for t in request.tools:
        obj: dict[str, Any] = {
            "name": t.name,
            "description": t.description or "",
            "input_schema": t.input_schema or {"type": "object", "properties": {}},
        }
        if t.cache:
            obj["cache_control"] = {"type": "ephemeral"}
        tool_blocks.append(obj)
    body["tools"] = tool_blocks
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/anthropic.py sdks/python/tests/test_anthropic_caching.py
git commit -m "feat(python,anthropic): serialize Tool.cache as cache_control"
```

---

## Task 6: Serialize `ToolChoice`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/anthropic.py` (`_build_body`)
- Create: `sdks/python/tests/test_anthropic_tool_choice.py`

Mapping (per Rust reference, lines 359-375):
- `ToolChoice.auto()` → `{"type": "auto"}`
- `ToolChoice.required()` → `{"type": "any"}` (Anthropic's name for required)
- `ToolChoice.none()` → remove the `tools` field entirely (Anthropic has no `none`)
- `ToolChoice.tool("x")` → `{"type": "tool", "name": "x"}`

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_anthropic_tool_choice.py`:

```python
import json

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message, Tool, ToolChoice


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


def _ok() -> httpx.Response:
    return httpx.Response(
        200,
        json={
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 1},
            "content": [{"type": "text", "text": "ok"}],
        },
    )


@respx.mock
@pytest.mark.asyncio
async def test_tool_choice_auto_serializes_auto(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        tools=[Tool(name="x")],
        tool_choice=ToolChoice.auto(),
    )
    await provider.chat(req)
    body = json.loads(route.calls[0].request.content)
    assert body["tool_choice"] == {"type": "auto"}


@respx.mock
@pytest.mark.asyncio
async def test_tool_choice_required_serializes_any(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        tools=[Tool(name="x")],
        tool_choice=ToolChoice.required(),
    )
    await provider.chat(req)
    body = json.loads(route.calls[0].request.content)
    assert body["tool_choice"] == {"type": "any"}


@respx.mock
@pytest.mark.asyncio
async def test_tool_choice_none_removes_tools(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        tools=[Tool(name="x")],
        tool_choice=ToolChoice.none(),
    )
    await provider.chat(req)
    body = json.loads(route.calls[0].request.content)
    assert "tools" not in body
    assert "tool_choice" not in body


@respx.mock
@pytest.mark.asyncio
async def test_tool_choice_tool_name(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        tools=[Tool(name="get_weather")],
        tool_choice=ToolChoice.tool("get_weather"),
    )
    await provider.chat(req)
    body = json.loads(route.calls[0].request.content)
    assert body["tool_choice"] == {"type": "tool", "name": "get_weather"}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_anthropic_tool_choice.py -v`
Expected: FAIL — `tool_choice` missing from body.

- [ ] **Step 3: Extend `_build_body`**

Add after the tools section in `_build_body`:

```python
if request.tool_choice is not None:
    tc = request.tool_choice
    if tc.type == "auto":
        body["tool_choice"] = {"type": "auto"}
    elif tc.type == "required":
        body["tool_choice"] = {"type": "any"}
    elif tc.type == "none":
        body.pop("tools", None)
    elif tc.type == "tool":
        body["tool_choice"] = {"type": "tool", "name": tc.name}
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/anthropic.py sdks/python/tests/test_anthropic_tool_choice.py
git commit -m "feat(python,anthropic): serialize ToolChoice to Anthropic format"
```

---

## Task 7: Serialize `ThinkingConfig` + force `temperature=1.0`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/anthropic.py` (`_build_body`, `_parse_response`)
- Create: `sdks/python/tests/test_anthropic_thinking.py`

When `request.thinking` is set:
- Add `thinking: {"type": "enabled", "budget_tokens": N}` to body.
- Override `temperature` to `1.0` (Anthropic API constraint).
- Non-stream response: join all `thinking`-type content blocks into `ChatResponse.thinking`.
- Stream: `thinking_delta` events currently fall through as text delta — acceptable for now; a richer stream-level distinction can land in a future release.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_anthropic_thinking.py`:

```python
import json

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message, ThinkingConfig


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


def _thinking_response() -> httpx.Response:
    return httpx.Response(
        200,
        json={
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 20},
            "content": [
                {"type": "thinking", "thinking": "Let me reason about this..."},
                {"type": "text", "text": "Answer: 42"},
            ],
        },
    )


@respx.mock
@pytest.mark.asyncio
async def test_thinking_serialized_in_body(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=_thinking_response()
    )
    req = ChatRequest(
        messages=[Message.user("solve")],
        thinking=ThinkingConfig(budget_tokens=4096),
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["thinking"] == {"type": "enabled", "budget_tokens": 4096}


@respx.mock
@pytest.mark.asyncio
async def test_thinking_forces_temperature_to_one(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=_thinking_response()
    )
    req = ChatRequest(
        messages=[Message.user("solve")],
        temperature=0.2,  # should be overridden
        thinking=ThinkingConfig(budget_tokens=1024),
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["temperature"] == 1.0


@respx.mock
@pytest.mark.asyncio
async def test_thinking_blocks_parsed_into_response(provider):
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=_thinking_response()
    )
    req = ChatRequest(
        messages=[Message.user("solve")],
        thinking=ThinkingConfig(budget_tokens=1024),
    )
    resp = await provider.chat(req)

    assert resp.thinking == "Let me reason about this..."
    assert resp.content == "Answer: 42"
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_anthropic_thinking.py -v`
Expected: FAIL on all three.

- [ ] **Step 3: Extend `_build_body` and `_parse_response`**

In `_build_body`, find the temperature section and replace:

```python
# Temperature — thinking forces 1.0
if request.thinking is not None:
    body["temperature"] = 1.0
elif request.temperature is not None:
    body["temperature"] = request.temperature

if request.thinking is not None:
    body["thinking"] = {
        "type": "enabled",
        "budget_tokens": request.thinking.budget_tokens,
    }
```

In `_parse_response`, replace the `text = "".join(...)` block and extend:

```python
content_blocks = payload.get("content", [])
text = "".join(
    block.get("text", "")
    for block in content_blocks
    if block.get("type") == "text"
)
thinking_parts = [
    block.get("thinking", "")
    for block in content_blocks
    if block.get("type") == "thinking"
]
thinking = "".join(thinking_parts) if thinking_parts else None
# ... existing tool_calls / stop_reason / usage logic ...
return ChatResponse(
    content=text,
    thinking=thinking,
    tool_calls=tool_calls,
    model=payload.get("model", self.model),
    usage=Usage(int(usage.get("input_tokens", 0)), int(usage.get("output_tokens", 0))),
    stop_reason=stop_reason_map.get(payload.get("stop_reason"), StopReason.other),
)
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/anthropic.py sdks/python/tests/test_anthropic_thinking.py
git commit -m "feat(python,anthropic): serialize thinking config and parse thinking blocks"
```

---

## Task 8: Serialize `stop_sequences` and parse `stop_sequence` stop reason

**Files:**
- Modify: `sdks/python/motosan_ai/providers/anthropic.py` (`_build_body`, `_parse_response`)
- Create: `sdks/python/tests/test_anthropic_stop_sequences.py`

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_anthropic_stop_sequences.py`:

```python
import json

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message, StopReason


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


@respx.mock
@pytest.mark.asyncio
async def test_stop_sequences_serialized(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200,
            json={
                "model": "claude-sonnet-4-6",
                "stop_reason": "stop_sequence",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "content": [{"type": "text", "text": "hi"}],
            },
        )
    )
    req = ChatRequest(
        messages=[Message.user("hi")],
        stop_sequences=["END", "STOP"],
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["stop_sequences"] == ["END", "STOP"]


@respx.mock
@pytest.mark.asyncio
async def test_stop_sequence_reason_parsed(provider):
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200,
            json={
                "model": "claude-sonnet-4-6",
                "stop_reason": "stop_sequence",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "content": [{"type": "text", "text": "hi"}],
            },
        )
    )
    resp = await provider.chat(
        ChatRequest(messages=[Message.user("hi")], stop_sequences=["END"])
    )
    assert resp.stop_reason == StopReason.stop_sequence


@respx.mock
@pytest.mark.asyncio
async def test_empty_stop_sequences_omitted(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200,
            json={
                "model": "claude-sonnet-4-6",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "content": [{"type": "text", "text": "hi"}],
            },
        )
    )
    await provider.chat(
        ChatRequest(messages=[Message.user("hi")], stop_sequences=[])
    )
    body = json.loads(route.calls[0].request.content)
    assert "stop_sequences" not in body
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_anthropic_stop_sequences.py -v`
Expected: FAIL — field missing from body; stop reason parsed as `other`.

- [ ] **Step 3: Extend `_build_body` + `_parse_response`**

Add in `_build_body` (after `thinking`):

```python
if request.stop_sequences:
    body["stop_sequences"] = list(request.stop_sequences)
```

Update `stop_reason_map` in `_parse_response`:

```python
stop_reason_map = {
    "end_turn": StopReason.end_turn,
    "max_tokens": StopReason.max_tokens,
    "tool_use": StopReason.tool_use,
    "stop": StopReason.stop,
    "stop_sequence": StopReason.stop_sequence,
}
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/anthropic.py sdks/python/tests/test_anthropic_stop_sequences.py
git commit -m "feat(python,anthropic): serialize stop_sequences and parse stop_sequence reason"
```

---

## Task 9: Serialize MCP servers + tool configs + beta header

**Files:**
- Modify: `sdks/python/motosan_ai/providers/anthropic.py` (`_headers`, `_build_body`)
- Create: `sdks/python/tests/test_anthropic_mcp.py`

MCP adds two body fields and one beta header identifier:
- `body["mcp_servers"] = [{type, url, name, authorization_token?}, ...]`
- Each `McpToolConfig` serializes as `{type: "mcp_toolset", mcp_server_name, allowed_tools?, denied_tools?}` and gets pushed into the `tools` array.
- Header `anthropic-beta` gains `mcp-client-2025-11-20`.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_anthropic_mcp.py`:

```python
import json

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import (
    ChatRequest,
    McpServerConfig,
    McpToolConfigAllowed,
    Message,
)


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


def _ok() -> httpx.Response:
    return httpx.Response(
        200,
        json={
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 1},
            "content": [{"type": "text", "text": "ok"}],
        },
    )


@respx.mock
@pytest.mark.asyncio
async def test_mcp_server_config_serialized(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        mcp_servers=[McpServerConfig(url="https://mcp.x.com", name="srv")],
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["mcp_servers"] == [
        {"type": "url", "url": "https://mcp.x.com", "name": "srv"}
    ]


@respx.mock
@pytest.mark.asyncio
async def test_mcp_tool_config_appended_to_tools_array(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        mcp_servers=[McpServerConfig(url="https://mcp.x.com", name="srv")],
        mcp_tool_configs=[
            McpToolConfigAllowed(mcp_server_name="srv", allowed_tools=["read"])
        ],
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["tools"] == [
        {
            "type": "mcp_toolset",
            "mcp_server_name": "srv",
            "allowed_tools": ["read"],
        }
    ]


@respx.mock
@pytest.mark.asyncio
async def test_mcp_beta_header_attached(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        mcp_servers=[McpServerConfig(url="https://mcp.x.com", name="srv")],
    )
    await provider.chat(req)
    assert "mcp-client-2025-11-20" in route.calls[0].request.headers["anthropic-beta"]


@respx.mock
@pytest.mark.asyncio
async def test_mcp_with_auth_token(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    req = ChatRequest(
        messages=[Message.user("hi")],
        mcp_servers=[
            McpServerConfig(
                url="https://mcp.x.com",
                name="srv",
                authorization_token="Bearer x",
            )
        ],
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["mcp_servers"][0]["authorization_token"] == "Bearer x"


@respx.mock
@pytest.mark.asyncio
async def test_no_mcp_no_beta_header_added(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok())
    await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    assert "mcp-client-2025-11-20" not in route.calls[0].request.headers.get(
        "anthropic-beta", ""
    )
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_anthropic_mcp.py -v`
Expected: FAIL on everything except the negative test.

- [ ] **Step 3: Extend `_headers` and `_build_body`**

Add a helper at module scope:

```python
def _mcp_tool_config_to_dict(cfg: Any) -> dict[str, Any]:
    from motosan_ai.types import (
        McpToolConfigAll,
        McpToolConfigAllowed,
        McpToolConfigDenied,
    )
    base: dict[str, Any] = {"type": "mcp_toolset", "mcp_server_name": cfg.mcp_server_name}
    if isinstance(cfg, McpToolConfigAllowed):
        base["allowed_tools"] = list(cfg.allowed_tools)
    elif isinstance(cfg, McpToolConfigDenied):
        base["denied_tools"] = list(cfg.denied_tools)
    return base


def _mcp_server_to_dict(srv: Any) -> dict[str, Any]:
    out: dict[str, Any] = {"type": srv.type, "url": srv.url, "name": srv.name}
    if srv.authorization_token is not None:
        out["authorization_token"] = srv.authorization_token
    return out
```

Modify `_headers` to accept an optional `has_mcp` flag:

```python
def _headers(self, *, has_mcp: bool = False) -> dict[str, str]:
    headers: dict[str, str] = {
        "anthropic-version": _ANTHROPIC_VERSION,
        "content-type": "application/json",
    }
    betas: list[str] = []
    if self._is_oauth:
        betas.extend(
            [
                "claude-code-20250219",
                "oauth-2025-04-20",
                "fine-grained-tool-streaming-2025-05-14",
                "interleaved-thinking-2025-05-14",
            ]
        )
        headers["authorization"] = f"Bearer {self.api_key}"
        headers["user-agent"] = "claude-code/1.0.33"
        headers["x-app"] = "cli"
    else:
        headers["x-api-key"] = self.api_key
    if has_mcp:
        betas.append("mcp-client-2025-11-20")
    if betas:
        headers["anthropic-beta"] = ",".join(betas)
    return headers
```

In `_build_body`, extend the tools section so MCP tool configs append to the same array:

```python
tool_blocks: list[dict[str, Any]] = []
if request.tools:
    for t in request.tools:
        obj: dict[str, Any] = {
            "name": t.name,
            "description": t.description or "",
            "input_schema": t.input_schema or {"type": "object", "properties": {}},
        }
        if t.cache:
            obj["cache_control"] = {"type": "ephemeral"}
        tool_blocks.append(obj)
if request.mcp_tool_configs:
    for cfg in request.mcp_tool_configs:
        tool_blocks.append(_mcp_tool_config_to_dict(cfg))
if tool_blocks:
    body["tools"] = tool_blocks
```

Add at the end of `_build_body`:

```python
if request.mcp_servers:
    body["mcp_servers"] = [_mcp_server_to_dict(s) for s in request.mcp_servers]
```

At the two call sites where `self._headers()` is invoked (in `chat` and `stream`), pass `has_mcp`:

```python
has_mcp = bool(request.mcp_servers) or bool(request.mcp_tool_configs)
# ... later
resp = await self._http.post(self._endpoint(), headers=self._headers(has_mcp=has_mcp), json=body)
```

Do the same for the streaming request builder.

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/anthropic.py sdks/python/tests/test_anthropic_mcp.py
git commit -m "feat(python,anthropic): serialize mcp_servers + mcp_tool_configs with beta header"
```

---

## Task 10: Parse cache usage tokens from non-stream response

**Files:**
- Modify: `sdks/python/motosan_ai/providers/anthropic.py` (`_parse_response`)
- Modify: `sdks/python/tests/test_anthropic_caching.py`

- [ ] **Step 1: Append failing test**

```python
@respx.mock
@pytest.mark.asyncio
async def test_cache_usage_tokens_parsed(provider):
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200,
            json={
                "model": "claude-sonnet-4-6",
                "stop_reason": "end_turn",
                "usage": {
                    "input_tokens": 100,
                    "output_tokens": 20,
                    "cache_creation_input_tokens": 50,
                    "cache_read_input_tokens": 200,
                },
                "content": [{"type": "text", "text": "ok"}],
            },
        )
    )
    resp = await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    assert resp.usage.input_tokens == 100
    assert resp.usage.output_tokens == 20
    assert resp.usage.cache_creation_input_tokens == 50
    assert resp.usage.cache_read_input_tokens == 200
```

- [ ] **Step 2: Run test to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_anthropic_caching.py::test_cache_usage_tokens_parsed -v`
Expected: FAIL — fields are `None`.

- [ ] **Step 3: Extend `_parse_response`**

Replace the `Usage` construction in `_parse_response`:

```python
usage_obj = payload.get("usage", {}) or {}
usage = Usage(
    input_tokens=int(usage_obj.get("input_tokens", 0)),
    output_tokens=int(usage_obj.get("output_tokens", 0)),
    cache_creation_input_tokens=usage_obj.get("cache_creation_input_tokens"),
    cache_read_input_tokens=usage_obj.get("cache_read_input_tokens"),
)
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/anthropic.py sdks/python/tests/test_anthropic_caching.py
git commit -m "feat(python,anthropic): parse cache_creation/read_input_tokens from usage"
```

---

## Task 11: Stream — emit `StreamEvent.usage` from `message_start` / `message_delta`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/anthropic.py` (`stream` method SSE loop)
- Create: `sdks/python/tests/test_anthropic_stream_usage.py`

Rust reference: lines 865-936 of `anthropic.rs`. `message_start` carries input usage + cache tokens; `message_delta` carries output tokens. Both become `StreamEvent` with `event_type="usage"` and the `usage` field populated.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_anthropic_stream_usage.py`:

```python
import json

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


def _sse(*events: dict) -> str:
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


@respx.mock
@pytest.mark.asyncio
async def test_stream_emits_usage_from_message_start(provider):
    sse = _sse(
        {
            "type": "message_start",
            "message": {
                "usage": {
                    "input_tokens": 100,
                    "output_tokens": 0,
                    "cache_creation_input_tokens": 50,
                    "cache_read_input_tokens": 200,
                }
            },
        },
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "hi"},
        },
        {"type": "message_stop"},
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200, text=sse, headers={"content-type": "text/event-stream"}
        )
    )

    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]

    usage_events = [e for e in events if e.event_type == "usage"]
    assert len(usage_events) >= 1
    first = usage_events[0]
    assert first.usage is not None
    assert first.usage.input_tokens == 100
    assert first.usage.cache_creation_input_tokens == 50
    assert first.usage.cache_read_input_tokens == 200


@respx.mock
@pytest.mark.asyncio
async def test_stream_emits_usage_from_message_delta(provider):
    sse = _sse(
        {
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {"input_tokens": 0, "output_tokens": 42},
        },
        {"type": "message_stop"},
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200, text=sse, headers={"content-type": "text/event-stream"}
        )
    )
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    usage_events = [e for e in events if e.event_type == "usage"]
    assert any(u.usage.output_tokens == 42 for u in usage_events)
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_anthropic_stream_usage.py -v`
Expected: FAIL — no usage events emitted.

- [ ] **Step 3: Extend SSE event loop**

In `stream()`, add two new branches before `content_block_start`:

```python
if event_type == "message_start":
    usage = (payload.get("message") or {}).get("usage")
    if usage:
        yield StreamEvent(
            content="",
            done=False,
            event_type="usage",
            usage=Usage(
                input_tokens=int(usage.get("input_tokens", 0)),
                output_tokens=int(usage.get("output_tokens", 0)),
                cache_creation_input_tokens=usage.get("cache_creation_input_tokens"),
                cache_read_input_tokens=usage.get("cache_read_input_tokens"),
            ),
        )
    continue

if event_type == "message_delta":
    usage = payload.get("usage")
    if usage:
        yield StreamEvent(
            content="",
            done=False,
            event_type="usage",
            usage=Usage(
                input_tokens=int(usage.get("input_tokens", 0)),
                output_tokens=int(usage.get("output_tokens", 0)),
            ),
        )
    # stop_reason handling lands in Task 12
    continue
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS. Verify existing `test_anthropic_stream_tool_use` still green — it doesn't include `message_start`/`message_delta`, so behavior is unchanged.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/anthropic.py sdks/python/tests/test_anthropic_stream_usage.py
git commit -m "feat(python,anthropic): emit StreamEvent.usage from message_start/delta"
```

---

## Task 12: Stream — capture `stop_reason` from `message_delta`, emit on terminal done

**Files:**
- Modify: `sdks/python/motosan_ai/providers/anthropic.py` (`stream` method)
- Modify: `sdks/python/tests/test_anthropic_stream_usage.py`

Rust reference: lines 897-914 and 997-1008. The adapter holds `current_stop_reason` across events; when `message_stop` fires, the terminal `done` event carries the captured reason.

- [ ] **Step 1: Append failing test**

```python
from motosan_ai.types import StopReason


@respx.mock
@pytest.mark.asyncio
async def test_stream_done_carries_stop_reason(provider):
    sse = _sse(
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "hi"},
        },
        {"type": "message_delta", "delta": {"stop_reason": "stop_sequence"}},
        {"type": "message_stop"},
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200, text=sse, headers={"content-type": "text/event-stream"}
        )
    )
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]

    done = next(e for e in events if e.done)
    assert done.stop_reason == StopReason.stop_sequence


@respx.mock
@pytest.mark.asyncio
async def test_stream_done_without_delta_has_no_stop_reason(provider):
    """Backward compat: existing tests that omit message_delta still work."""
    sse = _sse(
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "hi"},
        },
        {"type": "message_stop"},
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200, text=sse, headers={"content-type": "text/event-stream"}
        )
    )
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    done = next(e for e in events if e.done)
    assert done.stop_reason is None
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_anthropic_stream_usage.py -v -k stop_reason`
Expected: FAIL on the first; the backward-compat test passes.

- [ ] **Step 3: Track stop_reason across events**

Modify `stream()` to use a local `current_stop_reason` variable. At the start of the event loop (just after `current_tool_id: str | None = None`):

```python
current_stop_reason: StopReason | None = None

_stop_reason_map = {
    "end_turn": StopReason.end_turn,
    "max_tokens": StopReason.max_tokens,
    "tool_use": StopReason.tool_use,
    "stop": StopReason.stop,
    "stop_sequence": StopReason.stop_sequence,
}
```

Extend the `message_delta` branch from Task 11 to capture the reason:

```python
if event_type == "message_delta":
    delta = payload.get("delta") or {}
    reason = delta.get("stop_reason")
    if reason:
        current_stop_reason = _stop_reason_map.get(reason, StopReason.other)
    usage = payload.get("usage")
    if usage:
        yield StreamEvent(
            content="",
            done=False,
            event_type="usage",
            usage=Usage(
                input_tokens=int(usage.get("input_tokens", 0)),
                output_tokens=int(usage.get("output_tokens", 0)),
            ),
        )
    continue
```

Replace the `message_stop` branch to carry the captured reason:

```python
elif event_type == "message_stop":
    yield StreamEvent(content="", done=True, stop_reason=current_stop_reason)
    return
```

And update the `[DONE]` branch likewise:

```python
if data == "[DONE]":
    yield StreamEvent(content="", done=True, stop_reason=current_stop_reason)
    return
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS — 2 new tests + all existing stream tests (they don't set stop_reason on done, which is still correct because they don't emit `message_delta`).

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/anthropic.py sdks/python/tests/test_anthropic_stream_usage.py
git commit -m "feat(python,anthropic): surface stop_reason on terminal stream done event"
```

---

## Task 13: Live-test smoke suite (manual-only gate)

**Files:**
- Modify or Create: `sdks/python/tests/integration/test_anthropic_live.py`

Add live tests behind the existing OAuth/API-key gate. These are not run by default `check-python`; they run when `ANTHROPIC_API_KEY` is present (or OAuth token). Existing live-test pattern already exists — extend it.

- [ ] **Step 1: Check existing live test structure**

Run: `cd sdks/python && ls tests/integration/ && cat tests/integration/test_anthropic_live.py | head -40`
Review the gating pattern (typically `pytest.skip` when env var missing).

- [ ] **Step 2: Add new live tests following the existing pattern**

Append to `sdks/python/tests/integration/test_anthropic_live.py` (exact fixture names/skip markers follow the file's existing conventions):

```python
async def test_live_vision(anthropic_client):
    """Requires a real key + live network. Verifies vision roundtrip."""
    import base64

    tiny_png = base64.b64encode(bytes.fromhex(
        "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c489"
        "0000000d49444154789c636000010000050001a5f645400000000049454e44ae42"
        "6082"
    )).decode()
    req = ChatRequest(
        messages=[Message.user_with_image("What color is this?", tiny_png, "image/png")],
        max_tokens=64,
    )
    resp = await anthropic_client.chat(req)
    assert resp.content  # non-empty


async def test_live_thinking(anthropic_client):
    req = ChatRequest(
        messages=[Message.user("What is 13 * 17? Think step by step.")],
        thinking=ThinkingConfig(budget_tokens=1024),
        max_tokens=2048,
        model="claude-sonnet-4-6",
    )
    resp = await anthropic_client.chat(req)
    assert resp.thinking is not None
    assert "221" in resp.content or "221" in (resp.thinking or "")


async def test_live_prompt_caching_reports_cache_tokens(anthropic_client):
    big_system = "You are a helpful assistant.\n" * 200  # force >1024 tokens
    req = ChatRequest(
        messages=[Message.user("Say hi.")],
        system_blocks=[SystemBlock.cached(big_system)],
        max_tokens=32,
    )
    # First call — expect cache write
    first = await anthropic_client.chat(req)
    # Second call — expect cache read
    second = await anthropic_client.chat(req)

    assert (first.usage.cache_creation_input_tokens or 0) > 0
    assert (second.usage.cache_read_input_tokens or 0) > 0
```

- [ ] **Step 3: Run live tests manually**

Run: `cd sdks/python && ANTHROPIC_API_KEY=sk-ant-... uv run pytest tests/integration/test_anthropic_live.py -v`
Expected: PASS (or pre-push gate script handles it). Skip if the env var is absent.

- [ ] **Step 4: Commit**

```bash
git add sdks/python/tests/integration/test_anthropic_live.py
git commit -m "test(python,anthropic): add live vision / thinking / caching tests"
```

---

## Task 14: Release — CHANGELOG + version bump to 0.7.0

**Files:**
- Modify: `sdks/python/CHANGELOG.md`
- Modify: `sdks/python/pyproject.toml`

- [ ] **Step 1: Bump version**

Edit `sdks/python/pyproject.toml`:

```toml
version = "0.7.0"
```

- [ ] **Step 2: Prepend CHANGELOG entry**

At the top of `sdks/python/CHANGELOG.md`, below the title, prepend:

Replace the date with the actual release day (YYYY-MM-DD) when cutting the release.

```markdown
## [0.7.0] - YYYY-MM-DD

### Added — Anthropic wire-format parity with Rust SDK
- **Vision & PDF input** — user messages with `content_blocks` (image or document) now serialize as Anthropic content-block arrays, including `source` base64/URL variants.
- **Prompt caching** — `Message.cache` tags the last content block with `cache_control`; plain-text cached messages are wrapped in a text block. `SystemBlock[]` serializes as a system array with per-block cache control; `system_cache=True` wraps the plain `system` string in a cached array. `Tool.cache` tags tool definitions. `Usage.cache_creation_input_tokens` / `cache_read_input_tokens` parsed from non-stream responses.
- **ToolChoice** — `auto`, `required` (→ Anthropic `any`), `none` (→ removes `tools`), `tool(name)` fully supported.
- **Extended thinking** — `ThinkingConfig(budget_tokens)` serializes as `{"type":"enabled","budget_tokens":N}`; forces `temperature=1.0`. Non-stream responses parse `thinking` blocks into `ChatResponse.thinking`.
- **MCP (Model Context Protocol) server-side tools** — `mcp_servers` + `mcp_tool_configs` serialized; `anthropic-beta: mcp-client-2025-11-20` auto-attached when MCP fields present.
- **Stop sequences** — `stop_sequences: list[str]` serialized; `StopReason.stop_sequence` parsed.
- **Stream enhancements**
  - `StreamEvent.usage` emitted from `message_start` (input + cache tokens) and `message_delta` (output tokens).
  - `StreamEvent.stop_reason` carried on the terminal `done` event (captured from `message_delta.delta.stop_reason`).

### Changed
- `AnthropicProvider` inherits `BaseProvider`; `validate_request()` runs at `chat()` and `stream()` entry to reject unsupported content before any HTTP call.
- `_build_body` refactored: unified system-prompt handling for OAuth + standard-key paths; no behavior change for pre-Phase-2a code.

### Notes
- Only the Anthropic provider gained wire-format features in this release. OpenAI, MiniMax, Ollama, Gemini (HTTP), and CLI backends land in Phase 2b+.
- See `docs/superpowers/plans/2026-04-24-python-sdk-catchup-roadmap.md` for the full catch-up roadmap.
```

- [ ] **Step 3: Run the full gate**

Run: `cd /Users/daiwanwei/Projects/wade/motosan-ai && check-python`
Expected: ruff + format + pytest all pass.

- [ ] **Step 4: Commit**

```bash
git add sdks/python/pyproject.toml sdks/python/CHANGELOG.md
git commit -m "chore(python): release v0.7.0 — Anthropic wire-format parity"
```

---

## Final Self-Review Checklist

Before declaring Phase 2a done, verify:

- [ ] `cd sdks/python && uv run pytest tests/ -v` — all tests pass (target: 200+ passing).
- [ ] `cd /Users/daiwanwei/Projects/wade/motosan-ai && check-python` — full gate passes.
- [ ] Live tests green: vision, thinking, caching cache-token report.
- [ ] Wire format diff'd against Rust: for a matched `ChatRequest`, both SDKs produce byte-equivalent JSON bodies for the new fields (manual compare against `cargo test -p motosan-ai -- anthropic::tests::` output).
- [ ] `anthropic-beta` header identifiers correct: OAuth alone, MCP alone, OAuth+MCP all produce the right comma-joined string.
- [ ] `validate_request()` runs at chat entry + stream entry; capability mismatch raises `InvalidRequestError` before any `httpx` call.
- [ ] No existing mock test in `test_anthropic.py` required modification.
- [ ] Version in `pyproject.toml` is `0.7.0` and `CHANGELOG.md` has a matching entry.
- [ ] No `TODO` / `FIXME` / placeholder strings introduced.

If any box is unchecked, fix before tagging/publishing.

---

## What Phase 2a does NOT do

- ❌ New providers (Gemini HTTP, Gemini Code Assist, Codex CLI, Gemini CLI) — that's Phase 2b / Phase 3.
- ❌ `Client.chat_with()` / `stream_with()` / `stream_collect()` — Phase 4.
- ❌ OpenAI / MiniMax vision or tool_choice parity — Phase 2b.
- ❌ Refactor into a Rust-style `AnthropicRequestBuilder` class — the inline approach in `_build_body` stays readable through Phase 2a; a refactor lands only if Phase 2b pressure demands it.
- ❌ Surface `thinking_delta` as a distinct `StreamEventType` variant — currently flows through the text-delta path, which matches Rust behavior and is sufficient for consumers that call `ChatResponse.thinking` after collecting the stream.

All non-goals are tracked in the roadmap doc and get their own plans when queued.
