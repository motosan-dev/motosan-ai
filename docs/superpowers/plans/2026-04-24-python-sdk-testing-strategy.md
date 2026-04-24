# Python SDK Testing Strategy Implementation Plan

> **Status:** ✅ **COMPLETE (2026-04-24)** — shipped as `motosan-ai` v0.8.1.
>
> | Metric | Result |
> |--------|--------|
> | Tests | **311 passed**, 15 skipped (live-only) |
> | Lint / format | clean (one import-sort autofix applied) |
> | Snapshots | **27 JSON files** under `tests/snapshots/` |
> | New test files | `_snapshots.py` + `parity/` (4 files) + `test_client_integration.py` + `test_openai_vision.py` + `test_snapshots_helper.py` |
> | Nightly CI | `.github/workflows/ci-python-nightly.yml` added |
> | Bonus: code-review follow-ups | OpenAI vision (`image_url` wire format) + Gemini `has_text_block` guard removed + `Retry-After` header parsing wired into both Anthropic & Gemini |
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add drift-detection infrastructure that catches wire-format regressions the moment they happen — golden-file snapshots, cross-provider parity matrix, OpenAI vision coverage, Client-level integration tests, and a nightly live-test CI job.

**Architecture:** Two new test directories — `tests/snapshots/` (JSON files checked into git) and `tests/parity/` (one parametrized test file per feature that iterates all 4 HTTP providers). A tiny `_snapshots.py` helper implements read/write/compare logic with an `UPDATE_SNAPSHOTS=1` env-var to regenerate. Client-level integration tests land in `tests/test_client_integration.py`. Nightly live workflow is a new `.github/workflows/ci-python-nightly.yml`.

**Tech Stack:** Python 3.11+, `pytest`, `pytest-asyncio`, `respx`, stdlib `json`. No new dependencies.

---

## Reference material

- **Current Python tests:** `sdks/python/tests/` — 19 test files, 256 passing.
- **Current CI:** `.github/workflows/ci-python.yml` — runs `ruff` + `pytest -q` on push/PR.
- **Rust parity pattern:** `sdks/rust/tests/tool_choice_anthropic.rs` / `tool_choice_openai.rs` — per-provider regex body matchers. Python will use full-body JSON snapshots instead (stricter, catches field-addition bugs regex would miss).
- **OpenAI vision wire format (from Rust):** `sdks/rust/src/providers/openai.rs:342-360` — `{"type": "image_url", "image_url": {"url": "data:MIME;base64,DATA"}}` for base64, `{"url": URL}` for URL source.

---

## File Structure

| Path | Responsibility | Status |
|------|----------------|--------|
| `sdks/python/tests/_snapshots.py` | Snapshot helper — `assert_snapshot(name, obj)`; writes on first run; `UPDATE_SNAPSHOTS=1` regenerates | **Create** |
| `sdks/python/tests/snapshots/` | JSON snapshot files, one per assertion name | **Create** |
| `sdks/python/tests/parity/__init__.py` | Package marker | **Create** |
| `sdks/python/tests/parity/conftest.py` | Provider matrix fixture, canonical `ChatRequest` builders | **Create** |
| `sdks/python/tests/parity/test_simple_chat_parity.py` | Plain text chat body shape across 4 providers | **Create** |
| `sdks/python/tests/parity/test_tool_choice_parity.py` | All 4 `ToolChoice` variants × 4 providers | **Create** |
| `sdks/python/tests/parity/test_vision_parity.py` | Vision serialization across Anthropic/OpenAI/Gemini | **Create** |
| `sdks/python/tests/parity/test_stream_contract.py` | Stream event ordering contract | **Create** |
| `sdks/python/tests/test_client_integration.py` | Provider dispatch matrix, retry end-to-end, env-var fallback | **Create** |
| `sdks/python/motosan_ai/providers/openai.py` | Add `content_blocks` handling in `_serialize_messages` | **Modify** |
| `.github/workflows/ci-python-nightly.yml` | Nightly live-test workflow with secrets | **Create** |
| `devshell/scripts.nix` | Update `check-python` to include `tests/parity/` phase | **Modify** |
| `sdks/python/CHANGELOG.md` | v0.8.1 entry (test infra + OpenAI vision fix) | **Modify** |
| `sdks/python/pyproject.toml` | Version bump 0.8.0 → 0.8.1 | **Modify** |

Design principles:
- **Snapshot over regex.** Rust uses regex body matchers because `mockito` doesn't do structural matching. `respx` does, and stored JSON files catch field-addition bugs a regex would miss.
- **Parametrized over per-file.** One `test_tool_choice_parity.py` iterates `@pytest.mark.parametrize("provider_name", [...])` rather than 4 separate files. Less duplication, easier to add a 5th provider.
- **Snapshots are code review artifacts.** Diff on PRs shows exactly what the wire format changed. Treat snapshot changes the way you treat schema migrations.
- **No hidden state.** `UPDATE_SNAPSHOTS=1` is the only way to regenerate. Tests fail otherwise.

---

## Task 1: Snapshot helper

**Files:**
- Create: `sdks/python/tests/_snapshots.py`
- Create: `sdks/python/tests/test_snapshots_helper.py` (temporary — will be deleted after Task 2 proves the helper works in anger)

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_snapshots_helper.py`:

```python
from __future__ import annotations

import json
import os
from pathlib import Path

import pytest

from tests._snapshots import assert_snapshot, snapshot_path


def test_snapshot_path_resolves_under_tests_snapshots():
    p = snapshot_path("my_test")
    assert p.name == "my_test.json"
    assert p.parent.name == "snapshots"


def test_assert_snapshot_writes_on_first_run(tmp_path, monkeypatch):
    monkeypatch.setenv("UPDATE_SNAPSHOTS", "1")
    monkeypatch.setattr(
        "tests._snapshots.SNAPSHOT_DIR", tmp_path
    )
    assert_snapshot("simple_text", {"role": "user", "content": "hi"})
    saved = json.loads((tmp_path / "simple_text.json").read_text())
    assert saved == {"role": "user", "content": "hi"}


def test_assert_snapshot_passes_on_match(tmp_path, monkeypatch):
    monkeypatch.setattr("tests._snapshots.SNAPSHOT_DIR", tmp_path)
    (tmp_path / "match.json").write_text(
        json.dumps({"k": "v"}, indent=2, sort_keys=True)
    )
    assert_snapshot("match", {"k": "v"})  # should not raise


def test_assert_snapshot_fails_on_diff(tmp_path, monkeypatch):
    monkeypatch.setattr("tests._snapshots.SNAPSHOT_DIR", tmp_path)
    (tmp_path / "diff.json").write_text(
        json.dumps({"k": "old"}, indent=2, sort_keys=True)
    )
    with pytest.raises(AssertionError, match="snapshot mismatch"):
        assert_snapshot("diff", {"k": "new"})


def test_update_env_var_overwrites_existing(tmp_path, monkeypatch):
    monkeypatch.setenv("UPDATE_SNAPSHOTS", "1")
    monkeypatch.setattr("tests._snapshots.SNAPSHOT_DIR", tmp_path)
    (tmp_path / "over.json").write_text("{}")
    assert_snapshot("over", {"new": True})
    saved = json.loads((tmp_path / "over.json").read_text())
    assert saved == {"new": True}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_snapshots_helper.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'tests._snapshots'`.

- [ ] **Step 3: Implement helper**

Create `sdks/python/tests/_snapshots.py`:

```python
"""Snapshot-testing helper.

Stores a JSON-serialized payload under tests/snapshots/<name>.json. On
subsequent runs, compares the stored payload against the new value and raises
AssertionError on mismatch. Regenerate with UPDATE_SNAPSHOTS=1.

Snapshots are a code-review artifact: every diff is a deliberate wire-format
change. Treat updates the way you treat schema migrations.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any

SNAPSHOT_DIR = Path(__file__).parent / "snapshots"


def snapshot_path(name: str) -> Path:
    return SNAPSHOT_DIR / f"{name}.json"


def assert_snapshot(name: str, value: Any) -> None:
    path = snapshot_path(name)
    serialized = json.dumps(value, indent=2, sort_keys=True)
    update = os.environ.get("UPDATE_SNAPSHOTS") == "1"

    if update or not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(serialized + "\n")
        return

    stored = path.read_text().rstrip("\n")
    if stored != serialized:
        raise AssertionError(
            f"snapshot mismatch for {name}\n"
            f"  path:     {path}\n"
            f"  stored:   {stored[:200]}...\n"
            f"  received: {serialized[:200]}...\n"
            f"  regenerate with: UPDATE_SNAPSHOTS=1 pytest {path.stem}"
        )
```

Create an empty `sdks/python/tests/__init__.py` if it doesn't exist (required so `from tests._snapshots` imports resolve):

```bash
touch sdks/python/tests/__init__.py
```

Update `sdks/python/pyproject.toml` to mark tests as a package so relative imports work:

```toml
[tool.pytest.ini_options]
addopts = "-q"
asyncio_mode = "auto"
pythonpath = ["."]
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_snapshots_helper.py -v`
Expected: PASS — 5 tests.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/tests/_snapshots.py sdks/python/tests/__init__.py sdks/python/tests/test_snapshots_helper.py sdks/python/pyproject.toml
git commit -m "test(python): add snapshot helper for wire-format drift detection"
```

---

## Task 2: Parity matrix fixture

**Files:**
- Create: `sdks/python/tests/parity/__init__.py`
- Create: `sdks/python/tests/parity/conftest.py`

Provides a parametrized `(provider_name, provider, mock_url)` fixture plus helpers that build canonical `ChatRequest`s and intercept the outgoing HTTP body.

- [ ] **Step 1: Create package marker**

```bash
touch sdks/python/tests/parity/__init__.py
```

- [ ] **Step 2: Write `conftest.py`**

Create `sdks/python/tests/parity/conftest.py`:

```python
"""Shared fixtures for cross-provider parity tests.

Each parity test parametrizes over (provider_name, provider, endpoint_url).
The provider is already wired to mock its endpoint base via `base_url`.
Tests use respx to intercept and capture the outgoing HTTP body, then
snapshot-compare against a stored JSON file under tests/snapshots/.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Any

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.providers.minimax import MinimaxProvider
from motosan_ai.providers.openai import OpenAIProvider


@dataclass
class ProviderUnderTest:
    name: str
    provider: Any
    endpoint: str
    stream_endpoint: str
    ok_response: dict


_OK_ANTHROPIC = {
    "model": "claude-sonnet-4-6",
    "stop_reason": "end_turn",
    "usage": {"input_tokens": 1, "output_tokens": 1},
    "content": [{"type": "text", "text": "ok"}],
}

_OK_OPENAI = {
    "id": "chatcmpl-1",
    "object": "chat.completion",
    "model": "gpt-4o",
    "choices": [
        {
            "message": {"role": "assistant", "content": "ok"},
            "finish_reason": "stop",
            "index": 0,
        }
    ],
    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
}

_OK_GEMINI = {
    "candidates": [
        {"content": {"parts": [{"text": "ok"}]}, "finishReason": "STOP"}
    ],
    "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1},
    "modelVersion": "gemini-2.0-flash",
}

_OK_MINIMAX = {
    "id": "msg_1",
    "choices": [
        {
            "message": {"role": "assistant", "content": "ok"},
            "finish_reason": "stop",
            "index": 0,
        }
    ],
    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
}


@pytest.fixture(
    params=["anthropic", "openai", "gemini", "minimax"],
    ids=["anthropic", "openai", "gemini", "minimax"],
)
def provider_under_test(request) -> ProviderUnderTest:
    name = request.param
    if name == "anthropic":
        p = AnthropicProvider("test-key", base_url="https://mock.anthropic.com")
        return ProviderUnderTest(
            name=name,
            provider=p,
            endpoint="https://mock.anthropic.com/v1/messages",
            stream_endpoint="https://mock.anthropic.com/v1/messages",
            ok_response=_OK_ANTHROPIC,
        )
    if name == "openai":
        p = OpenAIProvider("test-key", base_url="https://mock.openai.com")
        return ProviderUnderTest(
            name=name,
            provider=p,
            endpoint="https://mock.openai.com/v1/chat/completions",
            stream_endpoint="https://mock.openai.com/v1/chat/completions",
            ok_response=_OK_OPENAI,
        )
    if name == "gemini":
        p = GeminiProvider("test-key", base_url="https://mock.gemini.com")
        return ProviderUnderTest(
            name=name,
            provider=p,
            endpoint=(
                "https://mock.gemini.com/models/gemini-2.0-flash:generateContent"
            ),
            stream_endpoint=(
                "https://mock.gemini.com/models/gemini-2.0-flash"
                ":streamGenerateContent?alt=sse"
            ),
            ok_response=_OK_GEMINI,
        )
    if name == "minimax":
        p = MinimaxProvider("test-key", base_url="https://mock.minimax.com")
        return ProviderUnderTest(
            name=name,
            provider=p,
            endpoint="https://mock.minimax.com/v1/text/chatcompletion_v2",
            stream_endpoint="https://mock.minimax.com/v1/text/chatcompletion_v2",
            ok_response=_OK_MINIMAX,
        )
    raise AssertionError(f"unknown provider {name}")


async def capture_chat_body(p: ProviderUnderTest, request) -> dict:
    """Runs a chat() call through respx mock and returns the JSON body sent."""
    route = respx.post(p.endpoint).mock(
        return_value=httpx.Response(200, json=p.ok_response)
    )
    await p.provider.chat(request)
    return json.loads(route.calls[0].request.content)
```

- [ ] **Step 3: Commit**

```bash
git add sdks/python/tests/parity/
git commit -m "test(python): add parity matrix fixture for cross-provider tests"
```

---

## Task 3: Parity test — simple text chat

**Files:**
- Create: `sdks/python/tests/parity/test_simple_chat_parity.py`

- [ ] **Step 1: Write failing test**

Create `sdks/python/tests/parity/test_simple_chat_parity.py`:

```python
"""Simple text-chat wire format across all HTTP providers.

Each provider's outgoing request body is snapshot-compared against a stored
JSON file. Any drift — field addition, renaming, role mapping change — fails
the test and shows the diff in PR review.
"""

from __future__ import annotations

import pytest
import respx

from motosan_ai.types import ChatRequest, Message

from tests._snapshots import assert_snapshot
from tests.parity.conftest import ProviderUnderTest, capture_chat_body


@respx.mock
@pytest.mark.asyncio
async def test_simple_user_message_body(provider_under_test: ProviderUnderTest):
    req = ChatRequest(messages=[Message.user("Hello")])
    body = await capture_chat_body(provider_under_test, req)
    assert_snapshot(f"simple_user_{provider_under_test.name}", body)


@respx.mock
@pytest.mark.asyncio
async def test_multi_turn_with_system(provider_under_test: ProviderUnderTest):
    req = ChatRequest(
        messages=[
            Message.user("q1"),
            Message.assistant("a1"),
            Message.user("q2"),
        ],
        system="Be concise.",
        temperature=0.2,
        max_tokens=100,
    )
    body = await capture_chat_body(provider_under_test, req)
    assert_snapshot(f"multi_turn_system_{provider_under_test.name}", body)
```

- [ ] **Step 2: Run to generate snapshots**

Run: `cd sdks/python && UPDATE_SNAPSHOTS=1 uv run pytest tests/parity/test_simple_chat_parity.py -v`
Expected: PASS — 8 tests (2 tests × 4 providers). 8 JSON files written under `tests/snapshots/`.

- [ ] **Step 3: Visually review snapshots**

Run: `ls sdks/python/tests/snapshots/ && cat sdks/python/tests/snapshots/simple_user_anthropic.json`
Expected: files like `simple_user_anthropic.json`, `simple_user_openai.json`, etc. Each should show the provider-native body shape (Anthropic = `messages` array, OpenAI = `messages` with system in array, Gemini = `contents` + `systemInstruction`, MiniMax = OpenAI-like).

If any snapshot looks wrong, fix the provider code — **do not edit the snapshot by hand.**

- [ ] **Step 4: Run without UPDATE_SNAPSHOTS to verify determinism**

Run: `cd sdks/python && uv run pytest tests/parity/test_simple_chat_parity.py -v`
Expected: PASS — snapshots match on second run.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/tests/parity/test_simple_chat_parity.py sdks/python/tests/snapshots/
git commit -m "test(python): add simple-chat parity snapshots across 4 providers"
```

---

## Task 4: Parity test — `ToolChoice` matrix

**Files:**
- Create: `sdks/python/tests/parity/test_tool_choice_parity.py`

Tests all 4 `ToolChoice` variants × 4 providers = 16 snapshots. Reveals mapping bugs (e.g. Anthropic `required`→`any`, Gemini `required`→`ANY` uppercase, OpenAI `required`→`required`).

- [ ] **Step 1: Write failing test**

Create `sdks/python/tests/parity/test_tool_choice_parity.py`:

```python
from __future__ import annotations

import pytest
import respx

from motosan_ai.types import ChatRequest, Message, Tool, ToolChoice

from tests._snapshots import assert_snapshot
from tests.parity.conftest import ProviderUnderTest, capture_chat_body


def _dummy_tool() -> Tool:
    return Tool(
        name="get_weather",
        description="Get weather for a city",
        input_schema={
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"],
        },
    )


@pytest.mark.parametrize(
    "choice_name,choice",
    [
        ("auto", ToolChoice.auto()),
        ("required", ToolChoice.required()),
        ("none", ToolChoice.none()),
        ("tool_named", ToolChoice.tool("get_weather")),
    ],
    ids=["auto", "required", "none", "tool_named"],
)
@respx.mock
@pytest.mark.asyncio
async def test_tool_choice_matrix(
    provider_under_test: ProviderUnderTest,
    choice_name: str,
    choice: ToolChoice,
):
    req = ChatRequest(
        messages=[Message.user("what is the weather?")],
        tools=[_dummy_tool()],
        tool_choice=choice,
    )
    body = await capture_chat_body(provider_under_test, req)
    assert_snapshot(
        f"tool_choice_{choice_name}_{provider_under_test.name}", body
    )
```

- [ ] **Step 2: Run to generate snapshots**

Run: `cd sdks/python && UPDATE_SNAPSHOTS=1 uv run pytest tests/parity/test_tool_choice_parity.py -v`
Expected: PASS — 16 tests, 16 JSON files written.

- [ ] **Step 3: Visually review key snapshots for correctness**

```bash
grep -H "\"type\"" sdks/python/tests/snapshots/tool_choice_required_*.json
```

Expected output:
- `tool_choice_required_anthropic.json` contains `"type": "any"` (Anthropic's name for required)
- `tool_choice_required_openai.json` contains `"tool_choice": "required"` or `{"type": "required"}` (OpenAI native)
- `tool_choice_required_gemini.json` contains `"mode": "ANY"` (Gemini uppercase)
- `tool_choice_required_minimax.json` per MiniMax's convention

For `none` variant, verify `tools` field is **absent** in every snapshot (all providers should drop tools when choice is none).

If any mapping is wrong, fix the provider — don't edit the snapshot.

- [ ] **Step 4: Run without UPDATE_SNAPSHOTS**

Run: `cd sdks/python && uv run pytest tests/parity/test_tool_choice_parity.py -v`
Expected: PASS — deterministic.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/tests/parity/test_tool_choice_parity.py sdks/python/tests/snapshots/
git commit -m "test(python): add tool_choice parity matrix (4 variants × 4 providers)"
```

---

## Task 5: OpenAI vision — serialization implementation

**Files:**
- Modify: `sdks/python/motosan_ai/providers/openai.py` (`_serialize_messages`)
- Create: `sdks/python/tests/test_openai_vision.py`

OpenAI provider declares `capabilities = with_image()` but has no `content_blocks` handling — vision requests currently emit only the plain `content` string, silently losing images. Wire format per Rust: `{"type": "image_url", "image_url": {"url": "data:MIME;base64,DATA"}}` for base64, `{"url": URL}` for URL source.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_openai_vision.py`:

```python
import json

import httpx
import pytest
import respx

from motosan_ai.providers.openai import OpenAIProvider
from motosan_ai.types import (
    ChatRequest,
    ImageBlock,
    ImageSourceBase64,
    ImageSourceUrl,
    Message,
    TextBlock,
)


@pytest.fixture
def provider():
    return OpenAIProvider("test-key", base_url="https://mock.openai.com")


def _ok() -> httpx.Response:
    return httpx.Response(
        200,
        json={
            "id": "chatcmpl-1",
            "object": "chat.completion",
            "model": "gpt-4o",
            "choices": [
                {
                    "message": {"role": "assistant", "content": "ok"},
                    "finish_reason": "stop",
                    "index": 0,
                }
            ],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
        },
    )


@respx.mock
@pytest.mark.asyncio
async def test_user_with_image_base64_becomes_image_url_data_uri(provider):
    route = respx.post("https://mock.openai.com/v1/chat/completions").mock(
        return_value=_ok()
    )
    req = ChatRequest(
        messages=[Message.user_with_image("look", "JVBER", "image/png")]
    )
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    user_msg = body["messages"][-1]
    assert user_msg["role"] == "user"
    assert user_msg["content"] == [
        {"type": "text", "text": "look"},
        {
            "type": "image_url",
            "image_url": {"url": "data:image/png;base64,JVBER"},
        },
    ]


@respx.mock
@pytest.mark.asyncio
async def test_image_url_becomes_image_url_with_raw_url(provider):
    route = respx.post("https://mock.openai.com/v1/chat/completions").mock(
        return_value=_ok()
    )
    msg = Message.user_with_blocks(
        [
            TextBlock(text="see"),
            ImageBlock(source=ImageSourceUrl(url="https://x.com/i.png")),
        ]
    )
    await provider.chat(ChatRequest(messages=[msg]))

    body = json.loads(route.calls[0].request.content)
    user_msg = body["messages"][-1]
    assert user_msg["content"] == [
        {"type": "text", "text": "see"},
        {"type": "image_url", "image_url": {"url": "https://x.com/i.png"}},
    ]


@respx.mock
@pytest.mark.asyncio
async def test_plain_text_user_unchanged(provider):
    """Regression: no content_blocks means content stays as plain string."""
    route = respx.post("https://mock.openai.com/v1/chat/completions").mock(
        return_value=_ok()
    )
    await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    body = json.loads(route.calls[0].request.content)
    assert body["messages"][-1]["content"] == "hi"
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd sdks/python && uv run pytest tests/test_openai_vision.py -v`
Expected: FAIL — first two tests; third passes. Images dropped by current `_serialize_messages`.

- [ ] **Step 3: Extend OpenAI `_serialize_messages`**

Open `sdks/python/motosan_ai/providers/openai.py`. Find the `_serialize_messages` method (around line 40-90). In the user-message branch, replace the plain-content serialization with block-aware serialization:

```python
if message.role == Role.user:
    if message.content_blocks:
        blocks: list[dict[str, Any]] = []
        for block in message.content_blocks:
            from motosan_ai.types import (
                ImageBlock,
                ImageSourceBase64,
                ImageSourceUrl,
                TextBlock,
            )
            if isinstance(block, TextBlock):
                blocks.append({"type": "text", "text": block.text})
            elif isinstance(block, ImageBlock):
                src = block.source
                if isinstance(src, ImageSourceBase64):
                    url = f"data:{src.media_type};base64,{src.data}"
                    blocks.append(
                        {"type": "image_url", "image_url": {"url": url}}
                    )
                elif isinstance(src, ImageSourceUrl):
                    blocks.append(
                        {"type": "image_url", "image_url": {"url": src.url}}
                    )
            # DocumentBlock rejected by validate_request() before reaching here
        outgoing.append({"role": "user", "content": blocks})
    else:
        outgoing.append({"role": "user", "content": message.content})
    continue
```

Move the `from motosan_ai.types import ...` to the top of the file with the other imports.

- [ ] **Step 4: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_openai_vision.py tests/test_openai.py -v`
Expected: PASS — 3 vision tests + all existing OpenAI tests.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/openai.py sdks/python/tests/test_openai_vision.py
git commit -m "feat(python,openai): serialize image content blocks as image_url parts"
```

---

## Task 6: Parity test — vision matrix

**Files:**
- Create: `sdks/python/tests/parity/test_vision_parity.py`

Covers Anthropic (`full()`), OpenAI (`with_image()`), Gemini (`with_image()`). Excludes MiniMax (uses OpenAI-compat; if Python MiniMax inherits the same `content_blocks` path, it should work — but that's a separate concern, not a parity gap).

- [ ] **Step 1: Write failing test**

Create `sdks/python/tests/parity/test_vision_parity.py`:

```python
from __future__ import annotations

import pytest
import respx

from motosan_ai.types import ChatRequest, Message

from tests._snapshots import assert_snapshot
from tests.parity.conftest import ProviderUnderTest, capture_chat_body


@respx.mock
@pytest.mark.asyncio
async def test_vision_base64_image(provider_under_test: ProviderUnderTest):
    if provider_under_test.name == "minimax":
        pytest.skip("MiniMax vision coverage is a separate concern")

    req = ChatRequest(
        messages=[Message.user_with_image("describe", "JVBER", "image/png")]
    )
    body = await capture_chat_body(provider_under_test, req)
    assert_snapshot(f"vision_base64_{provider_under_test.name}", body)
```

- [ ] **Step 2: Run to generate snapshots**

Run: `cd sdks/python && UPDATE_SNAPSHOTS=1 uv run pytest tests/parity/test_vision_parity.py -v`
Expected: PASS — 4 tests (1 skip for MiniMax, 3 snapshots written).

- [ ] **Step 3: Verify each provider's shape matches spec**

```bash
cat sdks/python/tests/snapshots/vision_base64_anthropic.json
cat sdks/python/tests/snapshots/vision_base64_openai.json
cat sdks/python/tests/snapshots/vision_base64_gemini.json
```

Expected:
- Anthropic: `content` is array with `{"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "JVBER"}}`
- OpenAI: `content` is array with `{"type": "image_url", "image_url": {"url": "data:image/png;base64,JVBER"}}`
- Gemini: `parts` contains `{"inlineData": {"mimeType": "image/png", "data": "JVBER"}}`

If any shape is wrong, fix the provider — don't edit the snapshot.

- [ ] **Step 4: Run without UPDATE_SNAPSHOTS**

Run: `cd sdks/python && uv run pytest tests/parity/test_vision_parity.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/tests/parity/test_vision_parity.py sdks/python/tests/snapshots/
git commit -m "test(python): add vision parity matrix (Anthropic/OpenAI/Gemini)"
```

---

## Task 7: Stream contract parity

**Files:**
- Create: `sdks/python/tests/parity/test_stream_contract.py`

Streams don't produce a single body to snapshot — instead, verify the **event contract**: a text-only stream must emit one or more `text` events then exactly one terminal `done` event; a tool-use stream must emit `tool_call_start` → `tool_call_args` → `tool_call_end` in order, wrapped by `text` events on either side if the model interleaves.

This is a behavior test, not a wire-format snapshot. Uses provider-specific mock SSE strings but asserts the **output event shape**, which should be invariant.

- [ ] **Step 1: Write failing test**

Create `sdks/python/tests/parity/test_stream_contract.py`:

```python
"""Stream event contract — provider-agnostic invariants.

All providers must:
- Emit at least one `text` event when the model responds with text
- Emit exactly one terminal `done` event
- Never emit events after `done`

Tool-use streams must emit `tool_call_start` → `tool_call_args` → `tool_call_end`
before the terminal `done`.
"""

from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai.types import ChatRequest, Message


def _anthropic_sse() -> str:
    events = [
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "hello"},
        },
        {"type": "message_stop"},
    ]
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


def _openai_sse() -> str:
    events = [
        {
            "id": "chatcmpl-1",
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": {"content": "hello"}}],
        },
        {
            "id": "chatcmpl-1",
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
        },
    ]
    return (
        "\n".join(f"data: {json.dumps(e)}" for e in events)
        + "\ndata: [DONE]\n"
    )


def _gemini_sse() -> str:
    events = [
        {
            "candidates": [
                {
                    "content": {"parts": [{"text": "hello"}]},
                    "finishReason": "STOP",
                }
            ]
        }
    ]
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


def _minimax_sse() -> str:
    events = [
        {
            "id": "1",
            "choices": [{"index": 0, "delta": {"content": "hello"}}],
        },
        {
            "id": "1",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
        },
    ]
    return (
        "\n".join(f"data: {json.dumps(e)}" for e in events)
        + "\ndata: [DONE]\n"
    )


_SSE_BY_PROVIDER = {
    "anthropic": _anthropic_sse,
    "openai": _openai_sse,
    "gemini": _gemini_sse,
    "minimax": _minimax_sse,
}


@respx.mock
@pytest.mark.asyncio
async def test_text_stream_contract(provider_under_test):
    respx.post(provider_under_test.stream_endpoint).mock(
        return_value=httpx.Response(
            200,
            text=_SSE_BY_PROVIDER[provider_under_test.name](),
            headers={"content-type": "text/event-stream"},
        )
    )

    events = [
        e
        async for e in provider_under_test.provider.stream(
            ChatRequest(messages=[Message.user("hi")])
        )
    ]

    # Contract checks
    text_events = [e for e in events if e.event_type == "text" and not e.done]
    done_events = [e for e in events if e.done]

    assert len(text_events) >= 1, (
        f"{provider_under_test.name}: expected >=1 text event, got 0"
    )
    assert "".join(e.content for e in text_events) == "hello"
    assert len(done_events) == 1, (
        f"{provider_under_test.name}: expected exactly 1 done event, got "
        f"{len(done_events)}"
    )
    assert events[-1].done, (
        f"{provider_under_test.name}: done event must be last"
    )
```

- [ ] **Step 2: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/parity/test_stream_contract.py -v`
Expected: PASS — 4 tests, one per provider. If any fails, the provider's stream adapter violates the contract.

- [ ] **Step 3: Commit**

```bash
git add sdks/python/tests/parity/test_stream_contract.py
git commit -m "test(python): add stream event contract parity across providers"
```

---

## Task 8: Client integration — provider dispatch matrix

**Files:**
- Create: `sdks/python/tests/test_client_integration.py`

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_client_integration.py`:

```python
"""Client-level integration tests — wiring, dispatch, retry, env-var fallback.

These tests verify behavior that emerges from Client composing providers,
not individual provider logic (covered elsewhere).
"""

from __future__ import annotations

import httpx
import pytest
import respx

from motosan_ai import Client, Provider
from motosan_ai.error import ConfigError, RateLimitError
from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.providers.minimax import MinimaxProvider
from motosan_ai.providers.openai import OpenAIProvider


@pytest.mark.parametrize(
    "provider_enum,env_var,expected_class",
    [
        (Provider.anthropic, "ANTHROPIC_API_KEY", AnthropicProvider),
        (Provider.openai, "OPENAI_API_KEY", OpenAIProvider),
        (Provider.gemini, "GEMINI_API_KEY", GeminiProvider),
        (Provider.minimax, "MINIMAX_API_KEY", MinimaxProvider),
    ],
    ids=["anthropic", "openai", "gemini", "minimax"],
)
def test_client_dispatch_resolves_correct_provider_class(
    provider_enum, env_var, expected_class, monkeypatch
):
    monkeypatch.setenv(env_var, "test-key-from-env")
    client = Client(provider=provider_enum)
    assert isinstance(client._provider, expected_class)
    assert client.api_key == "test-key-from-env"


@pytest.mark.parametrize(
    "provider_enum,env_var",
    [
        (Provider.anthropic, "ANTHROPIC_API_KEY"),
        (Provider.openai, "OPENAI_API_KEY"),
        (Provider.gemini, "GEMINI_API_KEY"),
        (Provider.minimax, "MINIMAX_API_KEY"),
    ],
)
def test_missing_env_var_raises_config_error(
    provider_enum, env_var, monkeypatch
):
    monkeypatch.delenv(env_var, raising=False)
    with pytest.raises(ConfigError):
        Client(provider=provider_enum)


def test_explicit_api_key_overrides_env(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "env-key")
    client = Client(provider=Provider.anthropic, api_key="explicit-key")
    assert client.api_key == "explicit-key"
```

- [ ] **Step 2: Run tests to verify pass**

Run: `cd sdks/python && uv run pytest tests/test_client_integration.py -v`
Expected: PASS — 9 tests.

- [ ] **Step 3: Commit**

```bash
git add sdks/python/tests/test_client_integration.py
git commit -m "test(python): add Client provider-dispatch matrix"
```

---

## Task 9: Client integration — retry end-to-end

**Files:**
- Modify: `sdks/python/tests/test_client_integration.py`

Verifies the full chain: `Client.chat()` → provider → 429 → retry.py backoff → 200.

- [ ] **Step 1: Append failing tests**

Append to `sdks/python/tests/test_client_integration.py`:

```python
@respx.mock
@pytest.mark.asyncio
async def test_client_retries_on_429_then_succeeds(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        side_effect=[
            httpx.Response(429, json={"error": {"message": "slow down"}}),
            httpx.Response(
                200,
                json={
                    "model": "claude-sonnet-4-6",
                    "stop_reason": "end_turn",
                    "usage": {"input_tokens": 1, "output_tokens": 1},
                    "content": [{"type": "text", "text": "ok"}],
                },
            ),
        ]
    )
    client = Client(provider=Provider.anthropic, max_retries=2)
    resp = await client.chat([{"role": "user", "content": "hi"}])
    assert resp.content == "ok"
    assert route.call_count == 2


@respx.mock
@pytest.mark.asyncio
async def test_client_gives_up_after_max_retries(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(429, json={"error": {"message": "slow"}})
    )
    client = Client(provider=Provider.anthropic, max_retries=1)
    with pytest.raises(RateLimitError):
        await client.chat([{"role": "user", "content": "hi"}])
    # One original + one retry = 2 calls
    assert route.call_count == 2


@respx.mock
@pytest.mark.asyncio
async def test_client_does_not_retry_on_4xx_non_429(monkeypatch):
    from motosan_ai.error import AuthError

    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(401, json={"error": {"message": "bad"}})
    )
    client = Client(provider=Provider.anthropic, max_retries=3)
    with pytest.raises(AuthError):
        await client.chat([{"role": "user", "content": "hi"}])
    assert route.call_count == 1  # no retries for auth errors
```

- [ ] **Step 2: Run tests to verify pass (or fail if retry logic is wrong)**

Run: `cd sdks/python && uv run pytest tests/test_client_integration.py -v`
Expected: PASS — 3 new tests. If any fail, there's a real retry bug (e.g. retrying on 401, or not counting attempts correctly).

- [ ] **Step 3: Commit**

```bash
git add sdks/python/tests/test_client_integration.py
git commit -m "test(python): add Client retry end-to-end integration tests"
```

---

## Task 10: Nightly live-test CI workflow

**Files:**
- Create: `.github/workflows/ci-python-nightly.yml`

Runs `tests/integration/` on schedule (nightly UTC 08:00) with API keys in GitHub secrets. Failing live tests email the repo admins via standard GitHub notifications. Intentionally separate from the PR gate — live tests flake from upstream incidents and shouldn't block merges.

- [ ] **Step 1: Create workflow**

Create `.github/workflows/ci-python-nightly.yml`:

```yaml
name: ci-python-nightly

on:
  schedule:
    # Daily at 08:00 UTC (16:00 Asia/Taipei)
    - cron: "0 8 * * *"
  workflow_dispatch: {}  # allow manual trigger from Actions tab

jobs:
  live:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: sdks/python
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: "3.11"
      - name: Install uv
        uses: astral-sh/setup-uv@v3
      - name: Sync deps
        run: uv sync --extra full --extra dev
      - name: Run live integration tests
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
          GEMINI_API_KEY: ${{ secrets.GEMINI_API_KEY }}
          MINIMAX_API_KEY: ${{ secrets.MINIMAX_API_KEY }}
        run: uv run pytest tests/integration/ -v
```

- [ ] **Step 2: Locally validate YAML syntax**

Run: `python -c "import yaml; yaml.safe_load(open('.github/workflows/ci-python-nightly.yml'))"`
Expected: no output (valid YAML).

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci-python-nightly.yml
git commit -m "ci(python): add nightly live-test workflow"
```

- [ ] **Step 4: Note for the user**

After the commit lands on `main`, the repo admin must add these secrets in GitHub Settings → Secrets and variables → Actions:
- `ANTHROPIC_API_KEY`
- `OPENAI_API_KEY`
- `GEMINI_API_KEY`
- `MINIMAX_API_KEY`

Without secrets, the nightly job fails on the live-test step — which is fine; it'll be visible and fixable.

---

## Task 11: Update `check-python` gate to include parity phase

**Files:**
- Modify: `devshell/scripts.nix` (the `check-python` block around line 54-67)

- [ ] **Step 1: Locate the script**

Run: `grep -n "check-python" devshell/scripts.nix`

- [ ] **Step 2: Update the echo label**

The existing pytest invocation already runs the whole `tests/` dir minus `tests/integration/`, so `tests/parity/` is automatically picked up. No change to the command itself — just update the label so operators know parity + snapshot tests are running:

Edit `devshell/scripts.nix`, find:

```nix
    echo "[3/3] pytest (unit)"
```

Replace with:

```nix
    echo "[3/3] pytest (unit + parity + snapshots)"
```

- [ ] **Step 3: Re-enter devshell and verify**

Run: `nix develop -c check-python 2>&1 | tail -10`
Expected: all 3 phases pass, label shows "unit + parity + snapshots".

(If `nix develop` isn't available locally, run the equivalent: `cd sdks/python && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration/`.)

- [ ] **Step 4: Commit**

```bash
git add devshell/scripts.nix
git commit -m "chore(devshell): clarify check-python covers parity + snapshots"
```

---

## Task 12: Release — CHANGELOG + version bump to 0.8.1

**Files:**
- Modify: `sdks/python/CHANGELOG.md`
- Modify: `sdks/python/pyproject.toml`

- [ ] **Step 1: Bump version**

Edit `sdks/python/pyproject.toml`:

```toml
version = "0.8.1"
```

- [ ] **Step 2: Prepend CHANGELOG entry**

Replace the date with the actual release day (YYYY-MM-DD) when cutting the release.

```markdown
## [0.8.1] - YYYY-MM-DD

### Added
- **Test infrastructure — drift detection**
  - `tests/_snapshots.py` helper: JSON-file snapshots with `UPDATE_SNAPSHOTS=1` regenerate mode.
  - `tests/parity/` — cross-provider matrix tests:
    - `test_simple_chat_parity.py` — plain text + multi-turn bodies.
    - `test_tool_choice_parity.py` — 4 `ToolChoice` variants × 4 providers.
    - `test_vision_parity.py` — image base64 serialization across Anthropic, OpenAI, Gemini.
    - `test_stream_contract.py` — provider-agnostic stream event invariants.
  - `tests/test_client_integration.py` — provider dispatch matrix, env-var fallback, retry end-to-end.
  - Nightly CI workflow (`.github/workflows/ci-python-nightly.yml`) runs live integration tests against real provider APIs.

### Fixed
- **OpenAI vision serialization** — `OpenAIProvider._serialize_messages` now emits `content_blocks` as `image_url` parts (base64 → data URI, URL → raw URL). Previously, calling `chat()` with `Message.user_with_image(...)` silently dropped the image and sent only the plain text content.

### Notes
- Snapshots are code-review artifacts. Any diff in `tests/snapshots/*.json` in a PR is a deliberate wire-format change; review as carefully as schema migrations.
- Nightly live-test secrets must be configured in GitHub repo settings (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`, `MINIMAX_API_KEY`).
```

- [ ] **Step 3: Run the full gate**

Run: `cd /Users/daiwanwei/Projects/wade/motosan-ai && check-python`
Expected: ruff + format + pytest (unit + parity + snapshots) all pass.

- [ ] **Step 4: Commit**

```bash
git add sdks/python/pyproject.toml sdks/python/CHANGELOG.md
git commit -m "chore(python): release v0.8.1 — test infra + OpenAI vision fix"
```

---

## Final Self-Review Checklist

Before declaring this work done, verify:

- [ ] `cd sdks/python && uv run pytest tests/ -v` — all tests pass (target: ~300 passing).
- [ ] `cd /Users/daiwanwei/Projects/wade/motosan-ai && check-python` — full gate passes.
- [ ] `UPDATE_SNAPSHOTS=1 uv run pytest tests/parity/` regenerates snapshots cleanly; re-running without the env var still passes.
- [ ] `ls tests/snapshots/` shows ~23 JSON files (2 simple_chat × 4 + 4 tool_choice × 4 + 3 vision).
- [ ] Every snapshot diff in the PR was visually reviewed — none introduced by accident.
- [ ] OpenAI vision test passes; manual eyeball of `test_openai_vision.py::test_user_with_image_base64_becomes_image_url_data_uri` snapshot confirms `data:image/png;base64,JVBER` format.
- [ ] `ci-python-nightly.yml` is syntactically valid YAML; cron fires at 08:00 UTC.
- [ ] Version `0.8.1` in `pyproject.toml`; CHANGELOG entry present.
- [ ] No `TODO` / `FIXME` / placeholder strings introduced.

If any box is unchecked, fix before tagging/publishing.

---

## What this plan does NOT do

- ❌ Property-based testing (Hypothesis) — overkill for wire-format validation; snapshot files already give exhaustive structural coverage for the canonical shapes.
- ❌ Contract tests against the real Anthropic/OpenAI/Gemini official SDKs — too slow, too flaky, and would couple our test suite to their release cadence.
- ❌ Python version matrix in CI — SDK targets 3.11+; add when a caller reports 3.12/3.13 incompatibility.
- ❌ MiniMax vision serialization coverage — MiniMax uses OpenAI-compat shape, so the fix from Task 5 likely propagates if the Python MiniMax provider shares `_serialize_messages` logic. Verify and add a dedicated test if it diverges — but that's a follow-up, not in scope here.
- ❌ Anthropic OAuth live test in nightly — requires a renewable OAuth token, not a static API key. Track separately.
- ❌ Golden-file coverage for every existing unit test — new snapshots are opt-in; existing `respx` dict-fragment assertions stay as they are. Only the parity matrix uses snapshots.
