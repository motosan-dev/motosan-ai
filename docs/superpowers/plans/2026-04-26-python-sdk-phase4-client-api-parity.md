# Python SDK Phase 4 — Client API Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring `motosan_ai.Client` to method-level parity with Rust's `Client` — add `chat_with(request)` / `stream_with(request)` / `stream_collect(messages)` / `stream_collect_with(request)`, soft-deprecate `chat_sync()`, and refresh the public docs.

**Architecture:** Keep `chat()` / `stream()` as kwargs-friendly conveniences (back-compat). Refactor them to internally build a `ChatRequest` and delegate to the new `*_with` methods, which become the canonical entry points. Add a private `_collect_stream` helper that drives a stream to completion and assembles a `ChatResponse` (text + tool calls + usage + stop_reason). All retry logic stays in the existing `chat()`/`stream()` paths since `*_with` methods are thin passthroughs.

**Tech Stack:** Python 3.11+, `asyncio`, `pytest`, `respx`. No new dependencies.

**Ships as:** `motosan-ai` v0.10.0.

---

## Reference material

- **Rust canon:** [sdks/rust/src/client.rs:85-164](sdks/rust/src/client.rs#L85) — the four methods and their dispatch shape:
  ```
  chat(messages)        → builds builder.messages(messages).model(self.model).build() → chat_with(req)
  chat_with(req)        → dispatch
  stream(messages)      → builds → stream_with(req)
  stream_with(req)      → dispatch + think_stripper
  stream_collect(msgs)  → stream(msgs) → collect_stream() → response
  stream_collect_with(req) → stream_with(req) → collect_stream() → response (model fallback to req.model or self.model)
  ```
- **Current Python client:** [sdks/python/motosan_ai/client.py:284-374](sdks/python/motosan_ai/client.py#L284) — `chat()` and `stream()` build their own ChatRequest from kwargs (`tools`, `system`, `temperature`, `max_tokens`, `provider_options`). They do NOT expose Phase 1's newer fields (`system_blocks`, `tool_choice`, `thinking`, `mcp_servers`, etc.) — that's intentional; callers needing those go through `chat_with(builder.build())`.
- **Builder reference:** [sdks/python/motosan_ai/types.py](sdks/python/motosan_ai/types.py) — `ChatRequest.builder()` returns `ChatRequestBuilder` (Phase 1, v0.6.0).
- **CLAUDE.md rule** (project root): "No sync wrappers in Python — callers use `asyncio.run()`." So `chat_sync()` should be soft-deprecated with a `DeprecationWarning`.

---

## File Structure

| Path | Responsibility | Status |
|------|----------------|--------|
| `sdks/python/motosan_ai/client.py` | Add `chat_with` / `stream_with` / `stream_collect` / `stream_collect_with`; refactor `chat`/`stream` to delegate; deprecate `chat_sync` | **Modify** (~+150 lines) |
| `sdks/python/motosan_ai/_stream_collect.py` | Private helper: drive a `StreamEvent` async iterator to completion, assemble `ChatResponse` | **Create** (~80 lines) |
| `sdks/python/tests/test_client_chat_with.py` | `chat_with()` direct passthrough; ChatRequest fields from Phase 1 reach the provider | **Create** |
| `sdks/python/tests/test_client_stream_with.py` | `stream_with()` passthrough + retry semantics | **Create** |
| `sdks/python/tests/test_client_stream_collect.py` | `stream_collect()` + `stream_collect_with()` assembly correctness (text, tool calls, usage, stop_reason) | **Create** |
| `sdks/python/tests/test_client_chat_sync_deprecated.py` | `chat_sync()` emits `DeprecationWarning` but still works | **Create** |
| `sdks/python/README.md` | Document the four methods with examples | **Modify** |
| `AGENTS.md` | Update Python SDK section | **Modify** |
| `llms.txt` | Update Client API surface | **Modify** |
| `skills/motosan-ai/SKILL.md` | Reflect new methods | **Modify** |
| `skills/motosan-ai/references/python-api.md` | Method-level reference | **Modify** |
| `sdks/python/CHANGELOG.md` | v0.10.0 entry | **Modify** |
| `sdks/python/pyproject.toml` | Version bump 0.9.3 → 0.10.0 | **Modify** |

Design principles:
- **Back-compat for `chat()` / `stream()`.** Their existing kwargs-based signatures don't change. Internally they build a `ChatRequest` and delegate to `chat_with` / `stream_with`.
- **`*_with` is the canonical path.** Callers needing Phase 1 fields (`tool_choice`, `thinking`, `mcp_servers`, etc.) use `Client.chat_with(ChatRequest.builder().thinking(...).build())`.
- **Retry stays where it is.** `chat_with` and `stream_with` route through the same `with_retry` and stream-retry-loop the existing methods use; no duplicate retry implementations.
- **`_collect_stream` is the only new pure function.** Pulled out of the per-provider chat() collectors (Anthropic OAuth, Code Assist already have their own copies) — Client-level wrapper means stream-only callers don't need to re-implement.
- **`chat_sync()` stays callable for one cycle** with a `DeprecationWarning`. Removed in v0.11.0.

---

## Task 1: `_collect_stream` helper

**Files:**
- Create: `sdks/python/motosan_ai/_stream_collect.py`
- Create: `sdks/python/tests/test_client_stream_collect.py`

Pure async function that iterates `AsyncIterator[StreamEvent]`, accumulates text, tool calls, usage, and the terminal `stop_reason`, and returns a `ChatResponse`. Mirrors the boilerplate already inlined in Anthropic's OAuth `chat()` and Code Assist's `chat()`.

- [ ] **Step 1: Write failing test**

Create `sdks/python/tests/test_client_stream_collect.py`:

```python
from __future__ import annotations

import json

import pytest

from motosan_ai._stream_collect import collect_stream
from motosan_ai.types import StopReason, StreamEvent, ToolCall, Usage


async def _events_to_iter(events):
    for e in events:
        yield e


@pytest.mark.asyncio
async def test_collect_text_only():
    events = [
        StreamEvent(content="Hello ", done=False),
        StreamEvent(content="world", done=False),
        StreamEvent(content="", done=True, stop_reason=StopReason.end_turn),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.content == "Hello world"
    assert resp.tool_calls == []
    assert resp.stop_reason == StopReason.end_turn


@pytest.mark.asyncio
async def test_collect_with_usage_event():
    events = [
        StreamEvent(content="hi", done=False),
        StreamEvent(
            content="",
            done=False,
            event_type="usage",
            usage=Usage(input_tokens=10, output_tokens=5),
        ),
        StreamEvent(content="", done=True, stop_reason=StopReason.end_turn),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.usage.input_tokens == 10
    assert resp.usage.output_tokens == 5


@pytest.mark.asyncio
async def test_collect_assembles_tool_call_from_start_args_end():
    events = [
        StreamEvent(
            content="",
            done=False,
            event_type="tool_call_start",
            tool_call_id="t1",
            tool_call_name="get_weather",
        ),
        StreamEvent(
            content="",
            done=False,
            event_type="tool_call_args",
            tool_call_id="t1",
            tool_call_args_delta='{"city":',
        ),
        StreamEvent(
            content="",
            done=False,
            event_type="tool_call_args",
            tool_call_id="t1",
            tool_call_args_delta='"Taipei"}',
        ),
        StreamEvent(content="", done=False, event_type="tool_call_end", tool_call_id="t1"),
        StreamEvent(content="", done=True, stop_reason=StopReason.tool_use),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert len(resp.tool_calls) == 1
    tc = resp.tool_calls[0]
    assert tc.id == "t1"
    assert tc.name == "get_weather"
    assert tc.input == {"city": "Taipei"}
    assert resp.stop_reason == StopReason.tool_use


@pytest.mark.asyncio
async def test_collect_handles_malformed_tool_args_as_empty_dict():
    events = [
        StreamEvent(
            content="",
            done=False,
            event_type="tool_call_start",
            tool_call_id="t1",
            tool_call_name="x",
        ),
        StreamEvent(
            content="",
            done=False,
            event_type="tool_call_args",
            tool_call_id="t1",
            tool_call_args_delta="not json",
        ),
        StreamEvent(content="", done=False, event_type="tool_call_end", tool_call_id="t1"),
        StreamEvent(content="", done=True),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.tool_calls[0].input == {}


@pytest.mark.asyncio
async def test_collect_default_stop_reason_when_done_lacks_one():
    events = [
        StreamEvent(content="hi", done=False),
        StreamEvent(content="", done=True),  # no stop_reason
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.stop_reason == StopReason.end_turn  # default


@pytest.mark.asyncio
async def test_collect_thinking_content_concatenated():
    events = [
        StreamEvent(
            content="reasoning step 1",
            done=False,
            event_type="thinking",
        ),
        StreamEvent(
            content=" step 2",
            done=False,
            event_type="thinking",
        ),
        StreamEvent(content="answer", done=False),
        StreamEvent(content="", done=True),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.thinking == "reasoning step 1 step 2"
    assert resp.content == "answer"
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_client_stream_collect.py -v`
Expected: ImportError on `motosan_ai._stream_collect`.

- [ ] **Step 3: Implement helper**

Create `sdks/python/motosan_ai/_stream_collect.py`:

```python
"""Drive an async StreamEvent iterator to completion and assemble a ChatResponse.

Used by Client.stream_collect / stream_collect_with so callers don't have to
reimplement the boilerplate Anthropic OAuth and Code Assist already inline.
"""

from __future__ import annotations

import json
from collections.abc import AsyncIterator

from motosan_ai.types import ChatResponse, StopReason, StreamEvent, ToolCall, Usage


async def collect_stream(events: AsyncIterator[StreamEvent]) -> ChatResponse:
    content = ""
    thinking = ""
    tool_calls: list[ToolCall] = []
    usage = Usage(0, 0)
    stop_reason = StopReason.end_turn  # default if upstream omits

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
            try:
                parsed = json.loads(current_tc_args) if current_tc_args else {}
            except json.JSONDecodeError:
                parsed = {}
            tool_calls.append(
                ToolCall(id=current_tc_id, name=current_tc_name, input=parsed)
            )
            current_tc_id = ""
            current_tc_name = ""
            current_tc_args = ""
        elif event.event_type == "usage" and event.usage is not None:
            usage = event.usage

        if event.done and event.stop_reason is not None:
            stop_reason = event.stop_reason

    return ChatResponse(
        content=content,
        thinking=thinking or None,
        tool_calls=tool_calls,
        model="",  # caller (Client.stream_collect_with) fills this in
        usage=usage,
        stop_reason=stop_reason,
    )
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_client_stream_collect.py -v`
Expected: 6 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/_stream_collect.py sdks/python/tests/test_client_stream_collect.py
git commit -m "feat(python,client): add _collect_stream helper for stream→ChatResponse assembly"
```

---

## Task 2: `Client.chat_with(request)` — full ChatRequest passthrough

**Files:**
- Modify: `sdks/python/motosan_ai/client.py`
- Create: `sdks/python/tests/test_client_chat_with.py`

Direct passthrough that bypasses kwargs serialization. Caller controls every `ChatRequest` field including Phase 1's new ones (`tool_choice`, `thinking`, `mcp_servers`, `system_blocks`, etc.).

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_client_chat_with.py`:

```python
from __future__ import annotations

import httpx
import pytest
import respx

from motosan_ai import Client, Provider
from motosan_ai.types import (
    ChatRequest,
    Message,
    SystemBlock,
    ThinkingConfig,
    ToolChoice,
)


@respx.mock
@pytest.mark.asyncio
async def test_chat_with_passes_thinking_to_provider(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
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
    client = Client(provider=Provider.anthropic)
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .thinking(2048)
        .build()
    )
    await client.chat_with(req)

    import json as _json

    body = _json.loads(route.calls[0].request.content)
    assert body["thinking"] == {"type": "enabled", "budget_tokens": 2048}


@respx.mock
@pytest.mark.asyncio
async def test_chat_with_passes_system_blocks(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
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
    client = Client(provider=Provider.anthropic)
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .system_block(SystemBlock.cached("Base"))
        .system_block(SystemBlock.new("Dynamic"))
        .build()
    )
    await client.chat_with(req)

    import json as _json

    body = _json.loads(route.calls[0].request.content)
    assert body["system"] == [
        {"type": "text", "text": "Base", "cache_control": {"type": "ephemeral"}},
        {"type": "text", "text": "Dynamic"},
    ]


@respx.mock
@pytest.mark.asyncio
async def test_chat_with_passes_tool_choice(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
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
    client = Client(provider=Provider.anthropic)
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .tool_choice(ToolChoice.required())
        .build()
    )
    await client.chat_with(req)

    import json as _json

    body = _json.loads(route.calls[0].request.content)
    assert body["tool_choice"] == {"type": "any"}


@respx.mock
@pytest.mark.asyncio
async def test_chat_with_falls_back_to_client_model_when_request_omits(monkeypatch):
    """Per Rust: if request.model is None, Client uses self.model."""
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
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
    client = Client(provider=Provider.anthropic, model="claude-haiku-4-5-20251001")
    req = ChatRequest.builder().message(Message.user("hi")).build()
    await client.chat_with(req)

    import json as _json

    body = _json.loads(route.calls[0].request.content)
    assert body["model"] == "claude-haiku-4-5-20251001"


@respx.mock
@pytest.mark.asyncio
async def test_chat_with_request_model_overrides_client_default(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
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
    client = Client(provider=Provider.anthropic, model="default-model")
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .model("override-model")
        .build()
    )
    await client.chat_with(req)

    import json as _json

    body = _json.loads(route.calls[0].request.content)
    assert body["model"] == "override-model"
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_client_chat_with.py -v`
Expected: AttributeError on `client.chat_with`.

- [ ] **Step 3: Implement `chat_with`**

In `sdks/python/motosan_ai/client.py`, add a method to `Client`:

```python
    async def chat_with(self, request: ChatRequest) -> ChatResponse:
        """Send a fully-built ChatRequest. Use this when you need fields
        that ``chat()``'s kwargs don't expose (tool_choice, thinking,
        mcp_servers, system_blocks, etc.).

        If ``request.model`` is None, falls back to ``self.model``.
        """
        if request.model is None and self.model is not None:
            request = replace(request, model=self.model)

        if self._max_retries > 0:
            from motosan_ai.retry import with_retry

            return await with_retry(
                lambda: self._provider.chat(request),
                max_retries=self._max_retries,
            )
        return await self._provider.chat(request)
```

Add `from dataclasses import replace` to imports at the top of the file.

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_client_chat_with.py tests/test_client_integration.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/client.py sdks/python/tests/test_client_chat_with.py
git commit -m "feat(python,client): add chat_with(request) for full ChatRequest passthrough"
```

---

## Task 3: Refactor `chat()` to delegate to `chat_with`

**Files:**
- Modify: `sdks/python/motosan_ai/client.py`

`chat()` builds the request from kwargs, then calls `chat_with()`. No behavior change for existing callers; just removes the duplicate retry call site.

- [ ] **Step 1: Write regression test (existing chat() shape preserved)**

Append to `sdks/python/tests/test_client_chat_with.py`:

```python
@respx.mock
@pytest.mark.asyncio
async def test_chat_kwargs_path_unchanged_regression(monkeypatch):
    """chat() with kwargs still works after the refactor."""
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
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
    client = Client(provider=Provider.anthropic)
    resp = await client.chat(
        [Message.user("hi")],
        system="Be terse.",
        temperature=0.3,
        max_tokens=100,
    )
    assert resp.content == "ok"

    import json as _json

    body = _json.loads(route.calls[0].request.content)
    assert body["system"] == "Be terse."
    assert body["temperature"] == 0.3
    assert body["max_tokens"] == 100
```

- [ ] **Step 2: Run — should already PASS** (existing chat() path unchanged so far)

Run: `cd sdks/python && uv run pytest tests/test_client_chat_with.py::test_chat_kwargs_path_unchanged_regression -v`
Expected: PASS.

- [ ] **Step 3: Refactor `chat()`**

In `sdks/python/motosan_ai/client.py`, replace the body of `chat()`:

```python
    async def chat(
        self,
        messages: Iterable[Message | dict[str, Any]],
        *,
        tools: list[Tool] | None = None,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
        provider_options: dict[str, Any] | None = None,
    ) -> ChatResponse:
        request = self._build_request(
            messages,
            tools=tools,
            system=system,
            temperature=temperature,
            max_tokens=max_tokens,
            provider_options=provider_options,
        )
        return await self.chat_with(request)
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS — all existing tests + new `chat_with` tests + the regression test.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/client.py sdks/python/tests/test_client_chat_with.py
git commit -m "refactor(python,client): chat() delegates to chat_with()"
```

---

## Task 4: `Client.stream_with(request)` + refactor `stream()` to delegate

**Files:**
- Modify: `sdks/python/motosan_ai/client.py`
- Create: `sdks/python/tests/test_client_stream_with.py`

`stream_with()` is the canonical streaming entry point. Existing `stream()` becomes a thin kwargs-builder + delegate. Retry-on-error logic stays in `stream_with` (since async generators can't reuse `with_retry`).

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_client_stream_with.py`:

```python
from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai import Client, Provider
from motosan_ai.types import ChatRequest, Message, ThinkingConfig


def _sse_lines(*events: dict) -> str:
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


@respx.mock
@pytest.mark.asyncio
async def test_stream_with_passes_thinking_to_provider(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    sse = _sse_lines(
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "hi"},
        },
        {"type": "message_stop"},
    )
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200, text=sse, headers={"content-type": "text/event-stream"}
        )
    )
    client = Client(provider=Provider.anthropic)
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .thinking(1024)
        .build()
    )
    events = [e async for e in client.stream_with(req)]
    assert any(e.content == "hi" for e in events)
    body = json.loads(route.calls[0].request.content)
    assert body["thinking"] == {"type": "enabled", "budget_tokens": 1024}


@respx.mock
@pytest.mark.asyncio
async def test_stream_with_falls_back_to_client_model(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    sse = _sse_lines({"type": "message_stop"})
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200, text=sse, headers={"content-type": "text/event-stream"}
        )
    )
    client = Client(provider=Provider.anthropic, model="claude-haiku-4-5-20251001")
    req = ChatRequest.builder().message(Message.user("hi")).build()
    [e async for e in client.stream_with(req)]
    body = json.loads(route.calls[0].request.content)
    assert body["model"] == "claude-haiku-4-5-20251001"


@respx.mock
@pytest.mark.asyncio
async def test_stream_kwargs_path_unchanged_regression(monkeypatch):
    """stream() with kwargs still works after the refactor."""
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    sse = _sse_lines(
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "hi"},
        },
        {"type": "message_stop"},
    )
    respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200, text=sse, headers={"content-type": "text/event-stream"}
        )
    )
    client = Client(provider=Provider.anthropic)
    events = [e async for e in client.stream([Message.user("hi")], system="terse")]
    assert any(e.content == "hi" for e in events)
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_client_stream_with.py -v`
Expected: AttributeError on `client.stream_with`.

- [ ] **Step 3: Implement `stream_with` + refactor `stream`**

In `sdks/python/motosan_ai/client.py`, replace `stream()` with these two methods. Move the existing retry-loop body into `stream_with`:

```python
    async def stream_with(
        self, request: ChatRequest
    ) -> AsyncIterator[StreamEvent]:
        """Stream a fully-built ChatRequest. Falls back to ``self.model``
        when ``request.model`` is None.

        Retry semantics are identical to ``stream()`` — retryable errors
        before the first event trigger backoff up to ``max_retries``.
        """
        if request.model is None and self.model is not None:
            request = replace(request, model=self.model)

        last_error: RateLimitError | None = None
        max_attempts = self._max_retries + 1 if self._max_retries > 0 else 1
        for attempt in range(max_attempts):
            try:
                stripper = ThinkStripper()
                async for event in self._provider.stream(request):
                    if event.event_type == "text" and event.content:
                        clean = stripper.feed(event.content)
                        if clean:
                            yield StreamEvent(content=clean, done=False)
                    else:
                        if event.done:
                            remaining = stripper.flush()
                            if remaining:
                                yield StreamEvent(content=remaining, done=False)
                        yield event
                return
            except (RateLimitError, NetworkError, ProviderError) as e:
                from motosan_ai.retry import (
                    DEFAULT_INITIAL_BACKOFF,
                    DEFAULT_MAX_BACKOFF,
                    _is_retryable,
                    _parse_retry_after,
                )

                if not _is_retryable(e):
                    raise
                last_error = e
                if attempt >= self._max_retries:
                    break
                retry_after = _parse_retry_after(str(e))
                wait = min(
                    retry_after
                    if retry_after is not None
                    else DEFAULT_INITIAL_BACKOFF * (2**attempt),
                    DEFAULT_MAX_BACKOFF,
                )
                logger.warning(
                    "Retryable stream error (attempt %d/%d), retrying in %.1fs: %s",
                    attempt + 1,
                    self._max_retries,
                    wait,
                    type(e).__name__,
                )
                await asyncio.sleep(wait)
        raise last_error  # type: ignore[misc]

    async def stream(
        self,
        messages: Iterable[Message | dict[str, Any]],
        *,
        tools: list[Tool] | None = None,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
        provider_options: dict[str, Any] | None = None,
    ) -> AsyncIterator[StreamEvent]:
        request = self._build_request(
            messages,
            tools=tools,
            system=system,
            temperature=temperature,
            max_tokens=max_tokens,
            provider_options=provider_options,
        )
        async for event in self.stream_with(request):
            yield event
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS — all 3 new stream_with tests + every existing stream test still green.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/client.py sdks/python/tests/test_client_stream_with.py
git commit -m "feat(python,client): add stream_with(request); refactor stream() to delegate"
```

---

## Task 5: `Client.stream_collect(messages)` and `stream_collect_with(request)`

**Files:**
- Modify: `sdks/python/motosan_ai/client.py`
- Modify: `sdks/python/tests/test_client_stream_collect.py`

Convenience methods that drive `stream` / `stream_with` to completion via `collect_stream` and return a `ChatResponse`. The `model` field on the response defaults to `request.model` or `self.model` per Rust.

- [ ] **Step 1: Append failing tests**

Append to `sdks/python/tests/test_client_stream_collect.py`:

```python
import httpx
import respx

from motosan_ai import Client, Provider
from motosan_ai.types import ChatRequest, Message


def _sse(*events):
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


@respx.mock
@pytest.mark.asyncio
async def test_client_stream_collect_returns_assembled_response(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    sse = _sse(
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "Hello "},
        },
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "world."},
        },
        {"type": "message_stop"},
    )
    respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200, text=sse, headers={"content-type": "text/event-stream"}
        )
    )
    client = Client(provider=Provider.anthropic, model="claude-sonnet-4-6")
    resp = await client.stream_collect([Message.user("hi")])
    assert resp.content == "Hello world."
    # model field falls back to client.model when stream doesn't carry it
    assert resp.model == "claude-sonnet-4-6"


@respx.mock
@pytest.mark.asyncio
async def test_client_stream_collect_with_uses_request_model(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    sse = _sse({"type": "message_stop"})
    respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200, text=sse, headers={"content-type": "text/event-stream"}
        )
    )
    client = Client(provider=Provider.anthropic, model="default-model")
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .model("override-model")
        .build()
    )
    resp = await client.stream_collect_with(req)
    assert resp.model == "override-model"


@respx.mock
@pytest.mark.asyncio
async def test_client_stream_collect_with_passes_thinking(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    sse = _sse({"type": "message_stop"})
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200, text=sse, headers={"content-type": "text/event-stream"}
        )
    )
    client = Client(provider=Provider.anthropic)
    req = (
        ChatRequest.builder()
        .message(Message.user("hi"))
        .thinking(2048)
        .build()
    )
    await client.stream_collect_with(req)
    body = json.loads(route.calls[0].request.content)
    assert body["thinking"] == {"type": "enabled", "budget_tokens": 2048}
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_client_stream_collect.py -v`
Expected: AttributeError on `client.stream_collect`.

- [ ] **Step 3: Implement methods**

Add to `Client` in `client.py`:

```python
    async def stream_collect(
        self,
        messages: Iterable[Message | dict[str, Any]],
        *,
        tools: list[Tool] | None = None,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
        provider_options: dict[str, Any] | None = None,
    ) -> ChatResponse:
        """Stream a chat request and assemble the full ChatResponse.

        Convenience wrapper around ``stream()`` + ``collect_stream``.
        """
        from motosan_ai._stream_collect import collect_stream

        events = self.stream(
            messages,
            tools=tools,
            system=system,
            temperature=temperature,
            max_tokens=max_tokens,
            provider_options=provider_options,
        )
        response = await collect_stream(events)
        if not response.model and self.model is not None:
            response = replace(response, model=self.model)
        return response

    async def stream_collect_with(self, request: ChatRequest) -> ChatResponse:
        """Stream a fully-built ChatRequest and assemble the full
        ChatResponse. Use this when you need Phase 1 fields
        (tool_choice, thinking, mcp_servers, system_blocks, etc.)."""
        from motosan_ai._stream_collect import collect_stream

        model_hint = request.model or self.model or ""
        events = self.stream_with(request)
        response = await collect_stream(events)
        if not response.model:
            response = replace(response, model=model_hint)
        return response
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_client_stream_collect.py -v`
Expected: 9 PASS (6 helper tests + 3 client tests).

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/client.py sdks/python/tests/test_client_stream_collect.py
git commit -m "feat(python,client): add stream_collect / stream_collect_with"
```

---

## Task 6: Soft-deprecate `chat_sync()`

**Files:**
- Modify: `sdks/python/motosan_ai/client.py`
- Create: `sdks/python/tests/test_client_chat_sync_deprecated.py`

Per CLAUDE.md: "No sync wrappers in Python". `chat_sync()` stays callable for one cycle but emits `DeprecationWarning`. Removed in v0.11.0.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_client_chat_sync_deprecated.py`:

```python
from __future__ import annotations

import warnings

import httpx
import pytest
import respx

from motosan_ai import Client, Provider
from motosan_ai.types import Message


@respx.mock
def test_chat_sync_emits_deprecation_warning(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    respx.post("https://api.anthropic.com/v1/messages").mock(
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
    client = Client(provider=Provider.anthropic)
    with warnings.catch_warnings(record=True) as recorded:
        warnings.simplefilter("always")
        resp = client.chat_sync([Message.user("hi")])
    assert resp.content == "ok"

    deprecations = [w for w in recorded if issubclass(w.category, DeprecationWarning)]
    assert len(deprecations) == 1
    assert "chat_sync" in str(deprecations[0].message)
    assert "asyncio.run" in str(deprecations[0].message)
```

- [ ] **Step 2: Run — FAIL** (no warning yet)

Run: `cd sdks/python && uv run pytest tests/test_client_chat_sync_deprecated.py -v`
Expected: assertion failure on `len(deprecations) == 1`.

- [ ] **Step 3: Add `DeprecationWarning`**

In `sdks/python/motosan_ai/client.py`, modify `chat_sync()`:

```python
    def chat_sync(
        self,
        messages: Iterable[Message | dict[str, Any]],
        *,
        tools: list[Tool] | None = None,
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
        provider_options: dict[str, Any] | None = None,
    ) -> ChatResponse:
        """Deprecated. Wrap ``await client.chat(...)`` in ``asyncio.run()`` instead.

        Will be removed in v0.11.0.
        """
        import warnings

        warnings.warn(
            "Client.chat_sync() is deprecated and will be removed in v0.11.0. "
            "Wrap await client.chat(...) in asyncio.run() instead.",
            DeprecationWarning,
            stacklevel=2,
        )
        return asyncio.run(
            self.chat(
                messages,
                tools=tools,
                system=system,
                temperature=temperature,
                max_tokens=max_tokens,
                provider_options=provider_options,
            )
        )
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_client_chat_sync_deprecated.py -v`
Expected: 1 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/client.py sdks/python/tests/test_client_chat_sync_deprecated.py
git commit -m "feat(python,client): soft-deprecate chat_sync() — DeprecationWarning, removal in v0.11.0"
```

---

## Task 7: Update `motosan_ai/__init__.py` exports

**Files:**
- Modify: `sdks/python/motosan_ai/__init__.py`

Re-export `collect_stream` for callers who want direct access to the helper without going through `Client.stream_collect`.

- [ ] **Step 1: Append failing test**

Append to `sdks/python/tests/test_client_stream_collect.py`:

```python
def test_collect_stream_exported_from_top_level():
    import motosan_ai

    assert callable(motosan_ai.collect_stream)
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_client_stream_collect.py::test_collect_stream_exported_from_top_level -v`
Expected: AttributeError.

- [ ] **Step 3: Add export**

In `sdks/python/motosan_ai/__init__.py`, add:

```python
from motosan_ai._stream_collect import collect_stream
```

Add `"collect_stream"` to `__all__`.

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_client_stream_collect.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/__init__.py sdks/python/tests/test_client_stream_collect.py
git commit -m "feat(python): export collect_stream from top-level"
```

---

## Task 8: Doc pass — `sdks/python/README.md`

**Files:**
- Modify: `sdks/python/README.md`

Document the four new methods with concrete examples.

- [ ] **Step 1: Locate existing Client section**

Run: `grep -n "Client.chat\|chat_sync\|chat(" sdks/python/README.md | head -10`

- [ ] **Step 2: Add a "Full ChatRequest control" section**

In `sdks/python/README.md`, after the existing `Client.chat()` example, add:

```markdown
### Full `ChatRequest` control

`Client.chat()` exposes the common kwargs (`tools`, `system`, `temperature`,
`max_tokens`, `provider_options`). For Phase 1 fields like `tool_choice`,
`thinking`, `mcp_servers`, `system_blocks`, or `stop_sequences`, use
`chat_with()` with a `ChatRequest.builder()`:

```python
from motosan_ai import Client, ChatRequest, Message, ThinkingConfig, ToolChoice

client = Client.anthropic()

req = (
    ChatRequest.builder()
    .message(Message.user("Solve: 13 * 17"))
    .thinking(2048)
    .system_cached("Show your work step by step.")
    .build()
)
resp = await client.chat_with(req)
print(resp.thinking)
print(resp.content)
```

### Streaming → assembled response

`stream_collect()` and `stream_collect_with()` drive a stream to completion
and return a `ChatResponse`. Useful when the underlying provider is
stream-only (e.g. Anthropic OAuth tokens):

```python
from motosan_ai import Client, ChatRequest, Message

client = Client.anthropic()

# Convenience kwargs path
resp = await client.stream_collect([Message.user("hi")])

# Or with full ChatRequest control
req = ChatRequest.builder().message(Message.user("hi")).thinking(1024).build()
resp = await client.stream_collect_with(req)
```
```

- [ ] **Step 3: Mark `chat_sync` as deprecated**

Find any `chat_sync` reference in the README and prepend with a deprecation note:

```markdown
> **Deprecated** since v0.10.0; will be removed in v0.11.0. Use
> `asyncio.run(client.chat(...))` instead.
```

- [ ] **Step 4: Visual review**

Run: `grep -n "chat_with\|stream_with\|stream_collect" sdks/python/README.md`
Expected: each method appears with at least one example.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/README.md
git commit -m "docs(python): document chat_with / stream_with / stream_collect; flag chat_sync deprecation"
```

---

## Task 9: Doc pass — repo-level docs

**Files:**
- Modify: `AGENTS.md`
- Modify: `llms.txt`
- Modify: `skills/motosan-ai/SKILL.md`
- Modify: `skills/motosan-ai/references/python-api.md`

- [ ] **Step 1: Locate Python Client method references in each doc**

Run: `grep -nl "Client.chat\|chat_with\|stream_with\|stream_collect" AGENTS.md llms.txt skills/motosan-ai/SKILL.md skills/motosan-ai/references/python-api.md`

- [ ] **Step 2: Update `AGENTS.md` Python SDK section**

Add a "Phase 4 / v0.10.0" entry under recent changes (or wherever the doc tracks Client API surface). Include:

```markdown
- `Client.chat_with(request)` / `stream_with(request)` / `stream_collect(messages)` / `stream_collect_with(request)` — full `ChatRequest` passthrough; pair with `ChatRequest.builder()` for Phase 1 fields like `thinking`, `tool_choice`, `mcp_servers`, `system_blocks`.
- `Client.chat_sync()` deprecated; removal in v0.11.0.
```

- [ ] **Step 3: Update `llms.txt`**

Add the four method signatures under the Python Client API section. Use the same brief style as the existing `chat` / `stream` entries.

- [ ] **Step 4: Update `skills/motosan-ai/SKILL.md`**

Add a one-paragraph mention of the canonical `*_with` methods and the convenience shape of `stream_collect`. Match the skill's existing tone.

- [ ] **Step 5: Update `skills/motosan-ai/references/python-api.md`**

Add a method-level reference section for each of the four new methods, mirroring the existing `chat()` / `stream()` entries. Include signatures, parameters, return types, and a one-line use case.

- [ ] **Step 6: Commit**

```bash
git add AGENTS.md llms.txt skills/motosan-ai/
git commit -m "docs: document Phase 4 Client API across repo-level reference docs"
```

---

## Task 10: Release — CHANGELOG + version bump to 0.10.0

**Files:**
- Modify: `sdks/python/CHANGELOG.md`
- Modify: `sdks/python/pyproject.toml`

- [ ] **Step 1: Bump version**

Edit `sdks/python/pyproject.toml`:

```toml
version = "0.10.0"
```

- [ ] **Step 2: Prepend CHANGELOG entry**

Replace YYYY-MM-DD with the actual release day.

```markdown
## [0.10.0] - YYYY-MM-DD

### Added — Client API parity with Rust SDK (Phase 4)
- **`Client.chat_with(request: ChatRequest)`** — full ChatRequest passthrough. Use with `ChatRequest.builder()` for Phase 1 fields like `tool_choice`, `thinking`, `mcp_servers`, `system_blocks`, `stop_sequences`.
- **`Client.stream_with(request: ChatRequest)`** — full ChatRequest passthrough for streaming. Same retry semantics as `stream()`.
- **`Client.stream_collect(messages, **kwargs)`** — drives a stream to completion and returns the assembled `ChatResponse`. Convenience wrapper around `stream() + collect_stream()`.
- **`Client.stream_collect_with(request: ChatRequest)`** — streaming + collecting with full ChatRequest control.
- **`motosan_ai.collect_stream(events) -> ChatResponse`** — top-level helper for callers who want stream-to-response assembly without going through `Client`. Handles text + tool calls (start/args/end) + usage + thinking + stop_reason.

### Changed
- `Client.chat()` and `Client.stream()` now delegate to `chat_with()` / `stream_with()` internally. No behavior change for existing callers.
- Both `*_with` methods fall back to `client.model` when `request.model` is None (matches Rust precedence).

### Deprecated
- **`Client.chat_sync()`** — emits `DeprecationWarning`. Wrap `await client.chat(...)` in `asyncio.run()` instead. Will be removed in v0.11.0.

### Notes
- Phase 4 closes the Rust-parity roadmap. Python SDK is now method-for-method aligned with `motosan-ai` Rust v0.14.x at the Client layer.
- See `docs/superpowers/plans/2026-04-26-python-sdk-phase4-client-api-parity.md` for the per-task TDD breakdown.
```

- [ ] **Step 3: Run the gate**

Run: `cd sdks/python && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration/`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add sdks/python/CHANGELOG.md sdks/python/pyproject.toml
git commit -m "chore(python): release v0.10.0 — Client API parity (Phase 4)"
```

---

## Final Self-Review Checklist

Before declaring Phase 4 done, verify:

- [ ] `cd sdks/python && uv run pytest tests/ -v` — all tests pass (target: ~530+ passing).
- [ ] `check-python` gate passes (ruff + format + pytest).
- [ ] **Method-for-method parity with Rust** — cross-check via `grep "pub async fn " sdks/rust/src/client.rs` against `grep "async def " sdks/python/motosan_ai/client.py`. The 4 methods (`chat`, `chat_with`, `stream`, `stream_with`, `stream_collect`, `stream_collect_with`) all present.
- [ ] **Model fallback** — `chat_with(req)` and `stream_with(req)` correctly use `client.model` when `request.model` is None. `stream_collect_with` fills `response.model` with `request.model or client.model` when stream omits it (matches Rust line 153-162).
- [ ] **No regression on existing kwargs callers** — `test_chat_kwargs_path_unchanged_regression` and `test_stream_kwargs_path_unchanged_regression` PASS.
- [ ] **`collect_stream` handles all event types** — text, thinking, tool_call_start/args/end, usage, terminal done with stop_reason. 6 dedicated tests cover each.
- [ ] **`chat_sync` emits `DeprecationWarning`** with a precise message pointing at `asyncio.run`. Function still returns a correct `ChatResponse`.
- [ ] **Top-level `motosan_ai.collect_stream` import works** — verified by smoke test.
- [ ] **Docs updated** — README, AGENTS.md, llms.txt, SKILL.md, python-api.md all reflect the new methods.
- [ ] No `TODO` / `FIXME` / placeholder strings introduced.

If any box is unchecked, fix before tagging/publishing.

---

## What Phase 4 does NOT do

- ❌ **Remove `chat_sync()`** — kept callable with `DeprecationWarning` for v0.10.0; removed in v0.11.0.
- ❌ **Add new ChatRequest fields** — `chat()`'s kwargs surface stays as-is. Callers needing newer fields use `chat_with(builder.build())`.
- ❌ **Per-provider streaming retry tuning** — current retry logic in `stream_with` matches what `stream` had; no per-provider overrides.
- ❌ **Sync wrapper for streaming** — there is no `stream_sync()` and there will not be. Async-only for streams.
- ❌ **`ChatRequest.builder()` API additions** — already complete in Phase 1; no Phase 4 work.
- ❌ **Rust-side changes** — Python only. Rust Client API is the canon Python is now matching.

All non-goals tracked in the roadmap doc; v0.11.0 will revisit `chat_sync` removal and any field additions discovered during real-world use.
