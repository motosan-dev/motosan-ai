# Plan: Port the chatgpt-codex provider to the Python SDK (release 0.14.0)

## ⚠️ CORRECTIONS (AUTHORITATIVE — these supersede anything below them)

This plan was researched against a **stale checkout** (the repo's main working tree was on a pre-0.13.0
branch + the codegraph index lags), so three things are wrong below and MUST be corrected. **When
implementing, work in a worktree off the CURRENT `origin/main` and READ the real worktree files** — the
plan's quoted line numbers / version strings / "X does not exist" notes are approximate. Ground every
edit in the actual current code (which already contains F1–F6 + the 0.13.0 release).

**C1 — ADD a provider-level default reasoning effort (for true Rust 0.21 parity).** The research missed
this because it read a pre-`70d3cdd` Rust source. The CURRENT `sdks/rust/src/providers/chatgpt_codex.rs`
has it; port it:
- Provider gains a field `reasoning_effort: str | None = None` and a builder setter
  `def reasoning_effort(self, effort: str | None) -> Self` (mirrors Rust `with_reasoning_effort`; use the
  no-`with_` Python naming like `.model()`/`.timeout()`). `new(...)`/the ctor stays stable (defaults None).
- In `_build_responses_body`, resolve effort as: **per-request `provider_options["reasoning_effort"]`
  wins; else the provider-level default; else OMIT the `reasoning` object entirely.** When an effort is
  resolved, emit `body["reasoning"] = {"effort": <effort>, "summary": "auto"}` (verbatim pass-through).
- `Client.chatgpt_codex(access_token, account_id, model, reasoning_effort=None)` gains the optional
  param and threads it into the provider's default (mirrors Rust `ClientBuilder::chatgpt_codex_reasoning_effort`).
- Tests (fold into T1 body tests + T4 wiring): reasoning emitted when (a) per-request option set,
  (b) provider default set; per-request **wins** over the default; omitted when neither is set.

**C2 — Version baseline is 0.13.0, NOT 0.12.1.** Current `origin/main` `sdks/python/pyproject.toml` =
`0.13.0`. T5 bumps **`0.13.0` → `0.14.0`** (match the real `version = "0.13.0"` string). Every release
file's current Python line is at 0.13.0 (AGENTS.md `Python v0.13.0`, llms.txt `Python 0.13.0`, SKILL.md
`Python 0.13.0`) — bump those to 0.14.0. Ignore the plan's "0.12.1" references.

**C3 — `uv.lock` is at the REPO ROOT (`/uv.lock`), not `sdks/python/uv.lock`.** In T5: run
`uv lock --project sdks/python` then `git add uv.lock` (root). There is no `sdks/python/uv.lock`.

(Acknowledged nits, no action required: the redundant `@pytest.mark.asyncio` decorators are harmless
under `asyncio_mode = "auto"`; non-200 error text passing the raw body matches the gemini_code_assist
analog; `StreamEvent.session_id` DOES exist now (F3) but is irrelevant to this HTTP provider.)

---

## Goal

Add a native ChatGPT-backend inference provider (`ChatGptCodexProvider`) to the **Python** SDK that
mirrors Rust's `sdks/rust/src/providers/chatgpt_codex.rs`. It POSTs the OpenAI **Responses API** to
`https://chatgpt.com/backend-api/codex/responses` with a caller-supplied OAuth bearer token + ChatGPT
account id + the codex CLI headers, streams the typed `response.*` SSE events, and maps them to motosan
`StreamEvent`s (text, reasoning→thinking, function_call tool lifecycle, usage, terminal stop reason).
Wire it into `Client` as `Provider.openai_chatgpt` / `Client.chatgpt_codex(...)`, and ship it as Python
**0.14.0** (additive, non-breaking).

Auth is **pre-obtained**: the caller supplies `access_token` + `account_id` directly (exactly like Rust
`ChatGptCodexProvider::new(access_token, account_id, model, base_url)`). There is **no OAuth login flow**
in scope and `api_key` is **not** required for this provider (like the CLI / Gemini Code Assist backends).

## Architecture

- **One new provider file**: `sdks/python/motosan_ai/providers/chatgpt_codex.py`. Mirrors the structure
  of the existing no-api-key OAuth-Bearer HTTP provider `providers/gemini_code_assist.py` exactly:
  module-level constants, an adapter-state dataclass, a pure unit-testable `_parse_sse_event(data, state)
  -> list[StreamEvent]` function (the port of Rust's `handle_event`), and a `ChatGptCodexProvider`
  subclass of `BaseProvider` whose `stream()` does the httpx streaming POST and whose `chat()` is
  `collect_stream(stream())`.
- **The Responses-API body builder and `response.*` SSE adapter are NEW code.** Verified: `providers/openai.py`
  builds only the **Chat Completions** API (`/v1/chat/completions`, `messages`/`choices[].delta`); it has
  no `instructions`/`input`/`input_text`/`output_text` or `response.*` parsing. Nothing to reuse there —
  both are ported fresh from the Rust `build_responses_body` + `handle_event`.
- **Client wiring**: a new `Provider.openai_chatgpt` enum variant, an `__init__` dispatch branch
  (api-key-not-required, requires `access_token` + `account_id`), a `Client.chatgpt_codex(...)` classmethod,
  a new `account_id` constructor param, and exports through `providers/__init__.py` + top-level `__init__.py`
  + both `__all__`s.
- **chat() == collect_stream(stream())** — there is no second non-streaming HTTP path (mirrors
  `gemini_code_assist.py` and Rust).
- **One PR / one branch**: `feat/py-chatgpt-codex`. Five tasks (T1–T4 code, T5 release).

## Tech Stack

- Python 3.11+, async-only (no sync wrappers; callers use `asyncio.run()`).
- `httpx.AsyncClient(timeout=120.0)` for the streaming POST (same as `gemini_code_assist.py`).
- Tests: `pytest` + `pytest-asyncio` (`asyncio_mode = "auto"`) + `respx>=0.21` for HTTP mocking.
- Lint/format: `ruff` via `uv`. CI lint scope is `ruff check motosan_ai/` only — `tests/` is NOT linted.
- Run everything via `uv`: `uv run pytest`, `uv run ruff check motosan_ai/`, `uv run ruff format`.

---

## Global Constraints

### Locked decisions (do not deviate)

1. **Auth = pre-obtained token.** `ChatGptCodexProvider.__init__(self, access_token, account_id,
   model=None, base_url=None)`. NO OAuth login flow. `api_key` is NOT required for this provider.
2. **Python only.** Do NOT touch `sdks/rust` or `sdks/typescript`.
3. **Idiomatic Python**: async-only, builder methods return `self`, provider logic ONLY in `providers/`,
   reuse the existing `StreamEvent`/`ChatResponse`/`Usage`/`StopReason`/`ToolCall` types. `chat()` =
   `collect_stream(stream())`.
4. **Release as Python 0.14.0** (additive, non-breaking). This is per the explicit task instruction. See
   "Spec gaps" below — the on-disk baseline is 0.12.1, not the 0.13.0 the task premise assumed; bumping
   straight to 0.14.0 as instructed leaves 0.13.0 unused, which is harmless for an additive release.

### Verified environment facts (these OVERRIDE the task premise where they conflict)

- **On-disk Python version is `0.12.1`** (`sdks/python/pyproject.toml:3`), NOT 0.13.0. Target 0.14.0 per
  instruction.
- **`StreamEvent` has NO `session_id` field** (`types.py:349-358`). The task premise ("StreamEvent now has
  session_id from 0.13.0") is false in this checkout. This is fine — an HTTP provider never sets a session
  id. Do NOT add or reference `session_id`.
- **`collect_stream` keys reasoning on the literal string `"thinking"`** (`_stream_collect.py:32`:
  `elif event.event_type == "thinking" and event.content:`), NOT `"thinking_delta"`. The Python adapter
  MUST emit `event_type="thinking"` for reasoning deltas, or `ChatResponse.thinking` stays empty. (This
  contradicts one research note that said `"thinking_delta"` — that note is WRONG; `"thinking_delta"` is
  never consumed by `collect_stream` and would silently drop reasoning.)
- **`StreamEventType` StrEnum** (`types.py:24-29`) lists only `text/tool_call_start/tool_call_args/
  tool_call_end/usage` — it does NOT contain `"thinking"`. That is fine: `StreamEvent.event_type` is a
  plain `str` (default `"text"`), and `collect_stream` keys on the literal string, so emit the literal
  `"thinking"` regardless of the enum.
- **`Tool.description` / `Tool.input_schema` are Optional** in Python (`types.py:220-224`). Emit them
  as-is (the Rust `ToolSchema` requires them; Python passes through whatever is present, including `None`).
- **`validate_request`** (`provider_base.py:32-39`) only rejects `content_blocks` Image/Document for a
  text_only provider. Flat-text user/assistant messages always pass. Call it first in `stream()`.
- **`BaseProvider.capabilities` default is `text_only()`** — the chatgpt-codex provider is text-only, so
  it does not need to override `capabilities` (but we set it explicitly for parity/clarity).

### Gates (run before every commit; all must pass)

```bash
uv run --project sdks/python ruff check motosan_ai/
uv run --project sdks/python ruff format --check
uv run --project sdks/python pytest
```

(If `uv run --project sdks/python <cmd>` is awkward in your shell, `cd sdks/python` first is fine — but
this plan uses explicit `--project sdks/python` so paths are unambiguous. Equivalent local form:
`cd sdks/python && uv run pytest`.) Test files under `tests/` are intentionally NOT linted (CI scope is
`ruff check motosan_ai/`), but they ARE format-checked by `ruff format --check`, so run `uv run ruff format`
over new test files too.

### Constants (exact)

```python
_DEFAULT_BASE_URL = "https://chatgpt.com/backend-api/codex/responses"
_ORIGINATOR = "codex_cli_rs"
_DEFAULT_MODEL = "gpt-5.5"   # used only when neither req.model nor provider model is set
```

Rust `new(...)` has NO model default (the caller always supplies one), but Python signatures default
`model=None`. To keep `_build_responses_body` and `chat()` total, store `self.model = model or
_DEFAULT_MODEL` so a bare provider still serializes `"model": "gpt-5.5"` — which is exactly what the Rust
test `body_has_required_codex_fields` asserts (`ChatGptCodexProvider::new(..., "gpt-5.5", None)`).

### Headers (exact, all lowercase keys, this order)

```python
{
    "authorization": f"Bearer {self.access_token}",
    "chatgpt-account-id": self.account_id,
    "originator": "codex_cli_rs",
    "openai-beta": "responses=experimental",
    "accept": "text/event-stream",
    "content-type": "application/json",
}
```

### URL

`_stream_url()` returns the base URL **verbatim** — the base_url already IS the full endpoint path
(default = `_DEFAULT_BASE_URL`). Unlike `gemini_code_assist.py`, do NOT append any path or query. Store
`self.base_url = base_url or _DEFAULT_BASE_URL` (the `.rstrip("/")` trick from gemini does NOT apply; the
URL ends in `/responses`, not a trailing slash, so rstrip is a harmless no-op but also pointless — omit it).

### Responses body shape (authoritative — port of Rust `build_responses_body`)

`_build_responses_body(self, request) -> dict[str, Any]`:

- `model` = `request.model or self.model`
- **Instructions (system prompt) precedence** (mirrors `openai.rs`):
  - If `request.system_blocks` is not None: for each block, `block.text.strip()`; append non-empty.
    `system_blocks` takes PRIORITY over `system`.
  - Else if `request.system` is not None: `request.system.strip()`; append if non-empty.
  - Plus: every `Role.system` **message** in `request.messages` appends its `content.strip()` to the
    instructions parts (system messages go to instructions, NEVER to `input`).
  - Join all parts with `"\n\n"`. If empty → fallback `"You are a helpful assistant."`.
- **`input` items** (iterate `request.messages`):
  - `Role.system` → append trimmed content to instructions; emit NO input item.
  - `Role.user` → `{"type": "message", "role": "user", "content": [{"type": "input_text", "text":
    message.content}]}` (text-only; image/document blocks are a phase-2 TODO, NOT emitted).
  - `Role.assistant`:
    - if `message.content` is non-empty → `{"type": "message", "role": "assistant", "content":
      [{"type": "output_text", "text": message.content}]}`
    - for each `tc` in `message.tool_calls` → `{"type": "function_call", "call_id": tc.id, "name":
      tc.name, "arguments": json.dumps(tc.input)}` (arguments is a JSON-encoded **string**).
  - `Role.tool` → only if `message.tool_call_id` is not None: `{"type": "function_call_output",
    "call_id": message.tool_call_id, "output": message.content}`.
- **Base body (always present):**
  ```json
  {
    "model": <model>,
    "store": false,
    "stream": true,
    "instructions": <instructions string>,
    "input": [ ...items... ],
    "include": ["reasoning.encrypted_content"],
    "tool_choice": "auto",
    "parallel_tool_calls": true
  }
  ```
- **Conditional `tools`** (only if `request.tools` is not None AND the mapped list is non-empty) — flat
  Responses tool shape with **`"strict": None`** (serializes to JSON `null`):
  ```python
  {"type": "function", "name": t.name, "description": t.description,
   "parameters": t.input_schema, "strict": None}
  ```
  Pass `t.description` and `t.input_schema` through as-is (they may be `None` in Python).
- **Conditional `reasoning`** (only if `request.provider_options` is not None and
  `request.provider_options.get("reasoning_effort")` is a `str`):
  `"reasoning": {"effort": <effort>, "summary": "auto"}`.
- **Conditional `temperature`** (only if `request.temperature` is not None): `"temperature": <value>`.

### `response.*` event → StreamEvent mapping table (authoritative — port of Rust `handle_event`)

Adapter state (dataclass `_ChatGptCodexAdapterState`): `seen_tool_ids: set[str]`, `saw_tool_call: bool =
False`, `error: str | None = None`. The pure mapping function `_parse_sse_event(data: str, state) ->
list[StreamEvent]` JSON-loads `data` (returns `[]` on empty / `"[DONE]"` / `JSONDecodeError` /
non-dict), then matches on `data["type"]`:

| SSE `type` string | Reads | Emits (Python `StreamEvent`) |
|---|---|---|
| `response.output_text.delta` | `delta` (str) | if non-empty: `StreamEvent(content=delta, done=False)` (event_type defaults to `"text"`) |
| `response.reasoning_text.delta` **OR** `response.reasoning_summary_text.delta` | `delta` (str) | if non-empty: `StreamEvent(content=delta, done=False, event_type="thinking")` — **MUST be `"thinking"`** (that is the literal `collect_stream` keys on) |
| `response.output_item.added` | `item.type`; if `== "function_call"`: `item.call_id`, `item.name` | if `call_id` non-empty: set `state.saw_tool_call = True`, `state.seen_tool_ids.add(call_id)`, emit `StreamEvent(content="", done=False, event_type="tool_call_start", tool_call_id=call_id, tool_call_name=name)`. Non-function_call items ignored. |
| `response.function_call_arguments.delta` | **`item_id`** (top-level key — NOT `call_id`), `delta` | if `item_id` non-empty: `StreamEvent(content="", done=False, event_type="tool_call_args", tool_call_id=item_id, tool_call_args_delta=delta)` |
| `response.output_item.done` | `item.type`; if `== "function_call"`: `item.call_id` | if `call_id` non-empty: `StreamEvent(content="", done=False, event_type="tool_call_end", tool_call_id=call_id)`. reasoning/message items ignored. |
| `response.completed` | `response.usage.{input_tokens,output_tokens}`, `response.usage.input_tokens_details.cached_tokens`, `response.status` | **First**, if `usage` present: `StreamEvent(content="", done=False, event_type="usage", usage=Usage(input_tokens, output_tokens, cache_creation_input_tokens=None, cache_read_input_tokens=cached if cached > 0 else None))` — surface `cached` **as-is** (do NOT subtract, unlike gemini). **Then** a terminal: stop = `tool_use` if `state.saw_tool_call` else `max_tokens` if `status == "incomplete"` else `end_turn`; emit `StreamEvent(content="", done=True, stop_reason=stop)`. |
| `error` **OR** `response.failed` | message from (in order) `data["message"]` → `data["response"]["error"]["message"]` → `data["error"]["message"]` → fallback `"ChatGPT-backend stream error"` | set `state.error = msg`; emit `[]` (the `stream()` loop raises `StreamError(state.error)` after draining — see T2). |
| anything else (`response.created`, `response.in_progress`, `content_part.*`, `response.output_text.done`, reasoning item add, etc.) | — | `[]` (ignored) |

Id-source gotchas to preserve **exactly**:
- `output_item.added` / `output_item.done` read the call id from `item.call_id`.
- `function_call_arguments.delta` reads the id from top-level `item_id` (NOT `call_id`).
- Usage `input_tokens` is the full prompt count already — surface `cached_tokens` as-is.

### Error mapping

```python
@staticmethod
def _map_http_error(status: int, message: str) -> Exception:
    if status == 401:
        return AuthError(message)
    if status == 429:
        return RateLimitError(message)
    return ProviderError(message)
```

Transport errors (`httpx.HTTPError`) → `raise NetworkError(str(exc)) from exc`. Fatal in-stream errors
(`error` / `response.failed` frames) → `raise StreamError(state.error)`. All five (`AuthError`,
`RateLimitError`, `ProviderError`, `NetworkError`, `StreamError`) come from `motosan_ai.error` and
subclass `MotosanError`.

### TDD discipline (every code task)

Write the failing test FIRST, run it to confirm it fails (red), write the full implementation (no
placeholders), run it green, then `uv run --project sdks/python ruff format` + `ruff check motosan_ai/`,
then commit. Each task is one commit on branch `feat/py-chatgpt-codex`.

---

## Task TOC

- **T0** — Branch setup (`feat/py-chatgpt-codex`).
- **T1** — Request body builder `_build_responses_body` + provider scaffold + request unit tests.
- **T2** — SSE `response.*` stream adapter (`_parse_sse_event` + state) + adapter unit tests.
- **T3** — Provider class `stream()` / `chat()` (auth headers, base_url, capabilities, httpx POST,
  StreamError raise, NetworkError wrap, `_map_http_error`) + respx HTTP tests.
- **T4** — Client wiring (`Provider.openai_chatgpt`, dispatch, `Client.chatgpt_codex(...)`, `account_id`
  param, exports) + dispatch tests + skip-guarded live test.
- **T5** — Release 0.14.0 (pyproject, CHANGELOG, AGENTS.md, llms.txt, SKILL.md, uv.lock) + done-gate greps.

---

## T0 — Branch setup

```bash
git -C /Users/daiwanwei/Projects/wade/motosan-ai checkout main
git -C /Users/daiwanwei/Projects/wade/motosan-ai pull --ff-only
git -C /Users/daiwanwei/Projects/wade/motosan-ai checkout -b feat/py-chatgpt-codex
```

(If `main` is not the desired base, branch from the current default per project workflow. Per MEMORY.md,
every `.py`/packaging change goes through a PR + CI — this whole plan is one PR off `feat/py-chatgpt-codex`.)

---

## T1 — Request body builder + provider scaffold + request unit tests

**Files:** create `sdks/python/motosan_ai/providers/chatgpt_codex.py` (scaffold + `_build_responses_body`),
create `sdks/python/tests/test_chatgpt_codex_request.py`.

### T1.1 — Write the failing test file

Create `sdks/python/tests/test_chatgpt_codex_request.py`. These mirror the Rust body tests
(`body_has_required_codex_fields`, `system_message_goes_to_instructions_not_input`,
`assistant_text_becomes_output_text_item`, `tool_call_and_result_serialize_as_function_items`,
`reasoning_and_temperature_are_conditional`) plus header/default checks.

```python
from __future__ import annotations

import json

from motosan_ai.providers.chatgpt_codex import ChatGptCodexProvider
from motosan_ai.types import (
    ChatRequest,
    Message,
    SystemBlock,
    Tool,
    ToolCall,
)


def _provider() -> ChatGptCodexProvider:
    return ChatGptCodexProvider("test-token", "acct-123", "gpt-5.5", None)


def test_default_model_and_base_url():
    p = ChatGptCodexProvider("tok", "acct-123")
    assert p.model == "gpt-5.5"
    assert p.base_url == "https://chatgpt.com/backend-api/codex/responses"
    assert p.access_token == "tok"
    assert p.account_id == "acct-123"


def test_explicit_model_and_base_url():
    p = ChatGptCodexProvider("tok", "acct-123", model="gpt-x", base_url="https://mock.test/r")
    assert p.model == "gpt-x"
    assert p.base_url == "https://mock.test/r"


def test_stream_url_returns_base_url_verbatim():
    p = ChatGptCodexProvider("tok", "acct-123", base_url="https://mock.test/r")
    assert p._stream_url() == "https://mock.test/r"


def test_auth_headers_present_and_lowercase():
    h = ChatGptCodexProvider("tok", "acct-123")._headers()
    assert h["authorization"] == "Bearer tok"
    assert h["chatgpt-account-id"] == "acct-123"
    assert h["originator"] == "codex_cli_rs"
    assert h["openai-beta"] == "responses=experimental"
    assert h["accept"] == "text/event-stream"
    assert h["content-type"] == "application/json"


def test_body_has_required_codex_fields():
    p = _provider()
    body = p._build_responses_body(ChatRequest(messages=[Message.user("hi")]))

    assert body["store"] is False
    assert body["stream"] is True
    assert body["model"] == "gpt-5.5"
    assert isinstance(body["instructions"], str)
    assert isinstance(body["input"], list)
    assert body["include"] == ["reasoning.encrypted_content"]
    assert body["tool_choice"] == "auto"
    assert body["parallel_tool_calls"] is True

    assert len(body["input"]) == 1
    assert body["input"][0]["type"] == "message"
    assert body["input"][0]["role"] == "user"
    assert body["input"][0]["content"][0]["type"] == "input_text"
    assert body["input"][0]["content"][0]["text"] == "hi"

    assert "tools" not in body
    assert "reasoning" not in body
    assert "temperature" not in body


def test_empty_request_uses_default_instructions():
    p = _provider()
    body = p._build_responses_body(ChatRequest(messages=[Message.user("hi")]))
    assert body["instructions"] == "You are a helpful assistant."


def test_system_message_goes_to_instructions_not_input():
    p = _provider()
    req = ChatRequest(messages=[Message.system("You are a pirate."), Message.user("hi")])
    body = p._build_responses_body(req)

    assert body["instructions"] == "You are a pirate."
    assert len(body["input"]) == 1
    assert body["input"][0]["role"] == "user"


def test_system_field_used_for_instructions():
    p = _provider()
    req = ChatRequest(messages=[Message.user("hi")], system="be terse")
    body = p._build_responses_body(req)
    assert body["instructions"] == "be terse"


def test_system_blocks_take_priority_over_system_field():
    p = _provider()
    req = ChatRequest(
        messages=[Message.user("hi")],
        system="ignored",
        system_blocks=[SystemBlock.new("block one"), SystemBlock.new("block two")],
    )
    body = p._build_responses_body(req)
    assert body["instructions"] == "block one\n\nblock two"


def test_assistant_text_becomes_output_text_item():
    p = _provider()
    req = ChatRequest(messages=[Message.user("hi"), Message.assistant("hello there")])
    body = p._build_responses_body(req)

    assert len(body["input"]) == 2
    assert body["input"][1]["type"] == "message"
    assert body["input"][1]["role"] == "assistant"
    assert body["input"][1]["content"][0]["type"] == "output_text"
    assert body["input"][1]["content"][0]["text"] == "hello there"


def test_tool_call_and_result_serialize_as_function_items():
    p = _provider()
    tool = Tool(
        name="get_weather",
        description="Fetch the weather",
        input_schema={"type": "object", "properties": {"city": {"type": "string"}}},
    )
    req = ChatRequest(
        messages=[
            Message.user("weather in Paris?"),
            Message.assistant_with_tool_calls(
                "",
                [ToolCall(id="call_1", name="get_weather", input={"city": "Paris"})],
            ),
            Message.tool_result("call_1", "sunny, 21C"),
        ],
        tools=[tool],
    )
    body = p._build_responses_body(req)

    tools = body["tools"]
    assert len(tools) == 1
    assert tools[0]["type"] == "function"
    assert tools[0]["name"] == "get_weather"
    assert tools[0]["description"] == "Fetch the weather"
    assert isinstance(tools[0]["parameters"], dict)
    assert tools[0]["strict"] is None

    input_items = body["input"]
    assert len(input_items) == 3

    fc = input_items[1]
    assert fc["type"] == "function_call"
    assert fc["call_id"] == "call_1"
    assert fc["name"] == "get_weather"
    assert json.loads(fc["arguments"]) == {"city": "Paris"}

    out = input_items[2]
    assert out["type"] == "function_call_output"
    assert out["call_id"] == "call_1"
    assert out["output"] == "sunny, 21C"


def test_empty_tools_list_omits_tools_key():
    p = _provider()
    body = p._build_responses_body(ChatRequest(messages=[Message.user("hi")], tools=[]))
    assert "tools" not in body


def test_reasoning_and_temperature_are_conditional():
    p = _provider()
    req = ChatRequest(
        messages=[Message.user("hi")],
        temperature=0.3,
        provider_options={"reasoning_effort": "high"},
    )
    body = p._build_responses_body(req)

    assert body["temperature"] == 0.3
    assert body["reasoning"]["effort"] == "high"
    assert body["reasoning"]["summary"] == "auto"


def test_reasoning_absent_when_effort_not_a_string():
    p = _provider()
    req = ChatRequest(messages=[Message.user("hi")], provider_options={"reasoning_effort": 5})
    body = p._build_responses_body(req)
    assert "reasoning" not in body


def test_per_request_model_overrides_default():
    p = _provider()
    body = p._build_responses_body(ChatRequest(messages=[Message.user("hi")], model="gpt-override"))
    assert body["model"] == "gpt-override"
```

Run (expect collection failure — module/symbol does not exist yet):

```bash
uv run --project sdks/python pytest tests/test_chatgpt_codex_request.py
```

### T1.2 — Implement the complete provider file (green)

Create `sdks/python/motosan_ai/providers/chatgpt_codex.py` in full — the COMPLETE, FINAL file (imports,
constants, `_ChatGptCodexAdapterState`, `_parse_sse_event`, and the `ChatGptCodexProvider` class with
`__init__` / `_stream_url` / `_headers` / `_build_responses_body` / `_map_http_error` / `stream` /
`chat`). No stubs, no placeholders: every method is written to its final form here. T2 and T3 then ONLY
ADD their respective test files — they never re-edit `chatgpt_codex.py`. T1's test file
(`test_chatgpt_codex_request.py`) exercises only `__init__` / `_headers` / `_stream_url` /
`_build_responses_body`; T2 exercises `_parse_sse_event`; T3 exercises `stream()` / `chat()` over respx.
Writing the whole module in T1 (rather than stubbing) keeps the tree green and import-clean after every
task while honoring the no-placeholder rule.

Full FINAL file (write it in its entirety in T1; T2/T3 add tests only):

```python
from __future__ import annotations

import json
from collections.abc import AsyncIterator
from dataclasses import dataclass, field
from typing import Any

import httpx

from motosan_ai._stream_collect import collect_stream
from motosan_ai.error import (
    AuthError,
    NetworkError,
    ProviderError,
    RateLimitError,
    StreamError,
)
from motosan_ai.provider_base import BaseProvider, ProviderCapabilities
from motosan_ai.types import (
    ChatRequest,
    ChatResponse,
    Role,
    StopReason,
    StreamEvent,
    Usage,
)

_DEFAULT_BASE_URL = "https://chatgpt.com/backend-api/codex/responses"
_DEFAULT_MODEL = "gpt-5.5"
_ORIGINATOR = "codex_cli_rs"


@dataclass
class _ChatGptCodexAdapterState:
    seen_tool_ids: set[str] = field(default_factory=set)
    saw_tool_call: bool = False
    error: str | None = None


def _parse_sse_event(data: str, state: _ChatGptCodexAdapterState) -> list[StreamEvent]:
    """Map one decoded Responses SSE ``data`` payload to zero or more StreamEvents.

    Pure (apart from mutating ``state``). Port of Rust ``ChatGptCodexStreamAdapter::handle_event``.
    On a fatal ``error`` / ``response.failed`` frame this sets ``state.error`` and returns ``[]``;
    the caller (``stream()``) raises ``StreamError`` after draining.
    """
    s = data.strip()
    if not s or s == "[DONE]":
        return []
    try:
        chunk = json.loads(s)
    except json.JSONDecodeError:
        return []
    if not isinstance(chunk, dict):
        return []

    event_type = chunk.get("type")
    out: list[StreamEvent] = []

    if event_type == "response.output_text.delta":
        delta = chunk.get("delta")
        if isinstance(delta, str) and delta:
            out.append(StreamEvent(content=delta, done=False))

    elif event_type in ("response.reasoning_text.delta", "response.reasoning_summary_text.delta"):
        delta = chunk.get("delta")
        if isinstance(delta, str) and delta:
            out.append(StreamEvent(content=delta, done=False, event_type="thinking"))

    elif event_type == "response.output_item.added":
        item = chunk.get("item")
        if isinstance(item, dict) and item.get("type") == "function_call":
            call_id = item.get("call_id") or ""
            name = item.get("name") or ""
            if call_id:
                state.saw_tool_call = True
                state.seen_tool_ids.add(call_id)
                out.append(
                    StreamEvent(
                        content="",
                        done=False,
                        event_type="tool_call_start",
                        tool_call_id=call_id,
                        tool_call_name=name,
                    )
                )

    elif event_type == "response.function_call_arguments.delta":
        item_id = chunk.get("item_id") or ""
        delta = chunk.get("delta")
        if item_id and isinstance(delta, str):
            out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="tool_call_args",
                    tool_call_id=item_id,
                    tool_call_args_delta=delta,
                )
            )

    elif event_type == "response.output_item.done":
        item = chunk.get("item")
        if isinstance(item, dict) and item.get("type") == "function_call":
            call_id = item.get("call_id") or ""
            if call_id:
                out.append(
                    StreamEvent(
                        content="",
                        done=False,
                        event_type="tool_call_end",
                        tool_call_id=call_id,
                    )
                )

    elif event_type == "response.completed":
        response = chunk.get("response")
        response = response if isinstance(response, dict) else {}
        usage = response.get("usage")
        if isinstance(usage, dict):
            input_tokens = int(usage.get("input_tokens") or 0)
            output_tokens = int(usage.get("output_tokens") or 0)
            details = usage.get("input_tokens_details")
            cached = 0
            if isinstance(details, dict):
                cached = int(details.get("cached_tokens") or 0)
            out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="usage",
                    usage=Usage(
                        input_tokens=input_tokens,
                        output_tokens=output_tokens,
                        cache_creation_input_tokens=None,
                        cache_read_input_tokens=cached if cached > 0 else None,
                    ),
                )
            )
        status = response.get("status") or "completed"
        if state.saw_tool_call:
            stop = StopReason.tool_use
        elif status == "incomplete":
            stop = StopReason.max_tokens
        else:
            stop = StopReason.end_turn
        out.append(StreamEvent(content="", done=True, stop_reason=stop))

    elif event_type in ("error", "response.failed"):
        msg = chunk.get("message")
        if not isinstance(msg, str) or not msg:
            response = chunk.get("response")
            if isinstance(response, dict):
                err = response.get("error")
                if isinstance(err, dict) and isinstance(err.get("message"), str):
                    msg = err["message"]
        if not isinstance(msg, str) or not msg:
            err = chunk.get("error")
            if isinstance(err, dict) and isinstance(err.get("message"), str):
                msg = err["message"]
        if not isinstance(msg, str) or not msg:
            msg = "ChatGPT-backend stream error"
        state.error = msg

    return out


class ChatGptCodexProvider(BaseProvider):
    capabilities: ProviderCapabilities = ProviderCapabilities.text_only()

    def __init__(
        self,
        access_token: str,
        account_id: str,
        model: str | None = None,
        base_url: str | None = None,
    ) -> None:
        self.access_token = access_token
        self.account_id = account_id
        self.model = model or _DEFAULT_MODEL
        self.base_url = base_url or _DEFAULT_BASE_URL
        self._http = httpx.AsyncClient(timeout=120.0)

    def _stream_url(self) -> str:
        return self.base_url

    def _headers(self) -> dict[str, str]:
        return {
            "authorization": f"Bearer {self.access_token}",
            "chatgpt-account-id": self.account_id,
            "originator": _ORIGINATOR,
            "openai-beta": "responses=experimental",
            "accept": "text/event-stream",
            "content-type": "application/json",
        }

    def _build_responses_body(self, request: ChatRequest) -> dict[str, Any]:
        model = request.model or self.model

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

        input_items: list[dict[str, Any]] = []
        for message in request.messages:
            if message.role == Role.system:
                trimmed = message.content.strip()
                if trimmed:
                    instructions_parts.append(trimmed)
            elif message.role == Role.user:
                input_items.append(
                    {
                        "type": "message",
                        "role": "user",
                        "content": [{"type": "input_text", "text": message.content}],
                    }
                )
            elif message.role == Role.assistant:
                if message.content:
                    input_items.append(
                        {
                            "type": "message",
                            "role": "assistant",
                            "content": [{"type": "output_text", "text": message.content}],
                        }
                    )
                for tool_call in message.tool_calls:
                    input_items.append(
                        {
                            "type": "function_call",
                            "call_id": tool_call.id,
                            "name": tool_call.name,
                            "arguments": json.dumps(tool_call.input),
                        }
                    )
            elif message.role == Role.tool:
                if message.tool_call_id is not None:
                    input_items.append(
                        {
                            "type": "function_call_output",
                            "call_id": message.tool_call_id,
                            "output": message.content,
                        }
                    )

        instructions = (
            "\n\n".join(instructions_parts)
            if instructions_parts
            else "You are a helpful assistant."
        )

        body: dict[str, Any] = {
            "model": model,
            "store": False,
            "stream": True,
            "instructions": instructions,
            "input": input_items,
            "include": ["reasoning.encrypted_content"],
            "tool_choice": "auto",
            "parallel_tool_calls": True,
        }

        if request.tools is not None:
            mapped = [
                {
                    "type": "function",
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema,
                    "strict": None,
                }
                for tool in request.tools
            ]
            if mapped:
                body["tools"] = mapped

        if request.provider_options is not None:
            effort = request.provider_options.get("reasoning_effort")
            if isinstance(effort, str):
                body["reasoning"] = {"effort": effort, "summary": "auto"}

        if request.temperature is not None:
            body["temperature"] = request.temperature

        return body

    @staticmethod
    def _map_http_error(status: int, message: str) -> Exception:
        if status == 401:
            return AuthError(message)
        if status == 429:
            return RateLimitError(message)
        return ProviderError(message)

    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        self.validate_request(request)
        body = self._build_responses_body(request)
        try:
            resp = await self._http.send(
                self._http.build_request(
                    "POST", self._stream_url(), headers=self._headers(), json=body
                ),
                stream=True,
            )
        except httpx.HTTPError as exc:
            raise NetworkError(str(exc)) from exc

        try:
            if not resp.is_success:
                error_body = await resp.aread()
                raise self._map_http_error(resp.status_code, error_body.decode())

            state = _ChatGptCodexAdapterState()
            async for line in resp.aiter_lines():
                if not line.startswith("data: "):
                    continue
                data = line[len("data: ") :]
                for event in _parse_sse_event(data, state):
                    yield event
                    if event.done:
                        return
                if state.error is not None:
                    raise StreamError(state.error)
        finally:
            await resp.aclose()

    async def chat(self, request: ChatRequest) -> ChatResponse:
        response = await collect_stream(self.stream(request))
        return ChatResponse(
            content=response.content,
            tool_calls=response.tool_calls,
            model=request.model or self.model,
            usage=response.usage,
            stop_reason=response.stop_reason,
            thinking=response.thinking,
        )
```

> Implementation note: the file is written in full in T1 (it is the FINAL version). T2 and T3 only add
> their respective TEST files — they do NOT re-edit `chatgpt_codex.py`. This avoids any placeholder/stub
> intermediate state. The T1 test file exercises only `__init__`/`_headers`/`_stream_url`/
> `_build_responses_body`; T2 exercises `_parse_sse_event`; T3 exercises `stream()`/`chat()`.

Run green + format + lint + commit:

```bash
uv run --project sdks/python pytest tests/test_chatgpt_codex_request.py
uv run --project sdks/python ruff format motosan_ai/providers/chatgpt_codex.py tests/test_chatgpt_codex_request.py
uv run --project sdks/python ruff check motosan_ai/
git -C /Users/daiwanwei/Projects/wade/motosan-ai add sdks/python/motosan_ai/providers/chatgpt_codex.py sdks/python/tests/test_chatgpt_codex_request.py
git -C /Users/daiwanwei/Projects/wade/motosan-ai commit -m "feat(py-chatgpt-codex): add ChatGptCodexProvider request body builder"
```

(Commit message footer per CLAUDE.md: end with `Co-Authored-By: Claude Opus 4.8 (1M context)
<noreply@anthropic.com>`.)

**Done criteria for T1:** `tests/test_chatgpt_codex_request.py` passes; `ruff check motosan_ai/` clean;
`ruff format --check` clean.

---

## T2 — SSE `response.*` stream adapter unit tests

`_parse_sse_event` + `_ChatGptCodexAdapterState` already exist (written in T1). T2 ADDS only the adapter
unit-test file. These mirror the Rust `adapter_tests` module
(`adapter_emits_text_and_done`, `adapter_emits_usage_from_response_completed`,
`adapter_maps_reasoning_delta_to_thinking`, `adapter_handles_function_call_lifecycle`,
`adapter_maps_incomplete_to_max_tokens`, `adapter_surfaces_top_level_error`).

**File:** create `sdks/python/tests/test_chatgpt_codex_stream.py`.

The pure-parse tests feed JSON strings through `_parse_sse_event(json.dumps(frame), state)` and assert on
the returned `list[StreamEvent]` (no network). For the top-level-error surfacing, T2 only asserts the pure
function sets `state.error` and returns `[]`; the actual `StreamError` raise via `stream()` is covered by
an `@respx.mock` test in T3 (and one here too, since the harness is identical).

```python
from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai.error import StreamError
from motosan_ai.providers.chatgpt_codex import (
    ChatGptCodexProvider,
    _ChatGptCodexAdapterState,
    _parse_sse_event,
)
from motosan_ai.types import ChatRequest, Message, StopReason

_URL = "https://chatgpt.com/backend-api/codex/responses"

# Real text-delta frames mirroring the Rust TEXT_FRAMES fixture, plus a complete
# response.completed frame (usage + status).
TEXT_FRAMES = [
    {"type": "response.created", "response": {"id": "resp_1", "status": "in_progress"}},
    {"type": "response.output_text.delta", "delta": "Hi", "item_id": "msg_1"},
    {"type": "response.output_text.delta", "delta": " there", "item_id": "msg_1"},
    {"type": "response.output_text.delta", "delta": ",", "item_id": "msg_1"},
    {"type": "response.output_text.delta", "delta": " friend", "item_id": "msg_1"},
    {"type": "response.output_text.done", "item_id": "msg_1", "text": "Hi there, friend"},
    {
        "type": "response.completed",
        "response": {"id": "resp_1", "status": "completed",
                     "usage": {"input_tokens": 12, "output_tokens": 5}},
    },
]


def _sse_text(*payloads: dict) -> str:
    return "\n".join(f"data: {json.dumps(p)}" for p in payloads) + "\n"


def _drive(frames: list[dict]) -> list:
    state = _ChatGptCodexAdapterState()
    out = []
    for frame in frames:
        out.extend(_parse_sse_event(json.dumps(frame), state))
    return out


def test_empty_and_done_sentinel_return_no_events():
    state = _ChatGptCodexAdapterState()
    assert _parse_sse_event("", state) == []
    assert _parse_sse_event("[DONE]", state) == []


def test_malformed_json_skipped():
    assert _parse_sse_event("not json {", _ChatGptCodexAdapterState()) == []


def test_unknown_event_type_ignored():
    assert _parse_sse_event(json.dumps({"type": "response.in_progress"}), _ChatGptCodexAdapterState()) == []


def test_empty_text_delta_emits_nothing():
    state = _ChatGptCodexAdapterState()
    assert _parse_sse_event(json.dumps({"type": "response.output_text.delta", "delta": ""}), state) == []


def test_adapter_emits_text_and_done():
    events = _drive(TEXT_FRAMES)
    text = "".join(e.content for e in events if e.event_type == "text")
    assert text == "Hi there, friend"
    done = next(e for e in events if e.done)
    assert done.stop_reason == StopReason.end_turn


def test_adapter_emits_usage_from_response_completed():
    events = _drive(TEXT_FRAMES)
    usage = next(e.usage for e in events if e.event_type == "usage")
    assert usage is not None
    assert usage.input_tokens == 12
    assert usage.output_tokens == 5
    assert usage.cache_read_input_tokens is None


def test_adapter_surfaces_cached_tokens_as_is():
    state = _ChatGptCodexAdapterState()
    frame = {
        "type": "response.completed",
        "response": {
            "status": "completed",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 5,
                "input_tokens_details": {"cached_tokens": 30},
            },
        },
    }
    events = _parse_sse_event(json.dumps(frame), state)
    usage = next(e.usage for e in events if e.event_type == "usage")
    assert usage is not None
    # Surfaced as-is, NOT subtracted (input_tokens already counts the full prompt).
    assert usage.input_tokens == 100
    assert usage.cache_read_input_tokens == 30


def test_adapter_maps_reasoning_delta_to_thinking():
    events = _drive(
        [
            {"type": "response.reasoning_text.delta", "delta": "think "},
            {"type": "response.reasoning_summary_text.delta", "delta": "more"},
        ]
    )
    thinking = "".join(e.content for e in events if e.event_type == "thinking")
    assert thinking == "think more"


def test_adapter_handles_function_call_lifecycle():
    events = _drive(
        [
            {"type": "response.output_item.added",
             "item": {"type": "function_call", "call_id": "call_42", "name": "get_weather"}},
            {"type": "response.function_call_arguments.delta", "item_id": "call_42",
             "delta": '{"city":'},
            {"type": "response.function_call_arguments.delta", "item_id": "call_42",
             "delta": '"Paris"}'},
            {"type": "response.output_item.done",
             "item": {"type": "function_call", "call_id": "call_42", "name": "get_weather"}},
            {"type": "response.completed",
             "response": {"status": "completed", "usage": {"input_tokens": 3, "output_tokens": 7}}},
        ]
    )

    start = next(e for e in events if e.event_type == "tool_call_start")
    assert start.tool_call_id == "call_42"
    assert start.tool_call_name == "get_weather"

    args = "".join(
        e.tool_call_args_delta or "" for e in events if e.event_type == "tool_call_args"
    )
    assert args == '{"city":"Paris"}'

    end = next(e for e in events if e.event_type == "tool_call_end")
    assert end.tool_call_id == "call_42"

    done = next(e for e in events if e.done)
    assert done.stop_reason == StopReason.tool_use


def test_adapter_maps_incomplete_to_max_tokens():
    events = _drive(
        [
            {"type": "response.completed",
             "response": {"status": "incomplete", "usage": {"input_tokens": 1, "output_tokens": 1}}},
        ]
    )
    done = next(e for e in events if e.done)
    assert done.stop_reason == StopReason.max_tokens


def test_adapter_surfaces_top_level_error_sets_state():
    state = _ChatGptCodexAdapterState()
    events = _parse_sse_event(
        json.dumps({"type": "error", "message": "rate limited", "code": "rate_limit_exceeded"}),
        state,
    )
    assert events == []
    assert state.error == "rate limited"


def test_response_failed_reads_nested_error_message():
    state = _ChatGptCodexAdapterState()
    _parse_sse_event(
        json.dumps({"type": "response.failed", "response": {"error": {"message": "boom"}}}),
        state,
    )
    assert state.error == "boom"


def test_error_without_message_uses_fallback():
    state = _ChatGptCodexAdapterState()
    _parse_sse_event(json.dumps({"type": "error"}), state)
    assert state.error == "ChatGPT-backend stream error"


@respx.mock
@pytest.mark.asyncio
async def test_stream_raises_stream_error_on_error_frame():
    sse = _sse_text({"type": "error", "message": "rate limited"})
    respx.post(_URL).mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    with pytest.raises(StreamError, match="rate limited"):
        async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
            pass
```

Run, format, lint, commit:

```bash
uv run --project sdks/python pytest tests/test_chatgpt_codex_stream.py
uv run --project sdks/python ruff format tests/test_chatgpt_codex_stream.py
uv run --project sdks/python ruff check motosan_ai/
git -C /Users/daiwanwei/Projects/wade/motosan-ai add sdks/python/tests/test_chatgpt_codex_stream.py
git -C /Users/daiwanwei/Projects/wade/motosan-ai commit -m "test(py-chatgpt-codex): cover response.* SSE adapter mapping"
```

**Done criteria for T2:** all of `tests/test_chatgpt_codex_stream.py` passes (text/usage/thinking/tool
lifecycle/incomplete/error). The thinking test in particular confirms `event_type="thinking"` is the
literal that flows through.

---

## T3 — Provider `stream()` / `chat()` HTTP behavior (respx tests)

`stream()` and `chat()` already exist (written in T1). T3 ADDS the HTTP-level respx tests proving the
end-to-end POST: headers + body capture, 200 streaming, 401→AuthError, 429→RateLimitError, transport
error→NetworkError, and `chat()` collecting the stream (including thinking → `ChatResponse.thinking`).

**File:** create `sdks/python/tests/test_chatgpt_codex_http.py`.

```python
from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai.error import AuthError, NetworkError, ProviderError, RateLimitError
from motosan_ai.providers.chatgpt_codex import ChatGptCodexProvider
from motosan_ai.types import ChatRequest, Message, StopReason

_URL = "https://chatgpt.com/backend-api/codex/responses"


def _sse_text(*payloads: dict) -> str:
    return "\n".join(f"data: {json.dumps(p)}" for p in payloads) + "\n"


def _text_stream() -> str:
    return _sse_text(
        {"type": "response.output_text.delta", "delta": "Hello "},
        {"type": "response.output_text.delta", "delta": "world."},
        {"type": "response.completed",
         "response": {"status": "completed", "usage": {"input_tokens": 5, "output_tokens": 2}}},
    )


@respx.mock
@pytest.mark.asyncio
async def test_stream_yields_text_then_done():
    respx.post(_URL).mock(
        return_value=httpx.Response(200, text=_text_stream(),
                                    headers={"content-type": "text/event-stream"})
    )
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    events = [e async for e in p.stream(ChatRequest(messages=[Message.user("hi")]))]
    text = "".join(e.content for e in events if e.event_type == "text" and not e.done)
    assert text == "Hello world."
    assert events[-1].done is True
    assert events[-1].stop_reason == StopReason.end_turn


@respx.mock
@pytest.mark.asyncio
async def test_stream_sends_codex_headers_and_responses_body():
    captured = {}

    def _capture(request):
        captured["headers"] = dict(request.headers)
        captured["body"] = json.loads(request.content)
        return httpx.Response(200, text=_text_stream(),
                              headers={"content-type": "text/event-stream"})

    respx.post(_URL).mock(side_effect=_capture)
    async for _ in ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None).stream(
        ChatRequest(messages=[Message.user("hi")])
    ):
        pass

    h = captured["headers"]
    assert h["authorization"] == "Bearer tok"
    assert h["chatgpt-account-id"] == "acct-123"
    assert h["originator"] == "codex_cli_rs"
    assert h["openai-beta"] == "responses=experimental"
    assert h["accept"] == "text/event-stream"

    body = captured["body"]
    assert body["store"] is False
    assert body["stream"] is True
    assert body["model"] == "gpt-5.5"
    assert body["include"] == ["reasoning.encrypted_content"]
    assert body["input"][0]["type"] == "message"
    assert body["input"][0]["content"][0]["type"] == "input_text"


@respx.mock
@pytest.mark.asyncio
async def test_chat_collects_stream_into_response():
    respx.post(_URL).mock(
        return_value=httpx.Response(200, text=_text_stream(),
                                    headers={"content-type": "text/event-stream"})
    )
    resp = await ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None).chat(
        ChatRequest(messages=[Message.user("hi")])
    )
    assert resp.content == "Hello world."
    assert resp.usage.input_tokens == 5
    assert resp.usage.output_tokens == 2
    assert resp.model == "gpt-5.5"
    assert resp.stop_reason == StopReason.end_turn


@respx.mock
@pytest.mark.asyncio
async def test_chat_surfaces_thinking():
    sse = _sse_text(
        {"type": "response.reasoning_text.delta", "delta": "plan "},
        {"type": "response.reasoning_summary_text.delta", "delta": "ahead"},
        {"type": "response.output_text.delta", "delta": "done"},
        {"type": "response.completed", "response": {"status": "completed"}},
    )
    respx.post(_URL).mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    resp = await ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None).chat(
        ChatRequest(messages=[Message.user("hi")])
    )
    assert resp.content == "done"
    assert resp.thinking == "plan ahead"


@respx.mock
@pytest.mark.asyncio
async def test_chat_tool_call_lifecycle_yields_tool_call():
    sse = _sse_text(
        {"type": "response.output_item.added",
         "item": {"type": "function_call", "call_id": "c1", "name": "get_weather"}},
        {"type": "response.function_call_arguments.delta", "item_id": "c1", "delta": '{"city":'},
        {"type": "response.function_call_arguments.delta", "item_id": "c1", "delta": '"Paris"}'},
        {"type": "response.output_item.done",
         "item": {"type": "function_call", "call_id": "c1", "name": "get_weather"}},
        {"type": "response.completed", "response": {"status": "completed"}},
    )
    respx.post(_URL).mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    resp = await ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None).chat(
        ChatRequest(messages=[Message.user("weather?")])
    )
    assert resp.stop_reason == StopReason.tool_use
    assert len(resp.tool_calls) == 1
    assert resp.tool_calls[0].id == "c1"
    assert resp.tool_calls[0].name == "get_weather"
    assert resp.tool_calls[0].input == {"city": "Paris"}


@respx.mock
@pytest.mark.asyncio
async def test_stream_401_raises_auth_error():
    respx.post(_URL).mock(
        return_value=httpx.Response(401, json={"error": {"message": "expired token"}})
    )
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    with pytest.raises(AuthError):
        async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
            pass


@respx.mock
@pytest.mark.asyncio
async def test_stream_429_raises_rate_limit_error():
    respx.post(_URL).mock(return_value=httpx.Response(429, json={"error": {"message": "slow down"}}))
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    with pytest.raises(RateLimitError):
        async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
            pass


@respx.mock
@pytest.mark.asyncio
async def test_stream_500_raises_provider_error():
    respx.post(_URL).mock(return_value=httpx.Response(500, json={"error": {"message": "boom"}}))
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    with pytest.raises(ProviderError):
        async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
            pass


@respx.mock
@pytest.mark.asyncio
async def test_stream_transport_error_raises_network_error():
    respx.post(_URL).mock(side_effect=httpx.ConnectError("conn refused"))
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    with pytest.raises(NetworkError):
        async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
            pass
```

Run, format, lint, commit:

```bash
uv run --project sdks/python pytest tests/test_chatgpt_codex_http.py
uv run --project sdks/python ruff format tests/test_chatgpt_codex_http.py
uv run --project sdks/python ruff check motosan_ai/
git -C /Users/daiwanwei/Projects/wade/motosan-ai add sdks/python/tests/test_chatgpt_codex_http.py
git -C /Users/daiwanwei/Projects/wade/motosan-ai commit -m "test(py-chatgpt-codex): cover stream/chat HTTP behavior and error mapping"
```

**Done criteria for T3:** all `tests/test_chatgpt_codex_http.py` pass; headers verified; `chat()` collects
content/usage/thinking/tool_calls; 401/429/500/transport all map to the right error class.

---

## T4 — Client wiring + exports + dispatch tests + live test

### T4.1 — Failing dispatch test

**File:** create `sdks/python/tests/test_chatgpt_codex_dispatch.py`.

```python
from __future__ import annotations

import pytest

from motosan_ai import ChatGptCodexProvider, Client, Provider
from motosan_ai.error import ConfigError


def test_provider_enum_has_openai_chatgpt():
    assert Provider.openai_chatgpt == "openai_chatgpt"


def test_client_chatgpt_codex_classmethod():
    c = Client.chatgpt_codex(access_token="tok", account_id="acct-123", model="gpt-5.5")
    assert c.provider == Provider.openai_chatgpt
    assert isinstance(c._provider, ChatGptCodexProvider)
    assert c._provider.access_token == "tok"
    assert c._provider.account_id == "acct-123"
    assert c._provider.model == "gpt-5.5"
    # No api key required for this provider.
    assert c.api_key == ""


def test_client_chatgpt_codex_requires_access_token():
    with pytest.raises(ConfigError, match="access_token"):
        Client.chatgpt_codex(access_token=None, account_id="acct-123")


def test_client_chatgpt_codex_requires_account_id():
    with pytest.raises(ConfigError, match="account_id"):
        Client.chatgpt_codex(access_token="tok", account_id=None)


def test_chatgpt_codex_provider_exported_at_top_level():
    import motosan_ai

    assert "ChatGptCodexProvider" in motosan_ai.__all__
    assert motosan_ai.ChatGptCodexProvider is ChatGptCodexProvider
```

Run (expect failure: `Provider.openai_chatgpt` / `Client.chatgpt_codex` / top-level export do not exist):

```bash
uv run --project sdks/python pytest tests/test_chatgpt_codex_dispatch.py
```

### T4.2 — Wire `providers/__init__.py`

Edit `sdks/python/motosan_ai/providers/__init__.py`:
- Add import after the `claude_code` line (alphabetical — `chatgpt_codex` sorts before `claude_code`):
  `from .chatgpt_codex import ChatGptCodexProvider`
- Add `"ChatGptCodexProvider",` to `__all__` (it sorts after `"ApprovalMode"`, before `"ClaudeCodeClient"`).

Result (imports block + `__all__`):
```python
from .anthropic import AnthropicProvider
from .chatgpt_codex import ChatGptCodexProvider
from .claude_code import ClaudeCodeClient
from .codex_cli import CodexCliClient, LocalProvider, SandboxMode
from .gemini import GeminiProvider
from .gemini_cli import ApprovalMode, GeminiCliClient
from .gemini_code_assist import GeminiCodeAssistProvider
from .minimax import MinimaxProvider
from .ollama import OllamaProvider
from .openai import OpenAIProvider

__all__ = [
    "AnthropicProvider",
    "ApprovalMode",
    "ChatGptCodexProvider",
    "ClaudeCodeClient",
    "CodexCliClient",
    "GeminiCliClient",
    "GeminiCodeAssistProvider",
    "GeminiProvider",
    "LocalProvider",
    "MinimaxProvider",
    "OllamaProvider",
    "OpenAIProvider",
    "SandboxMode",
]
```

### T4.3 — Wire top-level `__init__.py`

Edit `sdks/python/motosan_ai/__init__.py`:
- In the `from motosan_ai.providers import (...)` block add `ChatGptCodexProvider,` (alphabetical — after
  `ApprovalMode`, before `ClaudeCodeClient`).
- In top-level `__all__` add `"ChatGptCodexProvider",` (sorts after `"ChatResponse"`, before
  `"ClaudeCodeClient"`).

Resulting provider import block:
```python
from motosan_ai.providers import (
    ApprovalMode,
    ChatGptCodexProvider,
    ClaudeCodeClient,
    CodexCliClient,
    GeminiCliClient,
    GeminiCodeAssistProvider,
    GeminiProvider,
    LocalProvider,
    SandboxMode,
)
```
And in `__all__`, insert `"ChatGptCodexProvider",` immediately after `"ChatResponse",`.

### T4.4 — Wire `client.py`

Edit `sdks/python/motosan_ai/client.py`:

(a) Provider import block (currently lines 12-20) — add `ChatGptCodexProvider`:
```python
from motosan_ai.providers import (
    AnthropicProvider,
    ChatGptCodexProvider,
    CodexCliClient,
    GeminiCliClient,
    GeminiCodeAssistProvider,
    GeminiProvider,
    MinimaxProvider,
    OpenAIProvider,
)
```

(b) `Provider` enum (currently lines 27-35) — add the variant:
```python
class Provider(StrEnum):
    anthropic = "anthropic"
    openai = "openai"
    minimax = "minimax"
    ollama = "ollama"
    gemini = "gemini"
    codex_cli = "codex_cli"
    gemini_cli = "gemini_cli"
    gemini_code_assist = "gemini_code_assist"
    openai_chatgpt = "openai_chatgpt"
```

(c) `Client.__init__` signature — add `account_id` next to `project_id` (line 64):
```python
        access_token: str | None = None,
        project_id: str | None = None,
        account_id: str | None = None,
```

(d) Dispatch branch — add a sibling to the `gemini_code_assist` branch (insert immediately after the
`gemini_code_assist` `if` block, before `elif provider_value == Provider.codex_cli:`):
```python
        elif provider_value == Provider.openai_chatgpt:
            if not access_token:
                raise ConfigError("openai_chatgpt requires access_token")
            if not account_id:
                raise ConfigError("openai_chatgpt requires account_id")
            self.api_key = ""
            self._provider = ChatGptCodexProvider(
                access_token=access_token,
                account_id=account_id,
                model=model,
                base_url=base_url,
            )
```

(e) Classmethod — add `Client.chatgpt_codex(...)` (place it right after the `gemini_code_assist`
classmethod, before `minimax`):
```python
    @classmethod
    def chatgpt_codex(
        cls,
        access_token: str | None = None,
        account_id: str | None = None,
        model: str | None = None,
        base_url: str | None = None,
        max_retries: int = 3,
    ) -> Client:
        return cls(
            provider=Provider.openai_chatgpt,
            access_token=access_token,
            account_id=account_id,
            model=model,
            base_url=base_url,
            max_retries=max_retries,
        )
```

(f) `_load_api_key` — NO change. The `openai_chatgpt` branch sets `self.api_key = ""` and returns from
`__init__` before reaching the `else` that calls `_load_api_key` (exactly like `gemini_code_assist`).

> Note the deliberate name asymmetry (matches Rust + the research contract): the enum value is
> `openai_chatgpt` (mirrors Rust `Provider::OpenAiChatGpt`) but the classmethod is `chatgpt_codex` and the
> provider class is `ChatGptCodexProvider`. The dispatch test asserts both ends.

### T4.5 — Run dispatch test green

```bash
uv run --project sdks/python pytest tests/test_chatgpt_codex_dispatch.py
uv run --project sdks/python ruff format motosan_ai/ tests/test_chatgpt_codex_dispatch.py
uv run --project sdks/python ruff check motosan_ai/
```

### T4.6 — Skip-guarded live test

**File:** create `sdks/python/tests/integration/test_chatgpt_codex_live.py`. Mirror
`tests/integration/test_code_assist_live.py`'s skip-guard shape. Token + account id come from env (no
OAuth flow in scope).

```python
"""Live integration test for ChatGptCodexProvider.

Skip unless ``MOTOSAN_RUN_CHATGPT_CODEX_LIVE=1`` and both
``CHATGPT_CODEX_ACCESS_TOKEN`` and ``CHATGPT_CODEX_ACCOUNT_ID`` are set in the
environment (the provider takes a pre-obtained token; there is no OAuth flow).
Optionally override the model via ``CHATGPT_CODEX_MODEL`` (default ``gpt-5.5``).
"""

from __future__ import annotations

import os

import pytest

from motosan_ai.providers.chatgpt_codex import ChatGptCodexProvider
from motosan_ai.types import ChatRequest, Message

_RUN = os.environ.get("MOTOSAN_RUN_CHATGPT_CODEX_LIVE") == "1"
_TOKEN = os.environ.get("CHATGPT_CODEX_ACCESS_TOKEN")
_ACCOUNT = os.environ.get("CHATGPT_CODEX_ACCOUNT_ID")
_MODEL = os.environ.get("CHATGPT_CODEX_MODEL", "gpt-5.5")

pytestmark = [
    pytest.mark.skipif(not _RUN, reason="set MOTOSAN_RUN_CHATGPT_CODEX_LIVE=1 to run"),
    pytest.mark.skipif(_TOKEN is None, reason="CHATGPT_CODEX_ACCESS_TOKEN not set"),
    pytest.mark.skipif(_ACCOUNT is None, reason="CHATGPT_CODEX_ACCOUNT_ID not set"),
    pytest.mark.asyncio,
]


@pytest.fixture
def provider() -> ChatGptCodexProvider:
    assert _TOKEN is not None and _ACCOUNT is not None
    return ChatGptCodexProvider(access_token=_TOKEN, account_id=_ACCOUNT, model=_MODEL)


async def test_live_chat_basic(provider: ChatGptCodexProvider):
    resp = await provider.chat(ChatRequest(messages=[Message.user("Reply with exactly: PONG")]))
    assert "PONG" in resp.content


async def test_live_stream_emits_text_then_done(provider: ChatGptCodexProvider):
    events = []
    async for event in provider.stream(
        ChatRequest(messages=[Message.user("Reply with: STREAM_OK")])
    ):
        events.append(event)
    text = "".join(e.content for e in events if e.event_type == "text" and not e.done)
    assert "STREAM_OK" in text
    assert events[-1].done is True
```

Verify it is collected-and-skipped (not errored) without the env vars:

```bash
uv run --project sdks/python pytest tests/integration/test_chatgpt_codex_live.py -v
# expect: 2 skipped
```

### T4.7 — Full suite green + format + lint + commit

```bash
uv run --project sdks/python pytest
uv run --project sdks/python ruff format motosan_ai/ tests/
uv run --project sdks/python ruff format --check
uv run --project sdks/python ruff check motosan_ai/
git -C /Users/daiwanwei/Projects/wade/motosan-ai add sdks/python/motosan_ai/providers/__init__.py sdks/python/motosan_ai/__init__.py sdks/python/motosan_ai/client.py sdks/python/tests/test_chatgpt_codex_dispatch.py sdks/python/tests/integration/test_chatgpt_codex_live.py
git -C /Users/daiwanwei/Projects/wade/motosan-ai commit -m "feat(py-chatgpt-codex): wire Provider.openai_chatgpt + Client.chatgpt_codex"
```

**Done criteria for T4:** `Provider.openai_chatgpt == "openai_chatgpt"`; `Client.chatgpt_codex(...)` builds
a `ChatGptCodexProvider` with `api_key == ""`; missing `access_token` / `account_id` raise `ConfigError`;
`motosan_ai.ChatGptCodexProvider` importable and in `__all__`; full `uv run pytest` green (live test
skipped); `ruff check motosan_ai/` clean; `ruff format --check` clean.

---

## T5 — Release 0.14.0

Additive, non-breaking. Files per the project Release Checklist (CLAUDE.md): `pyproject.toml`,
`CHANGELOG.md`, `AGENTS.md`, `llms.txt`, `skills/motosan-ai/SKILL.md`, plus `uv.lock`.

### T5.1 — `sdks/python/pyproject.toml`

Bump line 3: `version = "0.12.1"` → `version = "0.14.0"`.

### T5.2 — `sdks/python/CHANGELOG.md`

Insert a new top entry above `## [0.12.1] - 2026-05-29`:
```markdown
## [0.14.0] - 2026-06-23

### Added
- **ChatGPT-backend Codex provider** (`ChatGptCodexProvider`, `Provider.openai_chatgpt`,
  `Client.chatgpt_codex(access_token, account_id, model)`): native inference against the OpenAI
  **Responses API** at `https://chatgpt.com/backend-api/codex/responses` using a pre-obtained ChatGPT
  OAuth bearer token + `chatgpt-account-id` + the codex CLI headers. Streams typed `response.*` SSE
  events (text, reasoning → thinking, function-call tool lifecycle, usage, terminal stop reason). Text-only
  (`ProviderCapabilities.text_only()`); no `api_key` required. Mirrors the Rust
  `ChatGptCodexProvider`.
```
(0.13.0 was never released on PyPI for Python; this release skips straight from 0.12.1 to 0.14.0 per the
release instruction — see "Spec gaps".)

### T5.3 — `AGENTS.md`

- Line 5 `Rust v0.20.0 · Python v0.12.1 (PyPI)` → `Rust v0.20.0 · Python v0.14.0 (PyPI)`.
- HTTP-providers table row (the "HTTP providers" row in the "Where To Find Things" table): append
  `chatgpt_codex.py` to the Python list, e.g. `... Python: ... gemini.py, gemini_code_assist.py,
  chatgpt_codex.py`.

### T5.4 — `llms.txt`

- Line 5 `- Python 0.12.1 · Rust 0.20.0` → `- Python 0.14.0 · Rust 0.20.0`.
- Line 200 provider-variants list: append ` | `ChatGptCodex`` to
  `... | `GeminiCodeAssist` | `ClaudeCode` | `CodexCli` | `GeminiCli``.
- Add a provider-catalog paragraph after the `GeminiCodeAssist` paragraph (~line 204):
  ```
  `ChatGptCodex` (Python v0.14.0 `ChatGptCodexProvider` / `Provider.openai_chatgpt` /
  `Client.chatgpt_codex(access_token, account_id, model)`) — HTTP client for the OpenAI **Responses API**
  at `chatgpt.com/backend-api/codex/responses`. Pre-obtained ChatGPT OAuth bearer token + `chatgpt-account-id`
  + codex CLI headers (`originator: codex_cli_rs`, `openai-beta: responses=experimental`). No `api_key`.
  Default model `gpt-5.5`. Text-only. `chat()` = `stream()` + collect (no non-streaming endpoint).
  Reasoning effort via `provider_options["reasoning_effort"]`. Mirrors the Rust `ChatGptCodexProvider`.
  ```

### T5.5 — `skills/motosan-ai/SKILL.md`

- Line 8 `Multi-provider LLM SDK — Python 0.12.1 / Rust 0.20.0 / TypeScript 0.10.0` → `... Python 0.14.0 ...`.
- Line 10 providers list: append `, ChatGPT Codex (Responses API)`.
- Add a bullet near the Gemini HTTP-providers bullet (~line 135):
  ```
  - **ChatGPT Codex provider** (Python v0.14.0): `Client.chatgpt_codex(access_token, account_id, model)` /
    `Provider.openai_chatgpt` / `ChatGptCodexProvider` — native inference via the OpenAI Responses API at
    `chatgpt.com/backend-api/codex/responses` with a pre-obtained ChatGPT OAuth bearer token +
    `chatgpt-account-id` + codex CLI headers. No `api_key`. Text-only, default model `gpt-5.5`, `chat()` =
    `stream()` + collect.
  ```

### T5.6 — `uv.lock`

The version bump changes the project's own pinned version; resync:
```bash
uv lock --project sdks/python
```
(If `uv.lock` for the Python project lives at `sdks/python/uv.lock`, this updates it. If there is no lock
file, skip — confirm with `git status` whether a lock changed.)

### T5.7 — Final gates + commit

```bash
uv run --project sdks/python ruff check motosan_ai/
uv run --project sdks/python ruff format --check
uv run --project sdks/python pytest
git -C /Users/daiwanwei/Projects/wade/motosan-ai add sdks/python/pyproject.toml sdks/python/CHANGELOG.md AGENTS.md llms.txt skills/motosan-ai/SKILL.md
# add sdks/python/uv.lock too if it changed
git -C /Users/daiwanwei/Projects/wade/motosan-ai commit -m "chore(py-chatgpt-codex): release motosan-ai python 0.14.0"
```

### Done-gate greps (must all return a hit)

```bash
grep -n 'version = "0.14.0"' sdks/python/pyproject.toml
grep -n '0.14.0' sdks/python/CHANGELOG.md
grep -n 'openai_chatgpt' sdks/python/motosan_ai/client.py
grep -n 'ChatGptCodexProvider' sdks/python/motosan_ai/__init__.py sdks/python/motosan_ai/providers/__init__.py
grep -rn 'ChatGptCodex' AGENTS.md llms.txt skills/motosan-ai/SKILL.md
grep -n 'Python v0.14.0\|Python 0.14.0' AGENTS.md llms.txt
grep -n 'Python 0.14.0' skills/motosan-ai/SKILL.md
```

---

## Open PR

After T5, open the PR (per MEMORY.md, all `.py`/packaging changes go through PR + CI):
```bash
git -C /Users/daiwanwei/Projects/wade/motosan-ai push -u origin feat/py-chatgpt-codex
gh pr create --title "feat(python): ChatGPT-backend Codex provider (0.14.0)" --body "<summary + 🤖 footer>"
```

---

## Spec gaps (could not fill / deliberately deviated)

1. **No provider-level default reasoning-effort field/setter.** The task's T3 line asks for a
   "provider-level default reasoning effort setter," but the Rust source of truth
   (`sdks/rust/src/providers/chatgpt_codex.rs`) has NO such field: the struct's only fields are `http,
   access_token, account_id, model, base_url, retry_policy`, and reasoning effort comes SOLELY from
   `req.provider_options["reasoning_effort"]`. `new(...)` takes only `(access_token, account_id, model,
   base_url)`. Adding a provider default would DIVERGE from the Rust contract, so this plan does NOT add
   one. **Decision needed from orchestrator** if a provider-level default is genuinely wanted (it would be
   a Python-only extension with no Rust counterpart). The plan implements effort exactly as Rust does.

2. **Version baseline is 0.12.1, not 0.13.0.** The task premise ("now v0.13.0") is false in this checkout
   (`pyproject.toml:3` = `0.12.1`; `AGENTS.md:5`, `llms.txt:5`, `SKILL.md:8` all say `0.12.1`). The task
   explicitly orders a 0.14.0 release, so this plan bumps `0.12.1 → 0.14.0` directly (0.13.0 is skipped on
   PyPI). This is harmless for an additive release. If the orchestrator instead wants strict +1 semver
   (0.12.1 → 0.13.0), every `0.14.0` in T5 becomes `0.13.0` — a mechanical substitution. **Flagged, not
   blocked**; defaulting to 0.14.0 per the explicit instruction.

3. **`StreamEvent.session_id` does not exist.** The task premise ("StreamEvent now has session_id from
   0.13.0") is false (`types.py:349-358` has no such field). Not needed — an HTTP provider never sets a
   session id, and the Rust adapter does not either. No action.

4. **Reasoning event_type is `"thinking"`, NOT `"thinking_delta"`.** One research note claimed the adapter
   must emit `event_type="thinking_delta"` for `collect_stream` to fill `ChatResponse.thinking`. That note
   is WRONG: `_stream_collect.py:32` keys on the literal string `"thinking"`. The plan emits `"thinking"`
   (verified against the actual `collect_stream` source); `"thinking_delta"` would silently drop reasoning.
   Resolved in-plan — no open gap, but called out because it contradicts an input contract.

5. **No retry loop inside the provider.** Rust's `stream()` has a retry loop (network/5xx backoff). The
   Python analog `gemini_code_assist.py` does NOT implement a provider-internal retry; retries are handled
   one layer up by `Client.stream_with` / `with_retry`. This plan matches the Python analog (no
   provider-internal retry) — consistent with the existing Python architecture, not a divergence from
   Python idiom. The `RetryPolicy` / `with_retry_policy` Rust surface has no Python equivalent and is out
   of scope.
