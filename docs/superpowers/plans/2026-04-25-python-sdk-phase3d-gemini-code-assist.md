# Python SDK Phase 3d — Gemini Code Assist OAuth + HTTP Provider Implementation Plan

> **Status:** ✅ **COMPLETE (2026-04-25)** — shipped as `motosan-ai` v0.9.3.
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `GeminiCodeAssistProvider` (HTTP) + `motosan_ai/oauth/google.py` (PKCE flow) so Python callers can authenticate via Google OAuth and talk to `cloudcode-pa.googleapis.com` end-to-end. Mirrors Rust's `GeminiCodeAssistProvider` + `motosan-ai-oauth` crate.

**Architecture:** **Two independent subsystems** that compose at the `Client` layer:
- **HTTP provider** (`motosan_ai/providers/gemini_code_assist.py`) — takes a pre-acquired access_token + project_id. Wraps the existing `GeminiProvider._build_body` output in an envelope (`{project, model, request, userAgent, requestId}`), POSTs to `cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse`, parses the SSE stream (each line carries `{response: {candidates, usageMetadata}}`).
- **OAuth flow** (`motosan_ai/oauth/google.py`) — PKCE-based loopback OAuth: generates challenge/verifier, opens browser to Google auth URL, runs a localhost HTTP server to catch the callback, exchanges code for token, persists token cache to `~/.config/motosan-ai/google-tokens.json`. Reusable for any Google OAuth use case (not just Code Assist).

**Tech Stack:** Python 3.11+, `httpx`, `respx` for mocks, `pytest-asyncio`. **No new prod deps** — stdlib `secrets` + `hashlib` for PKCE, stdlib `http.server` (`socketserver`) for the loopback callback. No `cryptography`, no `authlib`, no `google-auth`.

**Ships as:** `motosan-ai` v0.9.3 (final Phase 3 sub-release; Phase 4 is API-surface polish).

---

## Reference material

- **Rust canon (verified before writing):**
  - [sdks/rust/src/providers/gemini_code_assist.rs](sdks/rust/src/providers/gemini_code_assist.rs) — 473 lines. `GeminiCodeAssistProvider` at line 44; `build_envelope` at line 80; `apply_auth` at line 93; `CodeAssistStreamAdapter` at line 173.
  - [sdks/rust/src/models.rs:32-33](sdks/rust/src/models.rs#L32) — `DEFAULT_GEMINI_CODE_ASSIST_MODEL = "gemini-2.5-flash"`, `GEMINI_CODE_ASSIST_BASE_URL = "https://cloudcode-pa.googleapis.com"`.
  - [sdks/rust/crates/motosan-ai-oauth/src/lib.rs](sdks/rust/crates/motosan-ai-oauth/src/lib.rs) — `OAuthConfig`, `Token`, `login`, `refresh`, `build_auth_url`. PKCE flow with state validation + 120s timeout.
  - [sdks/rust/crates/motosan-ai-oauth/src/pkce.rs](sdks/rust/crates/motosan-ai-oauth/src/pkce.rs) — 64 random bytes → URL-safe base64 (no pad) → SHA256 → URL-safe base64 (no pad).
  - [sdks/rust/crates/motosan-ai-oauth/src/providers/gemini.rs](sdks/rust/crates/motosan-ai-oauth/src/providers/gemini.rs) — Google Gemini-CLI public credentials (intentionally distributed per Google's installed-app docs).

- **Verified wire facts:**
  - URL: `POST {base}/v1internal:streamGenerateContent?alt=sse` (no model in path — model goes in body)
  - Headers: `authorization: Bearer <token>`, `user-agent: google-cloud-sdk vscode_cloudshelleditor/0.1`, `x-goog-api-client: gl-node/22.17.0`, `client-metadata: {"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}`, `content-type: application/json`
  - Body envelope: `{"project": "<project_id>", "model": "<model>", "request": <inner gemini body>, "userAgent": "motosan-ai", "requestId": "motosan-{ts_ms}-{counter:09d}"}`
  - Inner body = same as `GeminiProvider._build_body(request)` (existing Phase 2b serializer)
  - SSE event payload: `{"response": {"candidates": [...], "usageMetadata": {...}}}` — note outer `response` wrapper, distinct from vanilla Gemini API
  - Usage: `usageMetadata.promptTokenCount - cachedContentTokenCount` → `Usage.input_tokens`; `cachedContentTokenCount` → `Usage.cache_read_input_tokens` (None if 0)
  - Tool-call IDs: prefer `functionCall.id` from API; fall back to `f"{name}_{ts_ms}_{counter}"` if missing/empty/duplicate
  - Capabilities: `with_image()` (same as vanilla Gemini)

- **OAuth wire facts (from Google + Gemini CLI source):**
  - Auth URL: `https://accounts.google.com/o/oauth2/auth`
  - Token URL: `https://oauth2.googleapis.com/token`
  - Scopes: `cloud-platform`, `userinfo.email`, `userinfo.profile`
  - Public client_id / client_secret from Gemini CLI (intentionally distributed)
  - Auth URL params: `client_id`, `response_type=code`, `redirect_uri`, `scope` (space-joined), `state`, `code_challenge`, `code_challenge_method=S256`, `access_type=offline`
  - Loopback redirect: `http://127.0.0.1:<port>/auth/callback`
  - Callback server returns `(code, state)` — caller validates state matches what was sent
  - Token response: `access_token`, `refresh_token`, `id_token` (optional), `expires_in` — store + augment with `issued_at` for expiry math
  - Refresh: POST `token_url` with `grant_type=refresh_token`, `refresh_token`, `client_id`, `client_secret`

- **Phase 2b reference:** [sdks/python/motosan_ai/providers/gemini.py](sdks/python/motosan_ai/providers/gemini.py) — `GeminiProvider._build_body` is reused unchanged for the inner request envelope.

---

## File Structure

| Path | Responsibility | Status |
|------|----------------|--------|
| `sdks/python/motosan_ai/providers/gemini_code_assist.py` | `GeminiCodeAssistProvider` HTTP provider — envelope, headers, SSE adapter | **Create** |
| `sdks/python/motosan_ai/oauth/__init__.py` | Package marker; export `Token`, `login_google`, `refresh_google`, `load_cached_token`, `save_token` | **Create** |
| `sdks/python/motosan_ai/oauth/google.py` | Google PKCE OAuth flow + token exchange/refresh + cache | **Create** |
| `sdks/python/motosan_ai/oauth/_pkce.py` | PKCE challenge/verifier generation | **Create** |
| `sdks/python/motosan_ai/oauth/_callback_server.py` | Localhost loopback `http.server` for OAuth callback capture | **Create** |
| `sdks/python/motosan_ai/providers/__init__.py` | Export `GeminiCodeAssistProvider` | **Modify** |
| `sdks/python/motosan_ai/__init__.py` | Top-level export | **Modify** |
| `sdks/python/motosan_ai/client.py` | Register `Provider.gemini_code_assist`, add `Client.gemini_code_assist()` classmethod | **Modify** |
| `sdks/python/tests/test_code_assist_request.py` | Envelope + auth header assertions | **Create** |
| `sdks/python/tests/test_code_assist_stream.py` | SSE adapter — text, tool calls, usage, stop_reason | **Create** |
| `sdks/python/tests/test_code_assist_dispatch.py` | `Provider.gemini_code_assist` dispatch | **Create** |
| `sdks/python/tests/test_oauth_pkce.py` | PKCE generator unit tests | **Create** |
| `sdks/python/tests/test_oauth_token.py` | Token expiry math + cache file persistence | **Create** |
| `sdks/python/tests/test_oauth_google.py` | `login_google` + `refresh_google` with mocked HTTP + callback server | **Create** |
| `sdks/python/tests/integration/test_code_assist_live.py` | Live test (requires real OAuth token) | **Create** |
| `sdks/python/CHANGELOG.md` | v0.9.3 entry | **Modify** |
| `sdks/python/pyproject.toml` | Version bump 0.9.2 → 0.9.3 | **Modify** |

Design principles:
- **Two subsystems composed at Client layer.** `GeminiCodeAssistProvider(access_token, project_id, ...)` doesn't know about OAuth; `oauth/google.py` doesn't know about Code Assist. `Client.gemini_code_assist()` orchestrates: load cached token → refresh-if-expired → construct provider.
- **No new prod deps.** stdlib only for PKCE + callback server. `httpx` already present for token exchange.
- **OAuth tested in isolation.** PKCE / token / callback-server each have unit tests with no live network. The end-to-end `login_google` test uses a mocked browser-open + a local HTTP test client that posts to the callback URL.
- **Reuse `GeminiProvider._build_body`.** No new wire-format code for the inner request — Phase 2b's serializer composes vision / tools / system / generationConfig identically.
- **Keep inner code path tested.** SSE adapter test cases cover the same shapes as `vanilla` Gemini plus the outer `response` wrapper Code Assist adds.

---

## Part A: HTTP provider (Tasks 1-7)

## Task 1: Module skeleton + URL helpers + auth headers

**Files:**
- Create: `sdks/python/motosan_ai/providers/gemini_code_assist.py`
- Create: `sdks/python/tests/test_code_assist_request.py`

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_code_assist_request.py`:

```python
from __future__ import annotations

from motosan_ai.providers.gemini_code_assist import GeminiCodeAssistProvider


def test_default_model_and_base_url():
    p = GeminiCodeAssistProvider("ya29.fake", "test-project")
    assert p.model == "gemini-2.5-flash"
    assert p.base_url == "https://cloudcode-pa.googleapis.com"


def test_explicit_model_and_base_url():
    p = GeminiCodeAssistProvider(
        "ya29.fake", "test-project", model="gemini-2.5-pro", base_url="https://mock.test"
    )
    assert p.model == "gemini-2.5-pro"
    assert p.base_url == "https://mock.test"


def test_stream_url_includes_v1internal_and_alt_sse():
    p = GeminiCodeAssistProvider("ya29.fake", "test-project")
    url = p._stream_url()
    assert url.endswith("/v1internal:streamGenerateContent?alt=sse")
    assert "cloudcode-pa.googleapis.com" in url


def test_auth_headers_present():
    p = GeminiCodeAssistProvider("ya29.fake", "test-project")
    h = p._headers()
    assert h["authorization"] == "Bearer ya29.fake"
    assert h["user-agent"] == "google-cloud-sdk vscode_cloudshelleditor/0.1"
    assert h["x-goog-api-client"] == "gl-node/22.17.0"
    assert "ideType" in h["client-metadata"]
    assert h["content-type"] == "application/json"
```

- [ ] **Step 2: Run — should FAIL (module not found)**

Run: `cd sdks/python && uv run pytest tests/test_code_assist_request.py -v`
Expected: ImportError.

- [ ] **Step 3: Create skeleton**

Create `sdks/python/motosan_ai/providers/gemini_code_assist.py`:

```python
from __future__ import annotations

import httpx

from motosan_ai.provider_base import ProviderCapabilities

_DEFAULT_BASE_URL = "https://cloudcode-pa.googleapis.com"
_DEFAULT_MODEL = "gemini-2.5-flash"
_USER_AGENT = "google-cloud-sdk vscode_cloudshelleditor/0.1"
_X_GOOG_API_CLIENT = "gl-node/22.17.0"
_CLIENT_METADATA = (
    '{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}'
)


class GeminiCodeAssistProvider:
    capabilities: ProviderCapabilities = ProviderCapabilities.with_image()

    def __init__(
        self,
        access_token: str,
        project_id: str,
        model: str | None = None,
        base_url: str | None = None,
    ) -> None:
        self.access_token = access_token
        self.project_id = project_id
        self.model = model or _DEFAULT_MODEL
        self.base_url = (base_url or _DEFAULT_BASE_URL).rstrip("/")
        self._http = httpx.AsyncClient(timeout=120.0)

    def _stream_url(self) -> str:
        return f"{self.base_url}/v1internal:streamGenerateContent?alt=sse"

    def _headers(self) -> dict[str, str]:
        return {
            "authorization": f"Bearer {self.access_token}",
            "user-agent": _USER_AGENT,
            "x-goog-api-client": _X_GOOG_API_CLIENT,
            "client-metadata": _CLIENT_METADATA,
            "content-type": "application/json",
        }
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_code_assist_request.py -v`
Expected: 4 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini_code_assist.py sdks/python/tests/test_code_assist_request.py
git commit -m "feat(python,code-assist): module skeleton + auth headers + stream URL"
```

---

## Task 2: Request envelope + ID generators

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini_code_assist.py`
- Modify: `sdks/python/tests/test_code_assist_request.py`

Wraps the inner request (from `GeminiProvider._build_body`) in `{project, model, request, userAgent, requestId}`. ID generator format per Rust: `motosan-{ts_ms}-{counter:09d}`.

- [ ] **Step 1: Append failing tests**

```python
import re
from motosan_ai.providers.gemini_code_assist import (
    _gen_request_id,
    _gen_tool_call_id,
)
from motosan_ai.types import ChatRequest, Message


def test_request_id_format():
    rid = _gen_request_id()
    assert re.fullmatch(r"motosan-\d+-\d{9}", rid), rid


def test_request_id_is_unique():
    a = _gen_request_id()
    b = _gen_request_id()
    assert a != b


def test_tool_call_id_format():
    tcid = _gen_tool_call_id("get_weather")
    assert re.fullmatch(r"get_weather_\d+_\d+", tcid), tcid


def test_envelope_wraps_inner_body():
    p = GeminiCodeAssistProvider("ya29.fake", "myproj")
    req = ChatRequest(messages=[Message.user("hi")])
    env = p._build_envelope(req)
    assert env["project"] == "myproj"
    assert env["model"] == "gemini-2.5-flash"
    assert env["userAgent"] == "motosan-ai"
    assert re.fullmatch(r"motosan-\d+-\d{9}", env["requestId"])
    assert "request" in env
    # Inner request from GeminiProvider is `{contents, generationConfig, ...}`
    assert "contents" in env["request"]
    assert "generationConfig" in env["request"]


def test_envelope_per_request_model_overrides_default():
    p = GeminiCodeAssistProvider("ya29.fake", "myproj")
    req = ChatRequest(messages=[Message.user("hi")], model="gemini-2.5-pro")
    env = p._build_envelope(req)
    assert env["model"] == "gemini-2.5-pro"
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_code_assist_request.py -v -k "request_id or tool_call_id or envelope"`
Expected: ImportError on `_gen_request_id`.

- [ ] **Step 3: Implement helpers + envelope**

Add to `gemini_code_assist.py` (top-level):

```python
import time
from itertools import count
from typing import Any

from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.types import ChatRequest

_REQUEST_COUNTER = count()
_TOOL_CALL_COUNTER = count()


def _gen_request_id() -> str:
    ts_ms = int(time.time() * 1000)
    n = next(_REQUEST_COUNTER)
    return f"motosan-{ts_ms}-{n:09d}"


def _gen_tool_call_id(name: str) -> str:
    ts_ms = int(time.time() * 1000)
    n = next(_TOOL_CALL_COUNTER)
    return f"{name}_{ts_ms}_{n}"
```

Add a method to `GeminiCodeAssistProvider`:

```python
    def _build_envelope(self, request: ChatRequest) -> dict[str, Any]:
        # Reuse Phase 2b's GeminiProvider serializer for the inner request body.
        # We piggy-back via a transient instance — no network, just composition.
        inner = GeminiProvider(
            api_key="unused", model=self.model, base_url=self.base_url
        )._build_body(request)
        return {
            "project": self.project_id,
            "model": request.model or self.model,
            "request": inner,
            "userAgent": "motosan-ai",
            "requestId": _gen_request_id(),
        }
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_code_assist_request.py -v`
Expected: 9 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini_code_assist.py sdks/python/tests/test_code_assist_request.py
git commit -m "feat(python,code-assist): envelope wrapper + request/tool_call ID generators"
```

---

## Task 3: SSE stream adapter — `_parse_sse_event`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini_code_assist.py`
- Create: `sdks/python/tests/test_code_assist_stream.py`

Per Rust adapter (lines 200-313): each SSE `data:` line contains `{response: {candidates, usageMetadata}}`. We unwrap the outer `response` field, then process the inner shape (matches vanilla Gemini).

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_code_assist_stream.py`:

```python
from __future__ import annotations

import json

import pytest

from motosan_ai.providers.gemini_code_assist import (
    _CodeAssistAdapterState,
    _parse_sse_event,
)
from motosan_ai.types import StopReason, StreamEvent


def test_empty_data_returns_no_events():
    state = _CodeAssistAdapterState()
    assert _parse_sse_event("", state) == []
    assert _parse_sse_event("[DONE]", state) == []


def test_text_part_emits_text_event():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {"response": {"candidates": [{"content": {"parts": [{"text": "hello"}]}}]}}
    )
    events = _parse_sse_event(payload, state)
    assert len(events) == 1
    assert events[0].content == "hello"
    assert events[0].done is False


def test_function_call_emits_start_args_end_in_order():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {
            "response": {
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
                        }
                    }
                ]
            }
        }
    )
    events = _parse_sse_event(payload, state)
    types = [e.event_type for e in events]
    assert types == ["tool_call_start", "tool_call_args", "tool_call_end"]
    assert events[0].tool_call_name == "get_weather"
    assert events[0].tool_call_id  # generated
    assert json.loads(events[1].tool_call_args_delta) == {"city": "Taipei"}


def test_function_call_uses_api_id_when_present_and_unique():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {
            "response": {
                "candidates": [
                    {
                        "content": {
                            "parts": [
                                {
                                    "functionCall": {
                                        "id": "api-123",
                                        "name": "x",
                                        "args": {},
                                    }
                                }
                            ]
                        }
                    }
                ]
            }
        }
    )
    events = _parse_sse_event(payload, state)
    starts = [e for e in events if e.event_type == "tool_call_start"]
    assert starts[0].tool_call_id == "api-123"


def test_function_call_regenerates_id_on_duplicate_seen():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {
            "response": {
                "candidates": [
                    {
                        "content": {
                            "parts": [
                                {"functionCall": {"id": "dup", "name": "x", "args": {}}}
                            ]
                        }
                    }
                ]
            }
        }
    )
    events1 = _parse_sse_event(payload, state)
    events2 = _parse_sse_event(payload, state)
    id1 = next(e for e in events1 if e.event_type == "tool_call_start").tool_call_id
    id2 = next(e for e in events2 if e.event_type == "tool_call_start").tool_call_id
    assert id1 == "dup"  # first occurrence keeps API id
    assert id2 != "dup"  # duplicate gets a fresh id
    assert id2.startswith("x_")


def test_usage_with_cached_subtracts_from_input_tokens():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {
            "response": {
                "candidates": [{"content": {"parts": [{"text": "ok"}]}}],
                "usageMetadata": {
                    "promptTokenCount": 100,
                    "cachedContentTokenCount": 30,
                    "candidatesTokenCount": 20,
                },
            }
        }
    )
    events = _parse_sse_event(payload, state)
    usage_events = [e for e in events if e.event_type == "usage"]
    assert len(usage_events) == 1
    u = usage_events[0].usage
    # input_tokens = prompt - cached (matches Rust pi-mono behavior)
    assert u.input_tokens == 70
    assert u.output_tokens == 20
    assert u.cache_read_input_tokens == 30


def test_usage_without_cached_returns_full_input_tokens():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {
            "response": {
                "candidates": [{"content": {"parts": [{"text": "ok"}]}}],
                "usageMetadata": {
                    "promptTokenCount": 100,
                    "candidatesTokenCount": 20,
                },
            }
        }
    )
    events = _parse_sse_event(payload, state)
    u = next(e.usage for e in events if e.event_type == "usage")
    assert u.input_tokens == 100
    assert u.cache_read_input_tokens is None


def test_finish_reason_stop_with_tool_call_emits_tool_use():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {
            "response": {
                "candidates": [
                    {
                        "content": {
                            "parts": [
                                {"functionCall": {"name": "x", "args": {}}}
                            ]
                        },
                        "finishReason": "STOP",
                    }
                ]
            }
        }
    )
    events = _parse_sse_event(payload, state)
    done = next(e for e in events if e.done)
    assert done.stop_reason == StopReason.tool_use


def test_finish_reason_stop_without_tool_call_emits_end_turn():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {
            "response": {
                "candidates": [
                    {
                        "content": {"parts": [{"text": "hi"}]},
                        "finishReason": "STOP",
                    }
                ]
            }
        }
    )
    events = _parse_sse_event(payload, state)
    done = next(e for e in events if e.done)
    assert done.stop_reason == StopReason.end_turn


def test_finish_reason_max_tokens():
    state = _CodeAssistAdapterState()
    payload = json.dumps(
        {
            "response": {
                "candidates": [
                    {"content": {"parts": [{"text": "trun"}]}, "finishReason": "MAX_TOKENS"}
                ]
            }
        }
    )
    events = _parse_sse_event(payload, state)
    done = next(e for e in events if e.done)
    assert done.stop_reason == StopReason.max_tokens


def test_chunk_without_response_wrapper_skipped():
    state = _CodeAssistAdapterState()
    payload = json.dumps({"candidates": [{"content": {"parts": [{"text": "x"}]}}]})
    assert _parse_sse_event(payload, state) == []


def test_malformed_json_skipped():
    state = _CodeAssistAdapterState()
    assert _parse_sse_event("not json {", state) == []
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_code_assist_stream.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement parser**

Add to `gemini_code_assist.py`:

```python
import json
from dataclasses import dataclass, field

from motosan_ai.types import StopReason, StreamEvent, Usage


@dataclass
class _CodeAssistAdapterState:
    """Mutable parser state — tracks tool IDs we've seen so we can
    regenerate on duplicate (Code Assist sometimes reuses IDs)."""
    seen_tool_ids: set[str] = field(default_factory=set)


_FINISH_REASON_MAP = {
    "MAX_TOKENS": StopReason.max_tokens,
}


def _parse_sse_event(data: str, state: _CodeAssistAdapterState) -> list[StreamEvent]:
    """Parse one SSE `data:` payload into 0+ StreamEvents.

    Code Assist wraps the standard Gemini response in an outer ``response``
    object; we unwrap it then walk parts, usage, and finishReason in order.
    """
    s = data.strip()
    if not s or s == "[DONE]":
        return []
    try:
        chunk = json.loads(s)
    except json.JSONDecodeError:
        return []

    response_data = chunk.get("response")
    if not isinstance(response_data, dict):
        return []

    candidates = response_data.get("candidates") or []
    if not candidates:
        return []
    candidate = candidates[0]

    parts = (candidate.get("content") or {}).get("parts") or []
    finish_reason = candidate.get("finishReason")

    out: list[StreamEvent] = []
    has_tool_calls = False

    for part in parts:
        text = part.get("text")
        if isinstance(text, str) and text:
            out.append(StreamEvent(content=text, done=False))
        fc = part.get("functionCall")
        if isinstance(fc, dict):
            has_tool_calls = True
            name = fc.get("name") or ""
            args = fc.get("args") or {}
            provided_id = fc.get("id")
            if (
                isinstance(provided_id, str)
                and provided_id
                and provided_id not in state.seen_tool_ids
            ):
                tool_id = provided_id
            else:
                tool_id = _gen_tool_call_id(name)
            state.seen_tool_ids.add(tool_id)
            out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="tool_call_start",
                    tool_call_id=tool_id,
                    tool_call_name=name,
                )
            )
            out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="tool_call_args",
                    tool_call_id=tool_id,
                    tool_call_args_delta=json.dumps(args),
                )
            )
            out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="tool_call_end",
                    tool_call_id=tool_id,
                )
            )

    usage_meta = response_data.get("usageMetadata")
    if isinstance(usage_meta, dict):
        prompt = int(usage_meta.get("promptTokenCount") or 0)
        cached = int(usage_meta.get("cachedContentTokenCount") or 0)
        output = int(usage_meta.get("candidatesTokenCount") or 0)
        # input_tokens excludes cached (matches Rust)
        input_tokens = max(prompt - cached, 0)
        out.append(
            StreamEvent(
                content="",
                done=False,
                event_type="usage",
                usage=Usage(
                    input_tokens=input_tokens,
                    output_tokens=output,
                    cache_read_input_tokens=cached if cached > 0 else None,
                ),
            )
        )

    if finish_reason:
        if has_tool_calls:
            stop = StopReason.tool_use
        elif finish_reason == "STOP":
            stop = StopReason.end_turn
        else:
            stop = _FINISH_REASON_MAP.get(finish_reason, StopReason.other)
        out.append(StreamEvent(content="", done=True, stop_reason=stop))

    return out
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_code_assist_stream.py -v`
Expected: 12 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini_code_assist.py sdks/python/tests/test_code_assist_stream.py
git commit -m "feat(python,code-assist): SSE adapter — unwrap response wrapper, dedup tool IDs, cached subtraction"
```

---

## Task 4: `chat()` and `stream()` HTTP

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini_code_assist.py`
- Modify: `sdks/python/tests/test_code_assist_stream.py`

`stream()` does the actual HTTP. `chat()` collects from `stream()` (mirrors Rust which uses `collect_stream`).

- [ ] **Step 1: Append failing tests**

```python
import httpx
import respx
from motosan_ai.providers.gemini_code_assist import GeminiCodeAssistProvider
from motosan_ai.types import ChatRequest, Message


def _sse_text(*payloads: dict) -> str:
    return "\n".join(f"data: {json.dumps(p)}" for p in payloads) + "\n"


@respx.mock
@pytest.mark.asyncio
async def test_stream_yields_text_then_done():
    sse = _sse_text(
        {"response": {"candidates": [{"content": {"parts": [{"text": "hi"}]}, "finishReason": "STOP"}]}}
    )
    respx.post(
        "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse"
    ).mock(
        return_value=httpx.Response(
            200, text=sse, headers={"content-type": "text/event-stream"}
        )
    )
    p = GeminiCodeAssistProvider("ya29.fake", "myproj")
    events = [e async for e in p.stream(ChatRequest(messages=[Message.user("hi")]))]
    text = "".join(e.content for e in events if e.event_type == "text" and not e.done)
    assert text == "hi"
    assert events[-1].done is True
    assert events[-1].stop_reason == StopReason.end_turn


@respx.mock
@pytest.mark.asyncio
async def test_chat_collects_stream_into_response():
    sse = _sse_text(
        {"response": {"candidates": [{"content": {"parts": [{"text": "Hello "}]}}]}},
        {"response": {"candidates": [{"content": {"parts": [{"text": "world."}]}}], "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 2}}},
        {"response": {"candidates": [{"content": {"parts": []}, "finishReason": "STOP"}]}},
    )
    respx.post(
        "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse"
    ).mock(
        return_value=httpx.Response(
            200, text=sse, headers={"content-type": "text/event-stream"}
        )
    )
    p = GeminiCodeAssistProvider("ya29.fake", "myproj")
    resp = await p.chat(ChatRequest(messages=[Message.user("hi")]))
    assert resp.content == "Hello world."
    assert resp.usage.input_tokens == 5
    assert resp.usage.output_tokens == 2
    assert resp.stop_reason == StopReason.end_turn


@respx.mock
@pytest.mark.asyncio
async def test_stream_401_raises_auth_error():
    from motosan_ai.error import AuthError

    respx.post(
        "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse"
    ).mock(
        return_value=httpx.Response(401, json={"error": {"message": "expired token"}})
    )
    p = GeminiCodeAssistProvider("ya29.fake", "myproj")
    with pytest.raises(AuthError):
        async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
            pass


@respx.mock
@pytest.mark.asyncio
async def test_stream_sends_envelope_in_body():
    captured = {}

    def _capture(request):
        captured["body"] = json.loads(request.content)
        return httpx.Response(
            200,
            text=_sse_text({"response": {"candidates": [{"content": {"parts": [{"text": "x"}]}, "finishReason": "STOP"}]}}),
            headers={"content-type": "text/event-stream"},
        )

    respx.post(
        "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse"
    ).mock(side_effect=_capture)
    p = GeminiCodeAssistProvider("ya29.fake", "myproj")
    async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
        pass
    body = captured["body"]
    assert body["project"] == "myproj"
    assert body["userAgent"] == "motosan-ai"
    assert "request" in body
    assert "contents" in body["request"]
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_code_assist_stream.py -v -k "stream_yields or chat_collects or stream_401 or stream_sends"`
Expected: AttributeError on `p.stream`.

- [ ] **Step 3: Implement `chat()` + `stream()`**

Add to `GeminiCodeAssistProvider`:

```python
from collections.abc import AsyncIterator

from motosan_ai.error import AuthError, NetworkError, ProviderError, RateLimitError
from motosan_ai.types import ChatResponse, ToolCall


@staticmethod
def _map_http_error(status: int, message: str) -> Exception:
    if status == 401:
        return AuthError(message)
    if status == 429:
        return RateLimitError(message)
    return ProviderError(message)


async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
    body = self._build_envelope(request)
    try:
        resp = await self._http.send(
            self._http.build_request(
                "POST", self._stream_url(), headers=self._headers(), json=body
            ),
            stream=True,
        )
    except httpx.HTTPError as exc:
        raise NetworkError(str(exc)) from exc

    if not resp.is_success:
        error_body = await resp.aread()
        raise self._map_http_error(resp.status_code, error_body.decode())

    state = _CodeAssistAdapterState()
    async for line in resp.aiter_lines():
        if not line.startswith("data: "):
            continue
        data = line[len("data: ") :]
        for event in _parse_sse_event(data, state):
            yield event
            if event.done:
                return


async def chat(self, request: ChatRequest) -> ChatResponse:
    content = ""
    tool_calls: list[ToolCall] = []
    usage = Usage(0, 0)
    stop_reason = StopReason.end_turn
    current_tc_id = ""
    current_tc_name = ""
    current_tc_args = ""

    async for event in self.stream(request):
        if event.event_type == "text" and event.content:
            content += event.content
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
        elif event.event_type == "usage" and event.usage is not None:
            usage = event.usage
        if event.done and event.stop_reason is not None:
            stop_reason = event.stop_reason

    return ChatResponse(
        content=content,
        tool_calls=tool_calls,
        model=request.model or self.model,
        usage=usage,
        stop_reason=stop_reason,
    )
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_code_assist_stream.py tests/test_code_assist_request.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini_code_assist.py sdks/python/tests/test_code_assist_stream.py
git commit -m "feat(python,code-assist): chat() + stream() HTTP with SSE adapter"
```

---

## Task 5: `Provider.gemini_code_assist` dispatch

**Files:**
- Modify: `sdks/python/motosan_ai/client.py`
- Modify: `sdks/python/motosan_ai/providers/__init__.py`
- Modify: `sdks/python/motosan_ai/__init__.py`
- Create: `sdks/python/tests/test_code_assist_dispatch.py`

Adds `Provider.gemini_code_assist` and a `Client.gemini_code_assist(access_token=, project_id=, ...)` classmethod. **No automatic OAuth in this task** — that wires in Task 13 after the OAuth module lands. For now, callers pass an access_token.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_code_assist_dispatch.py`:

```python
from __future__ import annotations

import pytest

from motosan_ai import Client, Provider
from motosan_ai.error import ConfigError
from motosan_ai.providers.gemini_code_assist import GeminiCodeAssistProvider


def test_provider_enum_has_gemini_code_assist():
    assert Provider.gemini_code_assist == "gemini_code_assist"


def test_client_gemini_code_assist_classmethod():
    c = Client.gemini_code_assist(access_token="ya29.fake", project_id="myproj")
    assert c.provider == Provider.gemini_code_assist
    assert isinstance(c._provider, GeminiCodeAssistProvider)
    assert c._provider.access_token == "ya29.fake"
    assert c._provider.project_id == "myproj"


def test_client_gemini_code_assist_requires_project_id():
    with pytest.raises(ConfigError, match="project_id"):
        Client.gemini_code_assist(access_token="ya29.fake", project_id=None)


def test_client_gemini_code_assist_requires_access_token():
    with pytest.raises(ConfigError, match="access_token"):
        Client.gemini_code_assist(access_token=None, project_id="myproj")
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_code_assist_dispatch.py -v`
Expected: AttributeError on `Provider.gemini_code_assist`.

- [ ] **Step 3: Wire dispatch**

Edit `sdks/python/motosan_ai/client.py`. Add to `Provider`:

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
```

In `Client.__init__`, add a parameter `access_token: str | None = None` and `project_id: str | None = None` after `binary_path`. Add a dispatch branch alongside `codex_cli`:

```python
        if provider_value == Provider.gemini_code_assist:
            from motosan_ai.providers.gemini_code_assist import (
                GeminiCodeAssistProvider,
            )

            if not access_token:
                raise ConfigError("gemini_code_assist requires access_token")
            if not project_id:
                raise ConfigError("gemini_code_assist requires project_id")

            self.provider = provider_value
            self.model = model
            self._max_retries = max_retries
            self.api_key = ""
            self._provider = GeminiCodeAssistProvider(
                access_token=access_token,
                project_id=project_id,
                model=model,
                base_url=base_url,
            )
            return
```

Add classmethod:

```python
    @classmethod
    def gemini_code_assist(
        cls,
        access_token: str | None = None,
        project_id: str | None = None,
        model: str | None = None,
        base_url: str | None = None,
        max_retries: int = 3,
    ) -> Client:
        return cls(
            provider=Provider.gemini_code_assist,
            access_token=access_token,
            project_id=project_id,
            model=model,
            base_url=base_url,
            max_retries=max_retries,
        )
```

Edit `motosan_ai/providers/__init__.py` and `motosan_ai/__init__.py` to export `GeminiCodeAssistProvider`.

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_code_assist_dispatch.py tests/test_client_integration.py -v`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/ sdks/python/tests/test_code_assist_dispatch.py
git commit -m "feat(python,code-assist): register Provider.gemini_code_assist + Client classmethod"
```

---

## Part B: OAuth PKCE flow (Tasks 6-11)

## Task 6: PKCE generator (`oauth/_pkce.py`)

**Files:**
- Create: `sdks/python/motosan_ai/oauth/__init__.py` (empty for now)
- Create: `sdks/python/motosan_ai/oauth/_pkce.py`
- Create: `sdks/python/tests/test_oauth_pkce.py`

64 random bytes → URL-safe base64 (no pad) → SHA256 → URL-safe base64 (no pad). Mirrors Rust pkce.rs.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_oauth_pkce.py`:

```python
from __future__ import annotations

import base64
import hashlib

from motosan_ai.oauth._pkce import Pkce


def test_verifier_is_base64url_no_pad():
    p = Pkce.generate()
    assert all(c.isalnum() or c in "-_" for c in p.verifier)
    assert "=" not in p.verifier
    # 64 bytes → 86 base64 chars (no pad)
    assert len(p.verifier) == 86


def test_challenge_matches_s256_of_verifier():
    p = Pkce.generate()
    expected = (
        base64.urlsafe_b64encode(hashlib.sha256(p.verifier.encode()).digest())
        .rstrip(b"=")
        .decode()
    )
    assert p.challenge == expected


def test_challenge_is_base64url_no_pad():
    p = Pkce.generate()
    assert all(c.isalnum() or c in "-_" for c in p.challenge)
    assert "=" not in p.challenge


def test_each_generate_is_unique():
    a = Pkce.generate()
    b = Pkce.generate()
    assert a.verifier != b.verifier
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_oauth_pkce.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement**

Create `sdks/python/motosan_ai/oauth/__init__.py`:

```python
# Phase 3d — Google OAuth flow. Public exports added incrementally.
```

Create `sdks/python/motosan_ai/oauth/_pkce.py`:

```python
from __future__ import annotations

import base64
import hashlib
import secrets
from dataclasses import dataclass


@dataclass(frozen=True)
class Pkce:
    verifier: str
    challenge: str

    @classmethod
    def generate(cls) -> Pkce:
        verifier_bytes = secrets.token_bytes(64)
        verifier = base64.urlsafe_b64encode(verifier_bytes).rstrip(b"=").decode("ascii")
        challenge = (
            base64.urlsafe_b64encode(hashlib.sha256(verifier.encode("ascii")).digest())
            .rstrip(b"=")
            .decode("ascii")
        )
        return cls(verifier=verifier, challenge=challenge)
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_oauth_pkce.py -v`
Expected: 4 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/oauth/ sdks/python/tests/test_oauth_pkce.py
git commit -m "feat(python,oauth): PKCE challenge/verifier generator"
```

---

## Task 7: `Token` type + cache file persistence

**Files:**
- Modify: `sdks/python/motosan_ai/oauth/__init__.py`
- Create: `sdks/python/motosan_ai/oauth/google.py` (skeleton — `Token`, cache helpers)
- Create: `sdks/python/tests/test_oauth_token.py`

`Token` mirrors Rust's struct. Cache lives at `~/.config/motosan-ai/google-tokens.json`.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_oauth_token.py`:

```python
from __future__ import annotations

import json
import time

import pytest

from motosan_ai.oauth.google import (
    DEFAULT_CACHE_PATH,
    Token,
    load_cached_token,
    save_token,
)


def test_token_not_expired_when_just_issued():
    t = Token(
        access_token="a",
        refresh_token="r",
        id_token=None,
        expires_in=3600,
        issued_at=int(time.time()),
    )
    assert not t.is_expired()


def test_token_expired_when_within_buffer():
    t = Token(
        access_token="a",
        refresh_token="r",
        id_token=None,
        expires_in=30,  # within 60s buffer
        issued_at=int(time.time()),
    )
    assert t.is_expired()


def test_token_expired_when_issued_at_zero():
    t = Token(
        access_token="a",
        refresh_token="r",
        id_token=None,
        expires_in=3600,
        issued_at=0,
    )
    assert t.is_expired()


def test_default_cache_path_under_home_config():
    assert DEFAULT_CACHE_PATH.parent.name == "motosan-ai"
    assert DEFAULT_CACHE_PATH.name == "google-tokens.json"


def test_save_and_load_roundtrip(tmp_path):
    cache_path = tmp_path / "tokens.json"
    t = Token(
        access_token="abc",
        refresh_token="ref",
        id_token="id",
        expires_in=3600,
        issued_at=12345,
    )
    save_token(t, path=cache_path)
    loaded = load_cached_token(path=cache_path)
    assert loaded == t


def test_load_cached_token_missing_returns_none(tmp_path):
    assert load_cached_token(path=tmp_path / "none.json") is None


def test_save_creates_parent_directory(tmp_path):
    nested = tmp_path / "deeply" / "nested" / "tokens.json"
    save_token(
        Token(access_token="a", refresh_token="r", id_token=None, expires_in=1, issued_at=1),
        path=nested,
    )
    assert nested.exists()


def test_save_token_chmod_user_only(tmp_path):
    """Token file should be 0600 — protect refresh token."""
    import os, stat

    cache_path = tmp_path / "tokens.json"
    save_token(
        Token(access_token="a", refresh_token="r", id_token=None, expires_in=1, issued_at=1),
        path=cache_path,
    )
    mode = stat.S_IMODE(os.stat(cache_path).st_mode)
    assert mode == 0o600
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_oauth_token.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement**

Create `sdks/python/motosan_ai/oauth/google.py`:

```python
from __future__ import annotations

import json
import os
import time
from dataclasses import asdict, dataclass
from pathlib import Path

DEFAULT_CACHE_PATH = Path.home() / ".config" / "motosan-ai" / "google-tokens.json"
_EXPIRY_BUFFER_SECS = 60


@dataclass(frozen=True)
class Token:
    access_token: str
    refresh_token: str
    id_token: str | None
    expires_in: int
    issued_at: int

    def is_expired(self) -> bool:
        """True when the token is within the 60s pre-expiry buffer."""
        return int(time.time()) + _EXPIRY_BUFFER_SECS >= self.issued_at + self.expires_in


def save_token(token: Token, *, path: Path = DEFAULT_CACHE_PATH) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(asdict(token), indent=2))
    os.chmod(path, 0o600)


def load_cached_token(*, path: Path = DEFAULT_CACHE_PATH) -> Token | None:
    if not path.exists():
        return None
    data = json.loads(path.read_text())
    return Token(**data)
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_oauth_token.py -v`
Expected: 8 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/oauth/google.py sdks/python/tests/test_oauth_token.py
git commit -m "feat(python,oauth): Token type + cache file persistence with 0600 mode"
```

---

## Task 8: Loopback callback server

**Files:**
- Create: `sdks/python/motosan_ai/oauth/_callback_server.py`
- Modify: `sdks/python/tests/test_oauth_token.py` (or new file `test_oauth_callback.py`)

Stdlib `http.server.HTTPServer` running on `127.0.0.1:N`. Single-shot: accepts the OAuth callback, captures `code` + `state` query params, returns 200 with a small HTML page, then shuts down. Wrapped in async via `asyncio.to_thread`.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_oauth_callback.py`:

```python
from __future__ import annotations

import asyncio

import httpx
import pytest

from motosan_ai.oauth._callback_server import bind, wait_for_callback


@pytest.mark.asyncio
async def test_bind_returns_port_in_loopback_range():
    server = await bind(port=None)
    try:
        assert 1024 <= server.port <= 65535
    finally:
        server.close()


@pytest.mark.asyncio
async def test_callback_captures_code_and_state():
    server = await bind(port=None)
    port = server.port

    async def fire_callback() -> None:
        # Wait briefly for server thread to start
        await asyncio.sleep(0.1)
        async with httpx.AsyncClient() as client:
            await client.get(
                f"http://127.0.0.1:{port}/auth/callback",
                params={"code": "auth-code-xyz", "state": "state-abc"},
            )

    fire_task = asyncio.create_task(fire_callback())
    code, state = await asyncio.wait_for(wait_for_callback(server), timeout=5.0)
    await fire_task
    assert code == "auth-code-xyz"
    assert state == "state-abc"


@pytest.mark.asyncio
async def test_callback_serves_success_html():
    server = await bind(port=None)
    port = server.port

    async def fire_then_check_response() -> str:
        await asyncio.sleep(0.1)
        async with httpx.AsyncClient() as client:
            r = await client.get(
                f"http://127.0.0.1:{port}/auth/callback",
                params={"code": "c", "state": "s"},
            )
            return r.text

    response_task = asyncio.create_task(fire_then_check_response())
    await asyncio.wait_for(wait_for_callback(server), timeout=5.0)
    body = await response_task
    assert "success" in body.lower() or "complete" in body.lower()
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_oauth_callback.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement callback server**

Create `sdks/python/motosan_ai/oauth/_callback_server.py`:

```python
"""Single-shot loopback HTTP server for OAuth callback capture.

Binds to ``127.0.0.1:<port>`` (random ephemeral by default), waits for a
single request to ``/auth/callback?code=...&state=...``, captures the
query params, returns a small success page, then shuts down.
"""

from __future__ import annotations

import asyncio
import threading
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlparse

_SUCCESS_PAGE = b"""<!doctype html>
<html><body>
<h1>Authentication complete</h1>
<p>You can close this window and return to the terminal.</p>
</body></html>
"""


@dataclass
class BoundServer:
    port: int
    _server: HTTPServer
    _thread: threading.Thread
    _result: asyncio.Future[tuple[str, str]]

    def close(self) -> None:
        self._server.shutdown()
        self._server.server_close()
        if self._thread.is_alive():
            self._thread.join(timeout=2.0)


async def bind(port: int | None) -> BoundServer:
    """Bind the loopback server. Pass ``port=None`` for random ephemeral."""
    loop = asyncio.get_running_loop()
    result: asyncio.Future[tuple[str, str]] = loop.create_future()

    class _Handler(BaseHTTPRequestHandler):
        # Suppress default stderr logging
        def log_message(self, format: str, *args: object) -> None:
            pass

        def do_GET(self) -> None:
            parsed = urlparse(self.path)
            if parsed.path != "/auth/callback":
                self.send_response(404)
                self.end_headers()
                return
            qs = parse_qs(parsed.query)
            code = qs.get("code", [""])[0]
            state = qs.get("state", [""])[0]
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.wfile.write(_SUCCESS_PAGE)
            if not result.done():
                loop.call_soon_threadsafe(result.set_result, (code, state))

    server = HTTPServer(("127.0.0.1", port or 0), _Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return BoundServer(
        port=server.server_address[1],
        _server=server,
        _thread=thread,
        _result=result,
    )


async def wait_for_callback(server: BoundServer) -> tuple[str, str]:
    """Await the OAuth callback. Returns ``(code, state)``."""
    try:
        return await server._result
    finally:
        server.close()
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_oauth_callback.py -v`
Expected: 3 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/oauth/_callback_server.py sdks/python/tests/test_oauth_callback.py
git commit -m "feat(python,oauth): single-shot loopback callback server"
```

---

## Task 9: `OAuthConfig` + Google Gemini config preset

**Files:**
- Modify: `sdks/python/motosan_ai/oauth/google.py`
- Modify: `sdks/python/tests/test_oauth_token.py` (append)

Mirror Rust's `OAuthConfig` + `gemini()` preset with the public Gemini-CLI client_id/secret.

- [ ] **Step 1: Append failing tests**

```python
from motosan_ai.oauth.google import OAuthConfig, google_gemini_config


def test_google_gemini_config_has_public_client_id():
    cfg = google_gemini_config()
    assert "681255809395" in cfg.client_id


def test_google_gemini_config_has_client_secret():
    cfg = google_gemini_config()
    assert cfg.client_secret is not None


def test_google_gemini_config_auth_url_is_google():
    cfg = google_gemini_config()
    assert "accounts.google.com" in cfg.auth_url


def test_google_gemini_config_token_url_is_google():
    cfg = google_gemini_config()
    assert cfg.token_url == "https://oauth2.googleapis.com/token"


def test_google_gemini_config_scopes_include_cloud_platform():
    cfg = google_gemini_config()
    assert any("cloud-platform" in s for s in cfg.scopes)
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_oauth_token.py -v -k "google_gemini"`
Expected: ImportError.

- [ ] **Step 3: Implement**

Append to `motosan_ai/oauth/google.py`:

```python
from typing import Sequence


@dataclass(frozen=True)
class OAuthConfig:
    client_id: str
    client_secret: str | None
    auth_url: str
    token_url: str
    scopes: Sequence[str]
    redirect_port: int | None = None


def google_gemini_config() -> OAuthConfig:
    """Public Gemini-CLI OAuth credentials.

    These are intentionally distributed in client software per Google's
    OAuth2 documentation for installed apps. Source: Gemini CLI
    open-source project (https://github.com/google-gemini/gemini-cli).
    """
    return OAuthConfig(
        client_id="681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com",
        client_secret="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",
        auth_url="https://accounts.google.com/o/oauth2/auth",
        token_url="https://oauth2.googleapis.com/token",
        scopes=(
            "https://www.googleapis.com/auth/cloud-platform",
            "https://www.googleapis.com/auth/userinfo.email",
            "https://www.googleapis.com/auth/userinfo.profile",
        ),
    )
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_oauth_token.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/oauth/google.py sdks/python/tests/test_oauth_token.py
git commit -m "feat(python,oauth): OAuthConfig + Google Gemini-CLI preset"
```

---

## Task 10: Token exchange + refresh HTTP

**Files:**
- Modify: `sdks/python/motosan_ai/oauth/google.py`
- Create: `sdks/python/tests/test_oauth_google.py`

POST to `token_url` with `grant_type=authorization_code` (login) or `grant_type=refresh_token` (refresh). Response → `Token` with `issued_at = int(time.time())`.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_oauth_google.py`:

```python
from __future__ import annotations

import time

import httpx
import pytest
import respx

from motosan_ai.oauth.google import (
    Token,
    exchange_code,
    google_gemini_config,
    refresh_token,
)


@respx.mock
@pytest.mark.asyncio
async def test_exchange_code_posts_to_token_url():
    cfg = google_gemini_config()
    route = respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200,
            json={
                "access_token": "ya29.new",
                "refresh_token": "1//ref",
                "expires_in": 3600,
                "id_token": "eyJ...",
            },
        )
    )
    token = await exchange_code(
        cfg, code="auth-code", verifier="ver", redirect_uri="http://127.0.0.1:9999/auth/callback"
    )
    assert token.access_token == "ya29.new"
    assert token.refresh_token == "1//ref"
    assert token.expires_in == 3600
    assert token.id_token == "eyJ..."
    assert abs(token.issued_at - int(time.time())) < 5

    # Verify the body shape
    sent = route.calls[0].request
    assert sent.headers["content-type"].startswith("application/x-www-form-urlencoded")
    body = sent.content.decode()
    assert "grant_type=authorization_code" in body
    assert "code=auth-code" in body
    assert "code_verifier=ver" in body
    assert f"client_id={cfg.client_id}" in body


@respx.mock
@pytest.mark.asyncio
async def test_exchange_code_400_raises():
    from motosan_ai.error import AuthError

    cfg = google_gemini_config()
    respx.post(cfg.token_url).mock(
        return_value=httpx.Response(400, json={"error": "invalid_grant"})
    )
    with pytest.raises(AuthError, match="invalid_grant"):
        await exchange_code(
            cfg, code="bad", verifier="v", redirect_uri="http://127.0.0.1:0/cb"
        )


@respx.mock
@pytest.mark.asyncio
async def test_refresh_token_uses_refresh_grant_type():
    cfg = google_gemini_config()
    route = respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200,
            json={
                "access_token": "ya29.refreshed",
                "expires_in": 3600,
                # Refresh response often omits refresh_token; we should keep the old one
            },
        )
    )
    token = await refresh_token(cfg, refresh_token_value="old-refresh")
    assert token.access_token == "ya29.refreshed"
    assert token.refresh_token == "old-refresh"  # preserved when not returned

    body = route.calls[0].request.content.decode()
    assert "grant_type=refresh_token" in body
    assert "refresh_token=old-refresh" in body


@respx.mock
@pytest.mark.asyncio
async def test_refresh_token_uses_returned_refresh_when_present():
    cfg = google_gemini_config()
    respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200,
            json={
                "access_token": "ya29.refreshed",
                "refresh_token": "1//new-ref",
                "expires_in": 3600,
            },
        )
    )
    token = await refresh_token(cfg, refresh_token_value="old-refresh")
    assert token.refresh_token == "1//new-ref"
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_oauth_google.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement**

Append to `motosan_ai/oauth/google.py`:

```python
import httpx

from motosan_ai.error import AuthError, NetworkError


async def exchange_code(
    config: OAuthConfig,
    *,
    code: str,
    verifier: str,
    redirect_uri: str,
) -> Token:
    """Exchange an authorization code for a Token via POST to token_url."""
    data = {
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": redirect_uri,
        "code_verifier": verifier,
        "client_id": config.client_id,
    }
    if config.client_secret:
        data["client_secret"] = config.client_secret

    return await _post_token(config, data)


async def refresh_token(
    config: OAuthConfig,
    *,
    refresh_token_value: str,
) -> Token:
    """Refresh an access token. Returned ``refresh_token`` falls back to
    the input value when the server response omits it (Google sometimes does)."""
    data = {
        "grant_type": "refresh_token",
        "refresh_token": refresh_token_value,
        "client_id": config.client_id,
    }
    if config.client_secret:
        data["client_secret"] = config.client_secret

    token = await _post_token(config, data)
    if not token.refresh_token:
        # Preserve the inbound refresh token when server didn't issue a new one
        token = Token(
            access_token=token.access_token,
            refresh_token=refresh_token_value,
            id_token=token.id_token,
            expires_in=token.expires_in,
            issued_at=token.issued_at,
        )
    return token


async def _post_token(config: OAuthConfig, data: dict[str, str]) -> Token:
    async with httpx.AsyncClient(timeout=30.0) as client:
        try:
            resp = await client.post(
                config.token_url,
                data=data,
                headers={"content-type": "application/x-www-form-urlencoded"},
            )
        except httpx.HTTPError as exc:
            raise NetworkError(f"OAuth token request failed: {exc}") from exc

    if resp.status_code != 200:
        try:
            err = resp.json()
            msg = err.get("error_description") or err.get("error") or resp.text
        except Exception:
            msg = resp.text
        raise AuthError(f"OAuth token exchange failed ({resp.status_code}): {msg}")

    payload = resp.json()
    return Token(
        access_token=payload["access_token"],
        refresh_token=payload.get("refresh_token", ""),
        id_token=payload.get("id_token"),
        expires_in=int(payload.get("expires_in", 3600)),
        issued_at=int(time.time()),
    )
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_oauth_google.py -v`
Expected: 4 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/oauth/google.py sdks/python/tests/test_oauth_google.py
git commit -m "feat(python,oauth): exchange_code + refresh_token HTTP with AuthError mapping"
```

---

## Task 11: `login()` orchestration — auth URL + browser open + callback wait + state validate

**Files:**
- Modify: `sdks/python/motosan_ai/oauth/google.py`
- Modify: `sdks/python/tests/test_oauth_google.py`

Combines PKCE + callback server + browser open + token exchange. State validation rejects mismatched callbacks.

- [ ] **Step 1: Append failing tests**

```python
from unittest.mock import AsyncMock, patch


@respx.mock
@pytest.mark.asyncio
async def test_login_full_flow_with_mocked_browser_and_callback():
    """End-to-end login() with mocked browser-open and callback fired locally."""
    cfg = google_gemini_config()

    # Mock the browser-open hook so it programmatically fires the callback
    async def fake_open_and_callback(auth_url: str, redirect_uri: str) -> None:
        # Extract the state param from auth_url so we echo the right one back
        from urllib.parse import parse_qs, urlparse

        qs = parse_qs(urlparse(auth_url).query)
        state = qs["state"][0]
        # Wait for server to be listening, then POST the callback
        import asyncio as _asyncio

        await _asyncio.sleep(0.1)
        async with httpx.AsyncClient() as c:
            await c.get(redirect_uri, params={"code": "test-code", "state": state})

    respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200,
            json={
                "access_token": "ya29.new",
                "refresh_token": "1//ref",
                "expires_in": 3600,
            },
        )
    )

    from motosan_ai.oauth.google import login

    token = await login(cfg, _open_browser=fake_open_and_callback)
    assert token.access_token == "ya29.new"


@pytest.mark.asyncio
async def test_login_rejects_state_mismatch():
    """If the callback returns a state that doesn't match what we sent, raise."""
    cfg = google_gemini_config()
    from motosan_ai.error import AuthError

    async def fire_wrong_state(auth_url: str, redirect_uri: str) -> None:
        import asyncio as _asyncio

        await _asyncio.sleep(0.1)
        async with httpx.AsyncClient() as c:
            await c.get(redirect_uri, params={"code": "c", "state": "WRONG"})

    from motosan_ai.oauth.google import login

    with pytest.raises(AuthError, match="state"):
        await login(cfg, _open_browser=fire_wrong_state)
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_oauth_google.py -v -k "login"`
Expected: ImportError on `login`.

- [ ] **Step 3: Implement `login()`**

Append to `motosan_ai/oauth/google.py`:

```python
import base64
import secrets
import webbrowser
from collections.abc import Awaitable, Callable
from urllib.parse import urlencode

from motosan_ai.oauth._callback_server import bind, wait_for_callback
from motosan_ai.oauth._pkce import Pkce

_LOGIN_TIMEOUT_SECS = 120

# Type alias: optional injection for tests — coroutine that opens the URL
# (or fires the callback programmatically) given (auth_url, redirect_uri).
OpenBrowserFn = Callable[[str, str], Awaitable[None]]


async def login(
    config: OAuthConfig,
    *,
    _open_browser: OpenBrowserFn | None = None,
) -> Token:
    """Run the full PKCE login flow and return a fresh Token.

    The browser is opened via ``webbrowser.open`` by default. Tests may
    inject ``_open_browser`` to fire the callback programmatically.
    """
    pkce = Pkce.generate()
    state = base64.urlsafe_b64encode(secrets.token_bytes(16)).rstrip(b"=").decode("ascii")

    server = await bind(config.redirect_port)
    redirect_uri = f"http://127.0.0.1:{server.port}/auth/callback"
    auth_url = _build_auth_url(config, pkce.challenge, state, redirect_uri)

    if _open_browser is not None:
        # Tests use this to simulate the user clicking through Google
        import asyncio as _asyncio

        _asyncio.create_task(_open_browser(auth_url, redirect_uri))
    else:
        print(f"Open this URL to log in:\n\n  {auth_url}\n")
        webbrowser.open(auth_url)

    try:
        code, returned_state = await asyncio.wait_for(
            wait_for_callback(server), timeout=_LOGIN_TIMEOUT_SECS
        )
    except asyncio.TimeoutError as exc:
        raise AuthError(
            f"OAuth login timed out after {_LOGIN_TIMEOUT_SECS}s"
        ) from exc

    if returned_state != state:
        raise AuthError(
            f"OAuth state mismatch: sent {state!r}, got {returned_state!r}"
        )

    return await exchange_code(
        config, code=code, verifier=pkce.verifier, redirect_uri=redirect_uri
    )


def _build_auth_url(
    config: OAuthConfig,
    challenge: str,
    state: str,
    redirect_uri: str,
) -> str:
    params = {
        "client_id": config.client_id,
        "response_type": "code",
        "redirect_uri": redirect_uri,
        "scope": " ".join(config.scopes),
        "state": state,
        "code_challenge": challenge,
        "code_challenge_method": "S256",
        "access_type": "offline",
    }
    return f"{config.auth_url}?{urlencode(params)}"
```

Add to imports near the top:

```python
import asyncio
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_oauth_google.py -v`
Expected: 6 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/oauth/google.py sdks/python/tests/test_oauth_google.py
git commit -m "feat(python,oauth): login() — PKCE + browser + callback + state validation"
```

---

## Part C: Wire-up + release (Tasks 12-13)

## Task 12: Public OAuth surface + auto-refresh helper

**Files:**
- Modify: `sdks/python/motosan_ai/oauth/__init__.py`
- Modify: `sdks/python/motosan_ai/__init__.py`
- Modify: `sdks/python/tests/test_oauth_google.py`

Export `Token`, `login`, `refresh_token`, `load_cached_token`, `save_token`, `google_gemini_config` from `motosan_ai.oauth`. Add a convenience `ensure_fresh_token(config)` that loads cache, refreshes if expired, returns Token.

- [ ] **Step 1: Append failing tests**

```python
@respx.mock
@pytest.mark.asyncio
async def test_ensure_fresh_token_returns_cached_when_valid(tmp_path):
    from motosan_ai.oauth import ensure_fresh_token

    cfg = google_gemini_config()
    cache = tmp_path / "tokens.json"
    fresh = Token(
        access_token="ok",
        refresh_token="r",
        id_token=None,
        expires_in=3600,
        issued_at=int(time.time()),
    )
    save_token(fresh, path=cache)

    token = await ensure_fresh_token(cfg, cache_path=cache)
    assert token.access_token == "ok"


@respx.mock
@pytest.mark.asyncio
async def test_ensure_fresh_token_refreshes_when_expired(tmp_path):
    from motosan_ai.oauth import ensure_fresh_token

    cfg = google_gemini_config()
    cache = tmp_path / "tokens.json"
    expired = Token(
        access_token="old",
        refresh_token="ref",
        id_token=None,
        expires_in=10,
        issued_at=0,  # very old
    )
    save_token(expired, path=cache)

    respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200, json={"access_token": "ya29.new", "expires_in": 3600}
        )
    )

    token = await ensure_fresh_token(cfg, cache_path=cache)
    assert token.access_token == "ya29.new"
    # Refreshed token should also be persisted
    reloaded = load_cached_token(path=cache)
    assert reloaded.access_token == "ya29.new"


@pytest.mark.asyncio
async def test_ensure_fresh_token_raises_when_no_cache(tmp_path):
    from motosan_ai.error import AuthError
    from motosan_ai.oauth import ensure_fresh_token

    cfg = google_gemini_config()
    cache = tmp_path / "missing.json"
    with pytest.raises(AuthError, match="login"):
        await ensure_fresh_token(cfg, cache_path=cache)
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_oauth_google.py -v -k "ensure_fresh"`
Expected: ImportError.

- [ ] **Step 3: Implement `ensure_fresh_token` + exports**

Append to `motosan_ai/oauth/google.py`:

```python
async def ensure_fresh_token(
    config: OAuthConfig,
    *,
    cache_path: Path = DEFAULT_CACHE_PATH,
) -> Token:
    """Load a cached token, refreshing if expired. Persists the result.

    Raises ``AuthError`` if no cached token exists — caller should ``login()``
    first.
    """
    cached = load_cached_token(path=cache_path)
    if cached is None:
        raise AuthError(
            f"no cached OAuth token at {cache_path}; run login() first"
        )
    if not cached.is_expired():
        return cached
    fresh = await refresh_token(config, refresh_token_value=cached.refresh_token)
    save_token(fresh, path=cache_path)
    return fresh
```

Rewrite `motosan_ai/oauth/__init__.py`:

```python
from motosan_ai.oauth.google import (
    DEFAULT_CACHE_PATH,
    OAuthConfig,
    Token,
    ensure_fresh_token,
    exchange_code,
    google_gemini_config,
    load_cached_token,
    login,
    refresh_token,
    save_token,
)

__all__ = [
    "DEFAULT_CACHE_PATH",
    "OAuthConfig",
    "Token",
    "ensure_fresh_token",
    "exchange_code",
    "google_gemini_config",
    "load_cached_token",
    "login",
    "refresh_token",
    "save_token",
]
```

Edit `motosan_ai/__init__.py` — add `from motosan_ai import oauth` so callers can do `motosan_ai.oauth.login(...)`. Add `"oauth"` to `__all__`.

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_oauth_google.py tests/test_oauth_token.py tests/test_oauth_pkce.py tests/test_oauth_callback.py -v`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/oauth/ sdks/python/motosan_ai/__init__.py sdks/python/tests/test_oauth_google.py
git commit -m "feat(python,oauth): public surface + ensure_fresh_token convenience"
```

---

## Task 13: Live integration test + release v0.9.3

**Files:**
- Create: `sdks/python/tests/integration/test_code_assist_live.py`
- Modify: `sdks/python/CHANGELOG.md`
- Modify: `sdks/python/pyproject.toml`

- [ ] **Step 1: Create live test**

Create `sdks/python/tests/integration/test_code_assist_live.py`:

```python
"""Live integration test for GeminiCodeAssistProvider.

Skip unless `MOTOSAN_RUN_CODE_ASSIST_LIVE=1` AND a usable cached Google
OAuth token exists at the default cache path AND ``GOOGLE_PROJECT_ID``
is set.

Run via:

    motosan-ai oauth login google   # one-time; opens browser
    GOOGLE_PROJECT_ID=my-project \\
      MOTOSAN_RUN_CODE_ASSIST_LIVE=1 \\
      uv run pytest tests/integration/test_code_assist_live.py -v
"""

from __future__ import annotations

import os

import pytest

from motosan_ai.oauth import (
    DEFAULT_CACHE_PATH,
    ensure_fresh_token,
    google_gemini_config,
)
from motosan_ai.providers.gemini_code_assist import GeminiCodeAssistProvider
from motosan_ai.types import ChatRequest, Message

_RUN = os.environ.get("MOTOSAN_RUN_CODE_ASSIST_LIVE") == "1"
_PROJECT = os.environ.get("GOOGLE_PROJECT_ID")
_TOKEN_PRESENT = DEFAULT_CACHE_PATH.exists()

pytestmark = [
    pytest.mark.skipif(not _RUN, reason="set MOTOSAN_RUN_CODE_ASSIST_LIVE=1 to run"),
    pytest.mark.skipif(_PROJECT is None, reason="GOOGLE_PROJECT_ID not set"),
    pytest.mark.skipif(
        not _TOKEN_PRESENT,
        reason=f"no cached token at {DEFAULT_CACHE_PATH}; run login first",
    ),
    pytest.mark.asyncio,
]


@pytest.fixture
async def provider() -> GeminiCodeAssistProvider:
    cfg = google_gemini_config()
    token = await ensure_fresh_token(cfg)
    return GeminiCodeAssistProvider(
        access_token=token.access_token, project_id=_PROJECT
    )


async def test_live_chat_basic(provider: GeminiCodeAssistProvider):
    resp = await provider.chat(
        ChatRequest(messages=[Message.user("Reply with exactly: PONG")])
    )
    assert "PONG" in resp.content


async def test_live_stream_emits_text_then_done(provider: GeminiCodeAssistProvider):
    events = []
    async for event in provider.stream(
        ChatRequest(messages=[Message.user("Reply with: STREAM_OK")])
    ):
        events.append(event)
    text = "".join(e.content for e in events if e.event_type == "text" and not e.done)
    assert "STREAM_OK" in text
    assert events[-1].done is True
```

- [ ] **Step 2: Bump version**

Edit `sdks/python/pyproject.toml`:

```toml
version = "0.9.3"
```

- [ ] **Step 3: Prepend CHANGELOG entry**

Replace YYYY-MM-DD with the actual release date.

```markdown
## [0.9.3] - YYYY-MM-DD

### Added — `GeminiCodeAssistProvider` + Google OAuth (Phase 3d)
- **`GeminiCodeAssistProvider`** — new HTTP provider targeting `cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse`.
  - Wraps the existing Phase 2b `GeminiProvider._build_body` output in the Code Assist envelope (`{project, model, request, userAgent, requestId}`).
  - Auth: `Authorization: Bearer <token>` plus the `user-agent` / `x-goog-api-client` / `client-metadata` header trio Google's IDE plugins use.
  - Tool-call IDs: prefer `functionCall.id` from the API; regenerate via `{name}_{ts_ms}_{counter}` on missing/empty/duplicate.
  - Usage: `promptTokenCount - cachedContentTokenCount → input_tokens`; `cachedContentTokenCount → cache_read_input_tokens` (None when 0).
  - Capabilities: `with_image()` (matches vanilla Gemini).
- **`motosan_ai.oauth` package** — Google PKCE OAuth flow:
  - `Pkce.generate()` — 64-byte verifier + S256 challenge.
  - `OAuthConfig` + `google_gemini_config()` — public Gemini-CLI client_id/secret per Google's installed-app docs.
  - `Token.is_expired()` — 60s pre-expiry buffer.
  - `_callback_server.bind()` / `wait_for_callback()` — single-shot loopback HTTP server using stdlib `http.server`.
  - `login(config, _open_browser=...)` — full PKCE flow with state validation; 120s callback timeout.
  - `exchange_code(...)` / `refresh_token(...)` — token endpoint HTTP with `AuthError` on 4xx.
  - `save_token(...)` / `load_cached_token(...)` — JSON cache at `~/.config/motosan-ai/google-tokens.json` with `0600` mode.
  - `ensure_fresh_token(...)` — load cache, refresh-if-expired, persist, return.
- **`Provider.gemini_code_assist`** + `Client.gemini_code_assist(access_token=, project_id=, ...)` classmethod. Constructor params `access_token` and `project_id` added to `Client.__init__`.

### Notes
- No new prod dependencies. PKCE uses stdlib `secrets` + `hashlib`; loopback server uses stdlib `http.server`.
- Token cache file is created with `0600` permissions to protect the refresh token.
- The Gemini-CLI client_id/secret are public (Google's installed-app convention) — embedded in source like Rust does.
- Live tests require `MOTOSAN_RUN_CODE_ASSIST_LIVE=1`, a cached token (run `login()` once), and `GOOGLE_PROJECT_ID`.
```

- [ ] **Step 4: Run the gate**

Run: `cd sdks/python && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration/`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/tests/integration/test_code_assist_live.py sdks/python/CHANGELOG.md sdks/python/pyproject.toml
git commit -m "chore(python): release v0.9.3 — Code Assist + Google OAuth (Phase 3d)"
```

---

## Final Self-Review Checklist

Before declaring Phase 3d done, verify:

- [ ] `cd sdks/python && uv run pytest tests/ -v` — all tests pass (target: ~510+ passing).
- [ ] `check-python` gate passes (ruff + format + pytest).
- [ ] **HTTP provider** — `GeminiCodeAssistProvider._build_envelope` produces `{project, model, request, userAgent, requestId}` byte-equivalent to Rust `build_envelope` (gemini_code_assist.rs:80-91).
- [ ] **Auth headers** — `_headers()` emits all 5 headers (auth, user-agent, x-goog-api-client, client-metadata, content-type) with the exact Google strings.
- [ ] **SSE adapter** — outer `response` wrapper unwrap works; tool-call ID dedup works; `cachedContentTokenCount` subtraction matches Rust.
- [ ] **PKCE** — 86-char verifier, S256 challenge byte-equivalent to Rust pkce.rs.
- [ ] **OAuth flow** — state validation rejects mismatch; 120s timeout; callback server returns 200 with success page.
- [ ] **Token cache** — file mode is `0600`; refresh preserves inbound `refresh_token` when server omits it.
- [ ] **`ensure_fresh_token`** — returns cached when valid, refreshes when expired, persists, errors when no cache.
- [ ] **`Provider.gemini_code_assist`** + `Client.gemini_code_assist()` raise `ConfigError` when `access_token` or `project_id` missing.
- [ ] No `TODO` / `FIXME` / placeholder strings introduced.
- [ ] Live test passes when `MOTOSAN_RUN_CODE_ASSIST_LIVE=1`, cache exists, `GOOGLE_PROJECT_ID` set.

If any box is unchecked, fix before tagging/publishing.

---

## What Phase 3d does NOT do

- ❌ **Auto-login from `Client.gemini_code_assist()` when no cached token** — caller must run `motosan_ai.oauth.login(google_gemini_config())` once first. Inline browser-popup from `Client.__init__` would block the event loop and surprise non-interactive callers. A separate convenience helper can be added later.
- ❌ **CLI `motosan-ai oauth login google` command** — the live-test docstring references this for human-friendly login but the actual CLI binary is out of scope. For now: `python -c "import asyncio, motosan_ai.oauth as o; asyncio.run(o.login(o.google_gemini_config()))"`.
- ❌ **Multi-account support / token namespacing** — single token cache per machine. Adding `--account` style namespacing would need `cache_path` parameterization throughout, deferred.
- ❌ **id_token verification** — `id_token` is preserved in the cache for callers who want it but the SDK doesn't validate the JWT signature. Out of scope for SDK-as-API-client.
- ❌ **Refreshing failed-with-401-mid-stream** — current `stream()` raises `AuthError` on 401 and lets the caller decide. Auto-refresh-and-retry on a 401 is a follow-up; would need access to the OAuth config inside the provider, which we deliberately kept out.
- ❌ **Test against real Google accounts in CI** — live test is opt-in only via env var. CI default green without OAuth secrets.

All non-goals tracked in the roadmap doc.
