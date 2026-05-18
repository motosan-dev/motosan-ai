# Python Anthropic OAuth Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add PKCE OAuth login + refresh for Anthropic Claude Pro/Max to the Python SDK, mirroring the Rust `anthropic-oauth` crate, by refactoring `motosan_ai/oauth/` into a generic core plus per-provider config modules.

**Architecture:** Split the `oauth/google.py` monolith into a private generic core (`_flow.py`) and per-provider config modules (`providers/gemini.py`, `providers/anthropic.py`). Add five `OAuthConfig` knobs (`callback_path`, `redirect_uri_host`, `token_body`, `extra_auth_params`, `state_strategy`) — added incrementally one per task so each task leaves the suite green. The Anthropic provider config then expresses Anthropic's OAuth quirks declaratively.

**Tech Stack:** Python 3.11+, `httpx` (async HTTP), `respx` (HTTP mocking in tests), `pytest` + `pytest-asyncio`, `ruff`. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-05-18-python-anthropic-oauth-design.md`

**Engineer pre-flight:** Read the spec end-to-end before starting Task 1. This plan is a faithful Python port of the already-shipped Rust `anthropic-oauth` crate; the *why* behind each knob lives in the spec.

**Test invocation:** All Python tests run from `sdks/python/`. Per-step commands assume you are in that directory unless stated otherwise. The full gate is:

```bash
cd sdks/python
ruff check motosan_ai/
ruff format --check motosan_ai/ tests/
uv run pytest tests/ -q --ignore=tests/integration/
```

---

## File Map

**Create:**
- `sdks/python/motosan_ai/oauth/_flow.py` — generic OAuth core (moved out of `google.py`)
- `sdks/python/motosan_ai/oauth/providers/__init__.py` — package marker
- `sdks/python/motosan_ai/oauth/providers/gemini.py` — `gemini_config()`
- `sdks/python/motosan_ai/oauth/providers/anthropic.py` — `claude_pro_max_config()`
- `sdks/python/tests/test_oauth_flow.py` — unit tests for the generalized core
- `sdks/python/tests/test_oauth_anthropic.py` — Anthropic provider config + refresh tests
- `sdks/python/tests/integration/test_anthropic_oauth_live.py` — skipped-by-default live login test

**Modify:**
- `sdks/python/motosan_ai/oauth/__init__.py` — re-export the new public surface
- `sdks/python/motosan_ai/oauth/_callback_server.py` — parametrize callback path
- `sdks/python/tests/test_oauth_callback.py` — pass `callback_path` to `bind()`, add path-match cases
- `sdks/python/tests/test_oauth_token.py` — switch import to `motosan_ai.oauth`, rename `google_gemini_config` → `gemini_config`
- `sdks/python/README.md` — OAuth section: `gemini_config` rename + Anthropic example
- `sdks/python/CHANGELOG.md` — new version entry
- `sdks/python/pyproject.toml` — minor version bump
- `llms.txt` — Python OAuth reference note

**Rename:**
- `sdks/python/tests/test_oauth_google.py` → `sdks/python/tests/test_oauth_gemini.py`

**Delete:**
- `sdks/python/motosan_ai/oauth/google.py`

---

## Branching

- [ ] **Step 0: Create feature branch**

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai
git checkout main && git pull
git checkout -b feat/python-anthropic-oauth
git push -u origin feat/python-anthropic-oauth
```

Per project rules: every code change goes through PR + CI. This plan produces a single PR.

---

## Task 1: Refactor `oauth/google.py` into generic core + per-provider module (no behavior change)

This is a pure move. `_flow.py` gets the generic code; `providers/gemini.py` gets the one provider config; `google.py` is deleted; `oauth/__init__.py` re-exports; all consumers are updated. The test suite must be green before and after with identical assertions.

**Files:**
- Create: `sdks/python/motosan_ai/oauth/_flow.py`
- Create: `sdks/python/motosan_ai/oauth/providers/__init__.py`
- Create: `sdks/python/motosan_ai/oauth/providers/gemini.py`
- Modify: `sdks/python/motosan_ai/oauth/__init__.py`
- Modify: `sdks/python/tests/test_oauth_token.py`
- Rename: `sdks/python/tests/test_oauth_google.py` → `tests/test_oauth_gemini.py`
- Modify: `sdks/python/tests/integration/test_code_assist_live.py`
- Delete: `sdks/python/motosan_ai/oauth/google.py`

- [ ] **Step 1: Confirm the baseline suite is green**

```bash
cd sdks/python && uv run pytest tests/ -q --ignore=tests/integration/
```

Expected: all pass. Record the pass count — Task 1 must end with the same count (minus none; the renamed file keeps its tests).

- [ ] **Step 2: Create `_flow.py` as a verbatim move of the generic code**

Create `sdks/python/motosan_ai/oauth/_flow.py` containing the **exact current contents of `oauth/google.py` EXCEPT the `google_gemini_config` function**. `google.py` is 228 lines; the `google_gemini_config` function spans lines 49–60. Concretely: copy lines 1–48 (imports through the `OAuthConfig` dataclass and the blank lines after it) and lines 61 through end-of-file (`save_token` through `ensure_fresh_token`), skipping lines 49–60. `ruff format` in Step 11 normalizes any blank-line spacing afterward.

Do not change any code in the moved content. The result is `_flow.py` with: `Token`, `OAuthConfig`, `save_token`, `load_cached_token`, `_post_token`, `exchange_code`, `refresh_token`, `OpenBrowserFn`, `_build_auth_url`, `login`, `ensure_fresh_token`, `DEFAULT_CACHE_PATH`, `_EXPIRY_BUFFER_SECS`, `_LOGIN_TIMEOUT_SECS`.

Note: `DEFAULT_CACHE_PATH` keeps its current value (`~/.config/motosan-ai/google-tokens.json`) — the spec's regression gate requires `test_oauth_token.py`'s `test_default_cache_path_under_home_config` assertion to stay valid. Anthropic callers pass their own `cache_path` to `ensure_fresh_token`.

- [ ] **Step 3: Create `providers/__init__.py`**

Create `sdks/python/motosan_ai/oauth/providers/__init__.py`:

```python
from motosan_ai.oauth.providers.gemini import gemini_config

__all__ = ["gemini_config"]
```

- [ ] **Step 4: Create `providers/gemini.py`**

Create `sdks/python/motosan_ai/oauth/providers/gemini.py` — the former `google_gemini_config`, renamed to `gemini_config`:

```python
from __future__ import annotations

from motosan_ai.oauth._flow import OAuthConfig


def gemini_config() -> OAuthConfig:
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

(Only the original `OAuthConfig` fields are passed here; the five new fields are added to `OAuthConfig` with defaults in Tasks 2–5, and this call site gains each value explicitly as those tasks land.)

- [ ] **Step 5: Rewrite `oauth/__init__.py`**

Replace `sdks/python/motosan_ai/oauth/__init__.py` entirely:

```python
from motosan_ai.oauth._flow import (
    DEFAULT_CACHE_PATH,
    OAuthConfig,
    Token,
    ensure_fresh_token,
    exchange_code,
    load_cached_token,
    login,
    refresh_token,
    save_token,
)
from motosan_ai.oauth.providers.gemini import gemini_config

__all__ = [
    "DEFAULT_CACHE_PATH",
    "OAuthConfig",
    "Token",
    "ensure_fresh_token",
    "exchange_code",
    "gemini_config",
    "load_cached_token",
    "login",
    "refresh_token",
    "save_token",
]
```

- [ ] **Step 6: Delete `google.py`**

```bash
git rm sdks/python/motosan_ai/oauth/google.py
```

- [ ] **Step 7: Update `test_oauth_token.py`**

In `sdks/python/tests/test_oauth_token.py`, change the import block (currently lines 7–14):

```python
from motosan_ai.oauth import (
    DEFAULT_CACHE_PATH,
    OAuthConfig,
    Token,
    gemini_config,
    load_cached_token,
    save_token,
)
```

Then rename every `google_gemini_config(` call to `gemini_config(` in this file. There are five such tests (`test_google_gemini_config_*`). Also rename those five test functions from `test_google_gemini_config_*` to `test_gemini_config_*` for consistency. Assertions are unchanged (e.g. `test_default_cache_path_under_home_config` still expects `google-tokens.json`).

- [ ] **Step 8: Rename and update `test_oauth_google.py`**

```bash
git mv sdks/python/tests/test_oauth_google.py sdks/python/tests/test_oauth_gemini.py
```

In `tests/test_oauth_gemini.py`, change the import block (currently lines 9–18):

```python
from motosan_ai.oauth import (
    Token,
    ensure_fresh_token,
    exchange_code,
    gemini_config,
    load_cached_token,
    login,
    refresh_token,
    save_token,
)
```

Then rename every `google_gemini_config(` call to `gemini_config(` throughout the file (it appears in many tests). Test logic and assertions are otherwise unchanged.

- [ ] **Step 9: Update `test_code_assist_live.py`**

In `sdks/python/tests/integration/test_code_assist_live.py` line 13, change:

```python
from motosan_ai.oauth import DEFAULT_CACHE_PATH, ensure_fresh_token, google_gemini_config
```

to:

```python
from motosan_ai.oauth import DEFAULT_CACHE_PATH, ensure_fresh_token, gemini_config
```

Then rename every `google_gemini_config(` call to `gemini_config(` in that file.

- [ ] **Step 10: Run the full suite**

```bash
cd sdks/python && uv run pytest tests/ -q --ignore=tests/integration/
```

Expected: same pass count as Step 1. Zero behavior change — this task only moved code and renamed one function.

- [ ] **Step 11: Lint + format**

```bash
cd sdks/python && ruff check motosan_ai/ && ruff format motosan_ai/ tests/
```

Expected: clean (or `ruff format` reformats; that's fine — it is applied, not just checked).

- [ ] **Step 12: Commit**

```bash
git add sdks/python/motosan_ai/oauth/ sdks/python/tests/test_oauth_token.py \
        sdks/python/tests/test_oauth_gemini.py \
        sdks/python/tests/integration/test_code_assist_live.py
git commit -m "refactor(python-oauth): split google.py into generic core + providers

oauth/google.py is split into a private generic core (_flow.py) and
a per-provider module (providers/gemini.py), mirroring the Rust
motosan-ai-oauth layout. google_gemini_config is renamed to
gemini_config. Pure move + rename — no behavior change.

Breaking: motosan_ai.oauth no longer exports google_gemini_config;
use gemini_config. In-repo consumers updated in this commit."
```

---

## Task 2: Add `callback_path` knob

**Files:**
- Modify: `sdks/python/motosan_ai/oauth/_callback_server.py`
- Modify: `sdks/python/motosan_ai/oauth/_flow.py`
- Modify: `sdks/python/motosan_ai/oauth/providers/gemini.py`
- Modify: `sdks/python/tests/test_oauth_callback.py`

- [ ] **Step 1: Write failing tests for callback-path matching**

In `sdks/python/tests/test_oauth_callback.py`, add these tests (and keep the existing three — they will be updated in Step 4):

```python
@pytest.mark.asyncio
async def test_callback_matches_configured_path():
    server = await bind(port=None, callback_path="/callback")
    port = server.port

    async def fire() -> None:
        await asyncio.sleep(0.1)
        async with httpx.AsyncClient() as client:
            await client.get(
                f"http://127.0.0.1:{port}/callback",
                params={"code": "c", "state": "s"},
            )

    fire_task = asyncio.create_task(fire())
    code, state = await asyncio.wait_for(wait_for_callback(server), timeout=5.0)
    await fire_task
    assert code == "c"
    assert state == "s"


@pytest.mark.asyncio
async def test_callback_ignores_non_configured_path():
    server = await bind(port=None, callback_path="/callback")
    port = server.port

    async def hit_wrong_then_right() -> None:
        await asyncio.sleep(0.1)
        async with httpx.AsyncClient() as client:
            # Wrong path: must 404 and not resolve the future.
            r = await client.get(f"http://127.0.0.1:{port}/auth/callback")
            assert r.status_code == 404
            # Right path: resolves.
            await client.get(
                f"http://127.0.0.1:{port}/callback",
                params={"code": "ok", "state": "st"},
            )

    fire_task = asyncio.create_task(hit_wrong_then_right())
    code, _ = await asyncio.wait_for(wait_for_callback(server), timeout=5.0)
    await fire_task
    assert code == "ok"
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd sdks/python && uv run pytest tests/test_oauth_callback.py -q
```

Expected: FAIL — `bind()` got an unexpected keyword argument `callback_path`.

- [ ] **Step 3: Add `callback_path` to `_callback_server.py`**

In `sdks/python/motosan_ai/oauth/_callback_server.py`, change `bind`'s signature and the handler comparison. The new `bind`:

```python
async def bind(port: int | None, callback_path: str) -> BoundServer:
    loop = asyncio.get_running_loop()
    result: asyncio.Future[tuple[str, str]] = loop.create_future()

    class _Handler(BaseHTTPRequestHandler):
        def log_message(self, format: str, *args: object) -> None:
            pass

        def do_GET(self) -> None:
            parsed = urlparse(self.path)
            if parsed.path != callback_path:
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
        port=server.server_address[1], _server=server, _thread=thread, _result=result
    )
```

`callback_path` is required (no default) — the spec specifies this; `_callback_server.py` is private plumbing with one production caller.

- [ ] **Step 4: Update the existing three `bind()` calls in `test_oauth_callback.py`**

The existing `test_bind_returns_port_in_loopback_range`, `test_callback_captures_code_and_state`, and `test_callback_serves_success_html` call `bind(port=None)`. Update each to `bind(port=None, callback_path="/auth/callback")`. Their existing assertions (port range, captured `code`/`state`, success HTML) are unchanged.

- [ ] **Step 5: Add `callback_path` field to `OAuthConfig`**

In `sdks/python/motosan_ai/oauth/_flow.py`, add the field to the `OAuthConfig` dataclass (after `redirect_port`):

```python
@dataclass(frozen=True)
class OAuthConfig:
    client_id: str
    client_secret: str | None
    auth_url: str
    token_url: str
    scopes: Sequence[str]
    redirect_port: int | None = None
    callback_path: str = "/auth/callback"
```

- [ ] **Step 6: Wire `callback_path` through `login`**

In `_flow.py`'s `login`, the current line builds `redirect_uri` and calls `bind`:

```python
    server = await bind(config.redirect_port)
    redirect_uri = f"http://127.0.0.1:{server.port}/auth/callback"
```

Change to:

```python
    server = await bind(config.redirect_port, config.callback_path)
    redirect_uri = f"http://127.0.0.1:{server.port}{config.callback_path}"
```

(The `redirect_uri_host` part of this line is generalized in Task 3; for now it stays `127.0.0.1`.)

- [ ] **Step 7: Add `callback_path` to `gemini_config()`**

In `providers/gemini.py`, add `callback_path="/auth/callback",` to the `OAuthConfig(...)` call (after `scopes=...`).

- [ ] **Step 8: Run tests**

```bash
cd sdks/python && uv run pytest tests/test_oauth_callback.py tests/test_oauth_gemini.py -q
```

Expected: all pass, including the two new path-matching tests.

- [ ] **Step 9: Lint + format**

```bash
cd sdks/python && ruff check motosan_ai/ && ruff format motosan_ai/ tests/
```

- [ ] **Step 10: Commit**

```bash
git add sdks/python/motosan_ai/oauth/ sdks/python/tests/test_oauth_callback.py
git commit -m "feat(python-oauth): config-driven callback_path

bind() and login() now take the callback HTTP path from OAuthConfig
instead of hardcoding /auth/callback. Anthropic registers its
callback at /callback."
```

---

## Task 3: Add `redirect_uri_host` + `extra_auth_params` knobs

**Files:**
- Modify: `sdks/python/motosan_ai/oauth/_flow.py`
- Modify: `sdks/python/motosan_ai/oauth/providers/gemini.py`
- Modify: `sdks/python/tests/test_oauth_flow.py` (created here)

- [ ] **Step 1: Create `test_oauth_flow.py` with failing tests**

Create `sdks/python/tests/test_oauth_flow.py`:

```python
from __future__ import annotations

from urllib.parse import parse_qs, urlparse

from motosan_ai.oauth._flow import OAuthConfig, _build_auth_url


def _cfg(**overrides) -> OAuthConfig:
    base = dict(
        client_id="test-client",
        client_secret=None,
        auth_url="https://auth.example.com/authorize",
        token_url="https://auth.example.com/token",
        scopes=("openid",),
    )
    base.update(overrides)
    return OAuthConfig(**base)


def test_build_auth_url_appends_extra_auth_params():
    cfg = _cfg(extra_auth_params=(("foo", "bar"), ("baz", "qux")))
    url = _build_auth_url(cfg, "challenge", "state", "http://127.0.0.1:9999/cb")
    qs = parse_qs(urlparse(url).query)
    assert qs["foo"] == ["bar"]
    assert qs["baz"] == ["qux"]


def test_build_auth_url_no_extra_params_has_no_access_type():
    cfg = _cfg(extra_auth_params=())
    url = _build_auth_url(cfg, "challenge", "state", "http://127.0.0.1:9999/cb")
    qs = parse_qs(urlparse(url).query)
    assert "access_type" not in qs
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd sdks/python && uv run pytest tests/test_oauth_flow.py -q
```

Expected: FAIL — `OAuthConfig` got an unexpected keyword argument `extra_auth_params`.

- [ ] **Step 3: Add `redirect_uri_host` + `extra_auth_params` fields to `OAuthConfig`**

In `_flow.py`, extend the dataclass:

```python
@dataclass(frozen=True)
class OAuthConfig:
    client_id: str
    client_secret: str | None
    auth_url: str
    token_url: str
    scopes: Sequence[str]
    redirect_port: int | None = None
    callback_path: str = "/auth/callback"
    redirect_uri_host: str = "127.0.0.1"
    extra_auth_params: Sequence[tuple[str, str]] = ()
```

- [ ] **Step 4: Generalize `_build_auth_url`**

In `_flow.py`, the current `_build_auth_url` hardcodes `"access_type": "offline"`. Replace it:

```python
def _build_auth_url(config: OAuthConfig, challenge: str, state: str, redirect_uri: str) -> str:
    params = {
        "client_id": config.client_id,
        "response_type": "code",
        "redirect_uri": redirect_uri,
        "scope": " ".join(config.scopes),
        "state": state,
        "code_challenge": challenge,
        "code_challenge_method": "S256",
    }
    for key, value in config.extra_auth_params:
        params[key] = value
    return f"{config.auth_url}?{urlencode(params)}"
```

- [ ] **Step 5: Use `redirect_uri_host` in `login`**

In `_flow.py`'s `login`, the line set in Task 2 Step 6:

```python
    redirect_uri = f"http://127.0.0.1:{server.port}{config.callback_path}"
```

becomes:

```python
    redirect_uri = f"http://{config.redirect_uri_host}:{server.port}{config.callback_path}"
```

The HTTP server still binds to `127.0.0.1` (unchanged in `_callback_server.py`); only the URI string sent to the auth server uses `redirect_uri_host`.

- [ ] **Step 6: Add the two fields to `gemini_config()`**

In `providers/gemini.py`, add to the `OAuthConfig(...)` call:

```python
        redirect_uri_host="127.0.0.1",
        extra_auth_params=(("access_type", "offline"),),
```

This preserves the previous behavior — `_build_auth_url` used to always append `access_type=offline`.

- [ ] **Step 7: Run tests**

```bash
cd sdks/python && uv run pytest tests/test_oauth_flow.py tests/test_oauth_gemini.py -q
```

Expected: all pass. `test_oauth_gemini.py`'s `test_login_full_flow_*` still works (gemini's `extra_auth_params` keeps `access_type=offline`).

- [ ] **Step 8: Lint + format**

```bash
cd sdks/python && ruff check motosan_ai/ && ruff format motosan_ai/ tests/
```

- [ ] **Step 9: Commit**

```bash
git add sdks/python/motosan_ai/oauth/ sdks/python/tests/test_oauth_flow.py
git commit -m "feat(python-oauth): redirect_uri_host + extra_auth_params knobs

_build_auth_url no longer hardcodes access_type=offline; it iterates
config.extra_auth_params. redirect_uri is built from
config.redirect_uri_host (Anthropic registers its redirect URI with
hostname 'localhost'; the bind address stays 127.0.0.1). gemini_config
keeps access_type=offline and host 127.0.0.1 — no behavior change."
```

---

## Task 4: Add `token_body` knob (`TokenBodyFormat`)

**Files:**
- Modify: `sdks/python/motosan_ai/oauth/_flow.py`
- Modify: `sdks/python/motosan_ai/oauth/providers/gemini.py`
- Modify: `sdks/python/tests/test_oauth_flow.py`

- [ ] **Step 1: Add failing tests for token-body format**

Append to `sdks/python/tests/test_oauth_flow.py`:

```python
import httpx
import pytest
import respx

from motosan_ai.oauth._flow import TokenBodyFormat, exchange_code


@respx.mock
@pytest.mark.asyncio
async def test_exchange_code_form_body():
    cfg = _cfg(token_body=TokenBodyFormat.FORM)
    route = respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200, json={"access_token": "AT", "refresh_token": "RT", "expires_in": 3600}
        )
    )
    await exchange_code(cfg, code="C", verifier="V", redirect_uri="http://127.0.0.1/cb")
    req = route.calls[0].request
    assert req.headers["content-type"].startswith("application/x-www-form-urlencoded")
    body = req.content.decode()
    assert "grant_type=authorization_code" in body
    assert "code=C" in body


@respx.mock
@pytest.mark.asyncio
async def test_exchange_code_json_body():
    cfg = _cfg(token_body=TokenBodyFormat.JSON)
    route = respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200, json={"access_token": "AT", "refresh_token": "RT", "expires_in": 3600}
        )
    )
    await exchange_code(cfg, code="C", verifier="V", redirect_uri="http://127.0.0.1/cb")
    req = route.calls[0].request
    assert req.headers["content-type"].startswith("application/json")
    import json as _json

    payload = _json.loads(req.content)
    assert payload["grant_type"] == "authorization_code"
    assert payload["code"] == "C"
```

Note: these two `exchange_code` calls have no `state=` argument — that
parameter does not exist until Task 5. Task 5 Step 1 updates both calls to
add `state="S"` and asserts the state appears in the body. Write them in the
no-`state` form shown above for now.

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd sdks/python && uv run pytest tests/test_oauth_flow.py -q
```

Expected: FAIL — cannot import `TokenBodyFormat` from `motosan_ai.oauth._flow`.

- [ ] **Step 3: Add `TokenBodyFormat` enum and `token_body` field**

In `_flow.py`, add `import enum` to the imports, then add the enum above `OAuthConfig`:

```python
class TokenBodyFormat(enum.Enum):
    FORM = "form"
    JSON = "json"
```

Add the field to `OAuthConfig` (after `redirect_uri_host`):

```python
    token_body: TokenBodyFormat = TokenBodyFormat.FORM
```

(Keep `extra_auth_params` last in the dataclass; insert `token_body` before it.)

- [ ] **Step 4: Generalize `_post_token`**

In `_flow.py`, the current `_post_token` hardcodes `data=` and a form `content-type` header. Replace it:

```python
async def _post_token(config: OAuthConfig, data: dict[str, str]) -> Token:
    async with httpx.AsyncClient(timeout=30.0) as client:
        try:
            if config.token_body is TokenBodyFormat.JSON:
                resp = await client.post(config.token_url, json=data)
            else:
                resp = await client.post(config.token_url, data=data)
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

httpx sets `application/x-www-form-urlencoded` for `data=` and `application/json` for `json=` automatically; the previously hardcoded header is gone.

- [ ] **Step 5: Add `token_body` to `gemini_config()`**

In `providers/gemini.py`, add to the `OAuthConfig(...)` call (before `extra_auth_params`):

```python
        token_body=TokenBodyFormat.FORM,
```

and add the import at the top:

```python
from motosan_ai.oauth._flow import OAuthConfig, TokenBodyFormat
```

- [ ] **Step 6: Run tests**

```bash
cd sdks/python && uv run pytest tests/test_oauth_flow.py tests/test_oauth_gemini.py -q
```

Expected: all pass. `test_oauth_gemini.py`'s `exchange_code` tests still pass (gemini stays on `FORM`).

- [ ] **Step 7: Lint + format**

```bash
cd sdks/python && ruff check motosan_ai/ && ruff format motosan_ai/ tests/
```

- [ ] **Step 8: Commit**

```bash
git add sdks/python/motosan_ai/oauth/ sdks/python/tests/test_oauth_flow.py
git commit -m "feat(python-oauth): TokenBodyFormat.FORM|JSON switch in _post_token

_post_token sends either a form body (data=) or a JSON body (json=)
based on config.token_body. The hardcoded form content-type header
is removed — httpx sets it. Gemini stays on FORM."
```

---

## Task 5: Add `state_strategy` knob (`StateStrategy`) + `state` in token body

**Files:**
- Modify: `sdks/python/motosan_ai/oauth/_flow.py`
- Modify: `sdks/python/motosan_ai/oauth/providers/gemini.py`
- Modify: `sdks/python/tests/test_oauth_flow.py`
- Modify: `sdks/python/tests/test_oauth_gemini.py`

- [ ] **Step 1: Update Task 4's tests to pass `state`, add a state-in-body assertion**

In `sdks/python/tests/test_oauth_flow.py`, update both `exchange_code` calls in `test_exchange_code_form_body` and `test_exchange_code_json_body` to include `state="S"`:

```python
    await exchange_code(
        cfg, code="C", state="S", verifier="V", redirect_uri="http://127.0.0.1/cb"
    )
```

In `test_exchange_code_form_body`, add after the existing body asserts:

```python
    assert "state=S" in body
```

In `test_exchange_code_json_body`, add after the existing payload asserts:

```python
    assert payload["state"] == "S"
```

Then add a test confirming `_build_auth_url` echoes the caller-provided state (no new import needed — `_build_auth_url`, `parse_qs`, `urlparse` are already imported at the top of `test_oauth_flow.py` from Task 3):

```python
def test_build_auth_url_state_is_caller_provided():
    # _build_auth_url echoes whatever state string it is given; the
    # RANDOM vs EQUALS_VERIFIER choice is made in login(), tested below.
    cfg = _cfg()
    url = _build_auth_url(cfg, "challenge", "STATEVALUE", "http://127.0.0.1/cb")
    qs = parse_qs(urlparse(url).query)
    assert qs["state"] == ["STATEVALUE"]
```

Add a `StateStrategy` import to the test file's imports:

```python
from motosan_ai.oauth._flow import StateStrategy
```

And a test that `login` uses the verifier as state under `EQUALS_VERIFIER`:

```python
@respx.mock
@pytest.mark.asyncio
async def test_login_equals_verifier_uses_verifier_as_state():
    from motosan_ai.oauth._flow import login

    cfg = _cfg(
        token_url="https://auth.example.com/token",
        state_strategy=StateStrategy.EQUALS_VERIFIER,
        callback_path="/auth/callback",
    )
    respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200, json={"access_token": "AT", "refresh_token": "RT", "expires_in": 3600}
        )
    )

    captured: dict[str, str] = {}

    async def fake_browser(auth_url: str, redirect_uri: str) -> None:
        import asyncio as _a
        from urllib.parse import parse_qs as _pq, urlencode, urlparse as _up
        from urllib.request import urlopen

        q = _pq(_up(auth_url).query)
        captured["state"] = q["state"][0]
        captured["challenge"] = q["code_challenge"][0]
        await _a.sleep(0.1)
        cb = f"{redirect_uri}?{urlencode({'code': 'c', 'state': q['state'][0]})}"
        await _a.to_thread(lambda: urlopen(cb, timeout=5).read())

    await login(cfg, _open_browser=fake_browser)
    # Under EQUALS_VERIFIER, state must be the PKCE verifier — i.e. the
    # SHA256(state) base64url-encoded must equal the code_challenge.
    import base64
    import hashlib

    expected_challenge = (
        base64.urlsafe_b64encode(hashlib.sha256(captured["state"].encode()).digest())
        .rstrip(b"=")
        .decode()
    )
    assert expected_challenge == captured["challenge"]
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd sdks/python && uv run pytest tests/test_oauth_flow.py -q
```

Expected: FAIL — cannot import `StateStrategy`; `exchange_code` has no `state` parameter.

- [ ] **Step 3: Add `StateStrategy` enum and `state_strategy` field**

In `_flow.py`, add the enum (below `TokenBodyFormat`):

```python
class StateStrategy(enum.Enum):
    RANDOM = "random"
    EQUALS_VERIFIER = "verifier"
```

Add the field to `OAuthConfig` as the **last** field (after `extra_auth_params`):

```python
    state_strategy: StateStrategy = StateStrategy.RANDOM
```

- [ ] **Step 4: Add `state` to `exchange_code` and the token body**

In `_flow.py`, the current `exchange_code`:

```python
async def exchange_code(
    config: OAuthConfig, *, code: str, verifier: str, redirect_uri: str
) -> Token:
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
```

becomes:

```python
async def exchange_code(
    config: OAuthConfig, *, code: str, state: str, verifier: str, redirect_uri: str
) -> Token:
    # `state` is echoed in the token POST body to match the Rust
    # anthropic-oauth crate. RFC 6749 §4.1.3 does not require this;
    # Anthropic empirically does. Google tolerates the extra field.
    data = {
        "grant_type": "authorization_code",
        "code": code,
        "state": state,
        "redirect_uri": redirect_uri,
        "code_verifier": verifier,
        "client_id": config.client_id,
    }
    if config.client_secret:
        data["client_secret"] = config.client_secret
    return await _post_token(config, data)
```

- [ ] **Step 5: Derive `state` from `state_strategy` in `login` and pass it to `exchange_code`**

In `_flow.py`'s `login`, the current state generation (near the top):

```python
    pkce = Pkce.generate()
    state = base64.urlsafe_b64encode(secrets.token_bytes(16)).rstrip(b"=").decode("ascii")
```

becomes:

```python
    pkce = Pkce.generate()
    if config.state_strategy is StateStrategy.EQUALS_VERIFIER:
        state = pkce.verifier
    else:
        state = base64.urlsafe_b64encode(secrets.token_bytes(16)).rstrip(b"=").decode("ascii")
```

And `login`'s final line currently:

```python
    return await exchange_code(config, code=code, verifier=pkce.verifier, redirect_uri=redirect_uri)
```

becomes:

```python
    return await exchange_code(
        config, code=code, state=state, verifier=pkce.verifier, redirect_uri=redirect_uri
    )
```

- [ ] **Step 6: Add `state_strategy` to `gemini_config()`**

In `providers/gemini.py`, add `state_strategy=StateStrategy.RANDOM,` as the last argument of the `OAuthConfig(...)` call, and update the import:

```python
from motosan_ai.oauth._flow import OAuthConfig, StateStrategy, TokenBodyFormat
```

- [ ] **Step 7: Update `test_oauth_gemini.py`'s `exchange_code` call**

`test_oauth_gemini.py` has `test_exchange_code_posts_to_token_url` and `test_exchange_code_400_raises`, which call `exchange_code` without `state`. Add `state="..."` to both:

- In `test_exchange_code_posts_to_token_url`:
  ```python
  token = await exchange_code(
      cfg, code="auth-code", state="st", verifier="ver",
      redirect_uri="http://127.0.0.1:9999/auth/callback",
  )
  ```
- In `test_exchange_code_400_raises`:
  ```python
  await exchange_code(
      cfg, code="bad", state="st", verifier="v", redirect_uri="http://127.0.0.1:0/cb"
  )
  ```

Their assertions are unchanged.

- [ ] **Step 8: Run the full unit suite**

```bash
cd sdks/python && uv run pytest tests/ -q --ignore=tests/integration/
```

Expected: all pass.

- [ ] **Step 9: Lint + format**

```bash
cd sdks/python && ruff check motosan_ai/ && ruff format motosan_ai/ tests/
```

- [ ] **Step 10: Commit**

```bash
git add sdks/python/motosan_ai/oauth/ sdks/python/tests/test_oauth_flow.py \
        sdks/python/tests/test_oauth_gemini.py
git commit -m "feat(python-oauth): state_strategy knob + state in token body

login() derives the OAuth state nonce from config.state_strategy:
RANDOM (standard) or EQUALS_VERIFIER (state == PKCE verifier, which
Anthropic's auth endpoint requires). exchange_code() now takes a
state argument and echoes it in the token POST body — the second
empirical fix from the Rust live validation. Gemini stays on RANDOM."
```

---

## Task 6: Add the Anthropic provider config

**Files:**
- Create: `sdks/python/motosan_ai/oauth/providers/anthropic.py`
- Modify: `sdks/python/motosan_ai/oauth/providers/__init__.py`
- Modify: `sdks/python/motosan_ai/oauth/__init__.py`
- Create: `sdks/python/tests/test_oauth_anthropic.py`

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_oauth_anthropic.py`:

```python
from __future__ import annotations

import httpx
import pytest
import respx

from motosan_ai.oauth import claude_pro_max_config
from motosan_ai.oauth._flow import StateStrategy, TokenBodyFormat, refresh_token


def test_claude_pro_max_client_id():
    assert claude_pro_max_config().client_id == "9d1c250a-e61b-44d9-88ed-5944d1962f5e"


def test_claude_pro_max_no_client_secret():
    assert claude_pro_max_config().client_secret is None


def test_claude_pro_max_redirect_port():
    assert claude_pro_max_config().redirect_port == 53692


def test_claude_pro_max_callback_path():
    assert claude_pro_max_config().callback_path == "/callback"


def test_claude_pro_max_redirect_uri_host_is_localhost():
    assert claude_pro_max_config().redirect_uri_host == "localhost"


def test_claude_pro_max_token_body_is_json():
    assert claude_pro_max_config().token_body is TokenBodyFormat.JSON


def test_claude_pro_max_state_strategy_is_equals_verifier():
    assert claude_pro_max_config().state_strategy is StateStrategy.EQUALS_VERIFIER


def test_claude_pro_max_extra_auth_params_has_code_true():
    assert ("code", "true") in tuple(claude_pro_max_config().extra_auth_params)


def test_claude_pro_max_scopes_include_claude_code_session():
    assert "user:sessions:claude_code" in tuple(claude_pro_max_config().scopes)


def test_claude_pro_max_auth_url_is_claude_ai():
    assert "claude.ai" in claude_pro_max_config().auth_url


@respx.mock
@pytest.mark.asyncio
async def test_refresh_posts_json_body():
    cfg = claude_pro_max_config()
    route = respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200, json={"access_token": "NEW_AT", "refresh_token": "NEW_RT", "expires_in": 3600}
        )
    )
    token = await refresh_token(cfg, refresh_token_value="OLD_RT")
    assert token.access_token == "NEW_AT"
    assert token.refresh_token == "NEW_RT"
    req = route.calls[0].request
    assert req.headers["content-type"].startswith("application/json")
    import json as _json

    payload = _json.loads(req.content)
    assert payload["grant_type"] == "refresh_token"
    assert payload["refresh_token"] == "OLD_RT"
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd sdks/python && uv run pytest tests/test_oauth_anthropic.py -q
```

Expected: FAIL — cannot import `claude_pro_max_config` from `motosan_ai.oauth`.

- [ ] **Step 3: Create `providers/anthropic.py`**

Create `sdks/python/motosan_ai/oauth/providers/anthropic.py`:

```python
"""Anthropic Claude Pro/Max OAuth provider config.

The ``client_id`` below is extracted from Anthropic's Claude Code CLI
authentication flow; the same value is used by the Rust ``anthropic-oauth``
crate and the reference implementation ``@earendil-works/pi-ai``. Anthropic
has not published this client_id as a public app registration for
third-party use — see the ToS disclosure in the Python SDK README.
"""

from __future__ import annotations

from motosan_ai.oauth._flow import OAuthConfig, StateStrategy, TokenBodyFormat


def claude_pro_max_config() -> OAuthConfig:
    return OAuthConfig(
        client_id="9d1c250a-e61b-44d9-88ed-5944d1962f5e",
        client_secret=None,
        auth_url="https://claude.ai/oauth/authorize",
        token_url="https://platform.claude.com/v1/oauth/token",
        scopes=(
            "org:create_api_key",
            "user:profile",
            "user:inference",
            "user:sessions:claude_code",
            "user:mcp_servers",
            "user:file_upload",
        ),
        redirect_port=53692,
        callback_path="/callback",
        redirect_uri_host="localhost",
        token_body=TokenBodyFormat.JSON,
        extra_auth_params=(("code", "true"),),
        state_strategy=StateStrategy.EQUALS_VERIFIER,
    )
```

- [ ] **Step 4: Export it from `providers/__init__.py`**

Replace `sdks/python/motosan_ai/oauth/providers/__init__.py`:

```python
from motosan_ai.oauth.providers.anthropic import claude_pro_max_config
from motosan_ai.oauth.providers.gemini import gemini_config

__all__ = ["claude_pro_max_config", "gemini_config"]
```

- [ ] **Step 5: Export it from `oauth/__init__.py`**

In `sdks/python/motosan_ai/oauth/__init__.py`, add the import and `__all__` entry:

```python
from motosan_ai.oauth._flow import (
    DEFAULT_CACHE_PATH,
    OAuthConfig,
    Token,
    ensure_fresh_token,
    exchange_code,
    load_cached_token,
    login,
    refresh_token,
    save_token,
)
from motosan_ai.oauth.providers.anthropic import claude_pro_max_config
from motosan_ai.oauth.providers.gemini import gemini_config

__all__ = [
    "DEFAULT_CACHE_PATH",
    "OAuthConfig",
    "Token",
    "claude_pro_max_config",
    "ensure_fresh_token",
    "exchange_code",
    "gemini_config",
    "load_cached_token",
    "login",
    "refresh_token",
    "save_token",
]
```

- [ ] **Step 6: Run tests**

```bash
cd sdks/python && uv run pytest tests/test_oauth_anthropic.py -q
```

Expected: all 11 tests pass.

- [ ] **Step 7: Lint + format**

```bash
cd sdks/python && ruff check motosan_ai/ && ruff format motosan_ai/ tests/
```

- [ ] **Step 8: Commit**

```bash
git add sdks/python/motosan_ai/oauth/ sdks/python/tests/test_oauth_anthropic.py
git commit -m "feat(python-oauth): add Anthropic Claude Pro/Max provider config

claude_pro_max_config() expresses Anthropic's OAuth quirks
declaratively via the five OAuthConfig knobs added in Tasks 2-5.
Exported from motosan_ai.oauth."
```

---

## Task 7: Live test, docs, version bump, PR

**Files:**
- Create: `sdks/python/tests/integration/test_anthropic_oauth_live.py`
- Modify: `sdks/python/README.md`
- Modify: `sdks/python/CHANGELOG.md`
- Modify: `sdks/python/pyproject.toml`
- Modify: `llms.txt`

- [ ] **Step 1: Create the skipped-by-default live login test**

Create `sdks/python/tests/integration/test_anthropic_oauth_live.py`:

```python
"""Live login smoke test for Anthropic OAuth.

Skipped by default — running it opens a real browser, requires a
Claude Pro/Max account, and binds to port 53692 on localhost. Run it
manually before releasing a new Python SDK version:

    cd sdks/python
    MOTOSAN_RUN_ANTHROPIC_OAUTH_LIVE=1 uv run pytest \\
        tests/integration/test_anthropic_oauth_live.py -v -s

Success criterion: the returned token's access_token starts with the
"sk-ant-oat01-" prefix and the refresh token is non-empty.
"""

from __future__ import annotations

import os

import pytest

from motosan_ai.oauth import claude_pro_max_config, login

_LIVE = os.environ.get("MOTOSAN_RUN_ANTHROPIC_OAUTH_LIVE") == "1"


@pytest.mark.skipif(not _LIVE, reason="set MOTOSAN_RUN_ANTHROPIC_OAUTH_LIVE=1 to run")
@pytest.mark.asyncio
async def test_live_login_returns_setup_token():
    token = await login(claude_pro_max_config())
    assert token.access_token.startswith("sk-ant-oat01-"), (
        f"unexpected token prefix: {token.access_token[:20]}"
    )
    assert token.refresh_token, "refresh_token must be non-empty"
    assert token.expires_in > 0, f"expires_in must be positive: {token.expires_in}"
    print(f"\nLive login OK. expires_in={token.expires_in}s")
```

- [ ] **Step 2: Verify the live test is collected but skipped**

```bash
cd sdks/python && uv run pytest tests/integration/test_anthropic_oauth_live.py -q
```

Expected: `1 skipped`. The file imports cleanly and the test is skipped because `MOTOSAN_RUN_ANTHROPIC_OAUTH_LIVE` is unset.

- [ ] **Step 3: Update the README OAuth section**

In `sdks/python/README.md`, find the OAuth helpers block (it currently imports `google_gemini_config`). Replace the code example and add an Anthropic subsection. The existing example becomes:

````markdown
OAuth helpers are available under `motosan_ai.oauth`:

```python
import asyncio
from motosan_ai.oauth import gemini_config, login, save_token

async def main():
    token = await login(gemini_config())
    save_token(token)

asyncio.run(main())
```

### Anthropic OAuth (Claude Pro/Max)

`claude_pro_max_config()` drives a PKCE login against `claude.ai` and returns
an `sk-ant-oat01-*` token usable directly with `AnthropicProvider`:

```python
import asyncio
from motosan_ai import AnthropicProvider
from motosan_ai.oauth import claude_pro_max_config, login

async def main():
    token = await login(claude_pro_max_config())
    provider = AnthropicProvider(api_key=token.access_token)
    # AnthropicProvider auto-detects the sk-ant-oat01- prefix.

asyncio.run(main())
```

**⚠️ ToS disclosure:** this uses the OAuth `client_id` registered by
Anthropic's Claude Code CLI. The resulting token authenticates **as a Claude
Code CLI session**. Anthropic has not published this `client_id` for
third-party use; usage for purposes other than running `claude` may be
subject to change, rate limited, or in violation of Anthropic's terms. You
are responsible for compliance. If you have an `sk-ant-api*` key, prefer it.
````

- [ ] **Step 4: Add a CHANGELOG entry**

In `sdks/python/CHANGELOG.md`, add a new section directly above the most recent existing version entry (`## [0.11.0] - 2026-04-27`). Match the file's existing header style — `## [VERSION] - DATE`. Use the next version (`0.12.0`, a minor bump from `0.11.0`) and today's date:

```markdown
## [0.12.0] - 2026-05-18

### Added
- Anthropic Claude Pro/Max OAuth: `motosan_ai.oauth.claude_pro_max_config()`
  plus `login()` / `refresh_token()` yield an `sk-ant-oat01-*` token usable
  directly with `AnthropicProvider`. See the README for the ToS disclosure.
- `OAuthConfig` gained `callback_path`, `redirect_uri_host`, `token_body`,
  `extra_auth_params`, and `state_strategy` fields, plus `TokenBodyFormat`
  and `StateStrategy` enums.

### Changed
- **Breaking:** `motosan_ai.oauth` no longer exports `google_gemini_config`;
  use `gemini_config` instead. The `oauth/` package was refactored from a
  single `google.py` module into a generic core plus per-provider config
  modules (`providers/gemini.py`, `providers/anthropic.py`).
- **Breaking:** `exchange_code()` gained a required `state` keyword argument
  (the `state` value is now echoed in the token-endpoint POST body, which
  Anthropic requires).
```

- [ ] **Step 5: Bump the version in `pyproject.toml`**

In `sdks/python/pyproject.toml`, change `version = "0.11.0"` to `version = "0.12.0"`.

- [ ] **Step 6: Update `llms.txt`**

`llms.txt` does **not** name any OAuth config function — verified: `grep -n "google_gemini_config" llms.txt` returns nothing, and there is no dedicated Python OAuth section. The only relevant mention is line ~197 in the `GeminiCodeAssist` entry: "...`motosan_ai.oauth` PKCE flow in Python...". So there is no rename to do.

The single edit: extend that sentence (or add a following sentence) in the `GeminiCodeAssist` entry's vicinity to note Anthropic OAuth support. Find the line containing `motosan_ai.oauth` PKCE flow in Python` and append, in the same area:

```
Python Anthropic Claude Pro/Max OAuth is available via `motosan_ai.oauth.claude_pro_max_config()` + `login()` (returns an `sk-ant-oat01-*` token).
```

Place it as a standalone sentence at the end of the `GeminiCodeAssist` paragraph, or as a new short line right after it — whichever reads cleanly in context. This is the only `llms.txt` change.

Verify the edit landed:

```bash
grep -n "claude_pro_max_config" llms.txt
```

Expected: at least one match (the line just added).

- [ ] **Step 7: Run the full Python gate**

```bash
cd sdks/python
ruff check motosan_ai/
ruff format --check motosan_ai/ tests/
uv run pytest tests/ -q --ignore=tests/integration/
```

Expected: all three pass.

- [ ] **Step 8: Commit**

```bash
git add sdks/python/tests/integration/test_anthropic_oauth_live.py \
        sdks/python/README.md sdks/python/CHANGELOG.md \
        sdks/python/pyproject.toml llms.txt
git commit -m "docs(python-oauth): live test, README/ToS, v0.12.0

- Skipped-by-default live login smoke test (mirrors the Rust
  #[ignore]'d login_live test).
- README: gemini_config rename in the example + an Anthropic OAuth
  example with the ToS disclosure.
- CHANGELOG: 0.12.0 entry (breaking: google_gemini_config rename,
  exchange_code state param).
- pyproject: 0.11.0 -> 0.12.0.
- llms.txt: note Anthropic OAuth support in the GeminiCodeAssist
  entry."
```

- [ ] **Step 9: Push and open the PR**

```bash
git push -u origin feat/python-anthropic-oauth
gh pr create --title "feat(python-oauth): Anthropic Claude Pro/Max OAuth (Rust parity)" --body "$(cat <<'EOF'
## Summary
- New `motosan_ai.oauth.claude_pro_max_config()` — PKCE login + refresh against `claude.ai`, yields an `sk-ant-oat01-*` token usable directly with `AnthropicProvider`. Python parity with the Rust `anthropic-oauth` crate.
- Refactored `motosan_ai/oauth/` from the `google.py` monolith into a generic core (`_flow.py`) plus per-provider config modules (`providers/{gemini,anthropic}.py`).
- `OAuthConfig` gained five knobs (`callback_path`, `redirect_uri_host`, `token_body`, `extra_auth_params`, `state_strategy`) and two enums (`TokenBodyFormat`, `StateStrategy`).

**Breaking:** `google_gemini_config` → `gemini_config`; `exchange_code()` gained a required `state` kwarg.

Spec: `docs/superpowers/specs/2026-05-18-python-anthropic-oauth-design.md`
Plan: `docs/superpowers/plans/2026-05-18-python-anthropic-oauth.md`

## Test plan
- [ ] CI green: `ruff check`, `ruff format --check`, `pytest tests/ --ignore=tests/integration/`
- [ ] Existing OAuth tests stay green (logic unchanged): `test_oauth_pkce.py`, `test_oauth_token.py`, `test_oauth_callback.py`, `test_oauth_gemini.py` (renamed from `test_oauth_google.py`)
- [ ] New tests pass: `test_oauth_flow.py`, `test_oauth_anthropic.py`
- [ ] Live test passed locally: `MOTOSAN_RUN_ANTHROPIC_OAUTH_LIVE=1 uv run pytest tests/integration/test_anthropic_oauth_live.py` returned an `sk-ant-oat01-` token (paste stderr in a PR comment)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 10: Report the PR URL**

Paste the PR URL in the chat.

---

## Done Criteria

- [ ] All 7 tasks complete and committed on `feat/python-anthropic-oauth`
- [ ] CI green on the PR (ruff check + ruff format check + pytest)
- [ ] `MOTOSAN_RUN_ANTHROPIC_OAUTH_LIVE=1 uv run pytest tests/integration/test_anthropic_oauth_live.py` was run locally and returned an `sk-ant-oat01-*` token
- [ ] PR URL reported

---

## Self-Review Notes

Plan covers every spec section:

- **Goals** — generic refactor (Task 1); five knobs (Tasks 2–5, one per task); Anthropic config + `state`-in-body (Tasks 5–6); storage helpers kept (moved verbatim in Task 1, `DEFAULT_CACHE_PATH` unchanged); single PR (Task 7).
- **Non-goals** — `AnthropicProvider` chat path untouched; no Rust changes; no new wire behavior beyond the Rust-validated flow; the API break is handled, not avoided.
- **Architecture: file layout** — Task 1 creates `_flow.py`, `providers/{__init__,gemini}.py`, rewrites `oauth/__init__.py`, deletes `google.py`.
- **Architecture: API break / consumers** — Task 1 updates `test_oauth_token.py`, renames `test_oauth_google.py`, updates `test_code_assist_live.py`; Task 7 updates README and `llms.txt`. `test_oauth_callback.py` is updated in Task 2 (its `bind()` calls).
- **The five knobs** — `callback_path` (Task 2), `redirect_uri_host` + `extra_auth_params` (Task 3), `token_body` (Task 4), `state_strategy` (Task 5). Each task adds the field with a default and fills it explicitly in `gemini_config()`.
- **Provider configs** — `gemini_config()` built up across Tasks 1–5; `claude_pro_max_config()` in Task 6.
- **Generic-core changes** — `_build_auth_url` (Task 3), `login` redirect URI + state (Tasks 2, 3, 5), `_post_token` (Task 4), `exchange_code` `state` param (Task 5), `_callback_server.py` (Task 2).
- **Error handling** — no new exception types; `_post_token`'s JSON branch uses httpx `json=` (Task 4). No dedicated task — covered by the existing `AuthError`/`NetworkError` paths, which `test_oauth_gemini.py`'s `test_exchange_code_400_raises` continues to exercise.
- **Testing** — `test_oauth_flow.py` (Tasks 3–5), `test_oauth_anthropic.py` (Task 6), `test_oauth_callback.py` extension (Task 2), `test_oauth_gemini.py` rename (Task 1), live test (Task 7). Regression gates run via the full `pytest tests/` invocation at the end of Tasks 1, 5, and 7.
- **Documentation** — README, CHANGELOG, `llms.txt`, version bump — all Task 7.
- **Versioning** — minor bump `0.11.0` → `0.12.0` (Task 7 Step 5).
