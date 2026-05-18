# Python Anthropic OAuth (Claude Pro/Max) — Design Spec

**Date**: 2026-05-18
**Status**: Approved (design); ready for implementation plan
**Scope**: Python SDK only — parity with the Rust `anthropic-oauth` crate
shipped in PR #180

## Background

The Rust SDK now has `anthropic-oauth` (published to crates.io as v0.1.0): a
PKCE OAuth login + refresh flow that yields a `sk-ant-oat01-*` token for
Anthropic's Claude Pro/Max accounts. The Python SDK's `AnthropicProvider`
already accepts `sk-ant-oat01-*` tokens (it auto-detects the prefix and
applies the Claude Code identity headers), but Python has no way to *obtain*
such a token.

This spec describes adding that capability to the Python SDK, mirroring the
Rust design.

### Current Python OAuth structure

`sdks/python/motosan_ai/oauth/` today:

- `_pkce.py` — private, `Pkce.generate()`
- `_callback_server.py` — private, `bind()` / `wait_for_callback()`. The
  callback HTTP path `/auth/callback` is hardcoded.
- `google.py` — the entire OAuth implementation: `OAuthConfig`, `Token`,
  `login`, `refresh_token`, `exchange_code`, `ensure_fresh_token`,
  `save_token`, `load_cached_token`, `google_gemini_config`.
- `__init__.py` — re-exports everything from `google.py`.

Unlike the Rust side (a generic `motosan-ai-oauth` crate plus per-provider
config files), Python has no generic separation: `google.py` *is* the
implementation, despite `OAuthConfig` being provider-agnostic. The Anthropic
flow diverges from `google.py`'s assumptions in the same five places the Rust
side had to generalize.

## Goals / Non-Goals

### Goals

- Refactor `motosan_ai/oauth/` into a generic core plus per-provider config
  modules, mirroring the Rust `motosan-ai-oauth` layout.
- Add five `OAuthConfig` knobs (`callback_path`, `redirect_uri_host`,
  `token_body`, `extra_auth_params`, `state_strategy`) so the Anthropic flow
  is expressed declaratively.
- Add an Anthropic provider config and the `state`-in-token-POST-body
  behavior (the second empirical fix from the Rust live validation).
- Keep the storage helpers (`save_token` / `load_cached_token` /
  `ensure_fresh_token`) and make them generic so Anthropic can use them.
- Single-PR scope.

### Non-Goals

- Touching the Python `AnthropicProvider` chat code path (it already
  supports `sk-ant-oat01-*` tokens).
- Any Rust SDK changes.
- New wire-level behavior — this is a faithful port of the Rust flow, which
  was already validated against the live Anthropic endpoints.
- Backwards compatibility for the old `oauth/` public names: the user
  approved an API break. The only in-repo consumers (one integration test
  and the README) are updated in the same PR.

## Architecture

### File layout

After the refactor, `sdks/python/motosan_ai/oauth/`:

```
_pkce.py                  private, unchanged
_callback_server.py       private, callback_path parametrized
_flow.py                  NEW private — generic core
providers/__init__.py     NEW
providers/gemini.py       NEW — gemini_config()
providers/anthropic.py    NEW — claude_pro_max_config()
__init__.py               public API re-export
google.py                 DELETED
```

Mapping to Rust: `_flow.py` corresponds to `motosan-ai-oauth`'s `lib.rs` +
`exchange.rs` (Python's 228-line implementation does not warrant a 3-file
split). `providers/` corresponds to Rust's `providers/`. Python has no Codex
OAuth, so `providers/` holds only `gemini` and `anthropic`.

`_flow.py` contains: `OAuthConfig`, `Token`, `StateStrategy`,
`TokenBodyFormat`, `login`, `refresh_token`, `exchange_code`,
`build_auth_url` (`_build_auth_url`), `post_token` (`_post_token`),
`save_token`, `load_cached_token`, `ensure_fresh_token`, `DEFAULT_CACHE_PATH`.

### API break — renamed/moved public names

- `google_gemini_config()` → `providers.gemini.gemini_config()` (mirrors
  Rust `providers::gemini::gemini()`).
- New `providers.anthropic.claude_pro_max_config()` (mirrors Rust
  `claude_pro_max()`).
- `OAuthConfig`, `Token`, `login`, `refresh_token`, `exchange_code`, the
  storage helpers, and `DEFAULT_CACHE_PATH` keep their names — they just move
  to `_flow.py`.

`oauth/__init__.py` re-exports the public surface: the `_flow.py` public
names plus `gemini_config` and `claude_pro_max_config`. Intended usage:

```python
from motosan_ai.oauth import login, claude_pro_max_config

token = await login(claude_pro_max_config())
```

In-repo consumers updated in the same PR:
- `tests/integration/test_code_assist_live.py` — imports
  `google_gemini_config`; switch to `gemini_config`.
- `sdks/python/README.md` — the OAuth example uses `google_gemini_config`.

### The five knobs

```python
class TokenBodyFormat(enum.Enum):
    FORM = "form"
    JSON = "json"


class StateStrategy(enum.Enum):
    RANDOM = "random"             # random 16-byte base64url; standard OAuth
    EQUALS_VERIFIER = "verifier"  # state == PKCE verifier; required by Anthropic


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
    token_body: TokenBodyFormat = TokenBodyFormat.FORM
    extra_auth_params: Sequence[tuple[str, str]] = ()
    state_strategy: StateStrategy = StateStrategy.RANDOM
```

The new fields all have defaults equal to current behavior, so existing
direct construction does not break. Both provider configs nonetheless fill
every field explicitly (explicit over magical, matching Rust).

| Field | Why Anthropic needs it |
| --- | --- |
| `callback_path` | Anthropic registers `redirect_uri` at `/callback`; `_callback_server.py` hardcodes `/auth/callback`. |
| `redirect_uri_host` | Anthropic's registered redirect URI uses hostname `localhost`; `login` currently builds the URI with `127.0.0.1`. The HTTP server still binds to `127.0.0.1`; only the URI string changes. |
| `token_body` | Anthropic's token endpoint is driven with `application/json` (matches the Rust impl, validated live). Google uses form encoding. |
| `extra_auth_params` | Replaces the hardcoded `access_type=offline` query param (a Google-ism) with a config-driven list. Anthropic adds `code=true`; Google keeps `access_type=offline`. |
| `state_strategy` | Anthropic's `claude.ai/oauth/authorize` rejects random `state` values with "Invalid request format"; it requires `state == verifier` (matching Claude Code CLI). Google uses `RANDOM`. |

### Provider configs

`providers/gemini.py` reproduces the current `google_gemini_config()`
behavior exactly:

```python
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
        redirect_port=None,
        callback_path="/auth/callback",
        redirect_uri_host="127.0.0.1",
        token_body=TokenBodyFormat.FORM,
        extra_auth_params=(("access_type", "offline"),),
        state_strategy=StateStrategy.RANDOM,
    )
```

`providers/anthropic.py`:

```python
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

The `client_id` is extracted from Anthropic's Claude Code CLI flow; the same
value is used by the Rust `anthropic-oauth` crate and `@earendil-works/pi-ai`.
Anthropic has not published it as a public app registration for third-party
use — the ToS disclosure already in the top-level README applies.

### Generic-core changes in `_flow.py`

- `_build_auth_url` — replace the hardcoded `"access_type": "offline"` query
  param with iteration over `config.extra_auth_params`.
- `login` — build `redirect_uri` from `config.redirect_uri_host` instead of
  hardcoded `127.0.0.1`; pass `config.callback_path` to the callback server;
  derive `state` from `config.state_strategy` (`RANDOM` generates a random
  16-byte base64url value, `EQUALS_VERIFIER` reuses `pkce.verifier`).
- `_post_token` — switch on `config.token_body`: `FORM` sends `data=` (httpx
  sets `application/x-www-form-urlencoded`), `JSON` sends `json=` (httpx sets
  `application/json`). The hardcoded `content-type` header is removed and
  left to httpx.
- `exchange_code` — include `state` in the token POST body (after `code`),
  matching the Rust `anthropic-oauth` fix. RFC 6749 §4.1.3 does not require
  it; Anthropic empirically does. Google tolerates the extra field. This
  means `exchange_code`'s signature gains a `state` parameter.

### `_callback_server.py` change

`bind()` gains a `callback_path: str` parameter. The `_Handler.do_GET`
comparison uses it instead of the hardcoded `/auth/callback`. The bind
address stays `127.0.0.1`.

### End-to-end usage

```python
from motosan_ai import AnthropicProvider
from motosan_ai.oauth import login, claude_pro_max_config

token = await login(claude_pro_max_config())
provider = AnthropicProvider(api_key=token.access_token)
# AnthropicProvider detects the "sk-ant-oat01-" prefix and applies
# Bearer auth + Claude Code identity headers automatically.
```

## Error Handling

No new exception classes. All failure modes map to the existing
`motosan_ai.error` types:

| Situation | Exception |
| --- | --- |
| Port 53692 already in use | `OSError` from `HTTPServer`, wrapped as `AuthError` |
| Browser flow not completed within 120 s | `AuthError("OAuth login timed out after 120s")` |
| Anthropic token endpoint returns 4xx/429 | `AuthError("OAuth token exchange failed (NNN): ...")` |
| `state != expected` | `AuthError("OAuth state mismatch...")` |
| Network failure | `NetworkError` |

The only adjustment: `_post_token`'s JSON branch uses httpx's `json=` so
httpx sets `application/json` itself; the previously hardcoded
`content-type: application/x-www-form-urlencoded` header is removed. The
error-body parsing (`err.get("error_description")`) works for both form and
JSON responses unchanged.

## Testing

Mirrors the existing `test_oauth_google.py` style — `respx` to intercept
HTTP (the Python equivalent of Rust's mockito).

| What it asserts | File |
| --- | --- |
| `_build_auth_url` honors `extra_auth_params`; under `EQUALS_VERIFIER`, `state` equals the PKCE verifier | `test_oauth_flow.py` (new) |
| `_post_token` produces a form body for `FORM` and a JSON body for `JSON`; the JSON branch includes `state` in the body | `test_oauth_flow.py` |
| Callback server matches the configured `callback_path` (`/callback` true, `/auth/callback` false, and vice versa) | `test_oauth_callback.py` (extend existing) |
| `claude_pro_max_config()` field values match expectations | `test_oauth_anthropic.py` (new) |
| `gemini_config()` behavior equals the old `google_gemini_config()` | `test_oauth_gemini.py` (renamed from `test_oauth_google.py`) |
| Anthropic `refresh_token` POSTs a JSON body to the mocked endpoint | `test_oauth_anthropic.py` |
| Live login smoke test (CI-excluded) | `tests/integration/test_anthropic_oauth_live.py` (new), marked to skip by default |

**Regression gates that must stay green**:

- The existing `test_oauth_pkce.py`, `test_oauth_token.py`, and
  `test_oauth_callback.py` test logic (callback tests gain new path-param
  cases but existing assertions stay valid).
- `test_oauth_google.py` is renamed to `test_oauth_gemini.py`; its imports
  switch from `google_gemini_config` to `gemini_config` and from
  `motosan_ai.oauth.google` to `motosan_ai.oauth`, but the test logic and
  assertions are unchanged.
- `tests/integration/test_code_assist_live.py` collects cleanly after its
  import is updated.

The live test is skipped by default (a pytest marker or `@pytest.mark.skip`)
and documented as a manual pre-release check, mirroring the Rust
`#[ignore]`'d `login_live` test.

`check-python` (ruff → format check → pytest) is the per-task verification
gate.

## Documentation

- `sdks/python/README.md`: update the OAuth section — switch the example to
  `gemini_config` and add an Anthropic OAuth example plus the ToS note.
- `sdks/python/CHANGELOG.md`: new version entry describing the `oauth`
  refactor (breaking: renamed public names) and the new Anthropic support.
- `llms.txt`: the Python OAuth reference mentions `motosan_ai.oauth`; update
  it to note Anthropic support and the `gemini_config` rename.
- `AGENTS.md` / `skills/motosan-ai/SKILL.md`: update only if they reference
  the old `google_gemini_config` name.

## Versioning

Per the release checklist, the Python SDK version
(`sdks/python/pyproject.toml`) gets a **minor bump** — new public API
(`claude_pro_max_config`, `StateStrategy`, `TokenBodyFormat`) plus a breaking
rename (`google_gemini_config` → `gemini_config`). The exact version number
is set at release time following the process in `llms.txt` § Release Steps
(Python).

## Open Questions

None. All design questions were resolved during brainstorming:

- Structure: refactor into generic core + per-provider modules.
- Storage helpers: kept and made generic.
- API compatibility: break allowed; in-repo consumers updated in the same PR.
