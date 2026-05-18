# Anthropic OAuth Login (Claude Pro/Max) — Design Spec

**Date**: 2026-05-18
**Status**: Approved (design); ready for implementation plan
**Scope**: Rust SDK only (Python parity deferred to a separate PR)

## Background

`AnthropicProvider` in both Rust (`sdks/rust/src/providers/anthropic.rs`) and
Python (`sdks/python/motosan_ai/providers/anthropic.py`) already accepts
`sk-ant-oat01-*` setup tokens and applies the required "stealth" headers
(`Authorization: Bearer`, `user-agent: claude-code/...`, `x-app: cli`,
`anthropic-beta: claude-code-20250219,oauth-2025-04-20`) plus the mandatory
`"You are Claude Code, Anthropic's official CLI for Claude."` system prompt
prefix. What is missing is a way to **obtain** such a token: there is no PKCE
login flow against `claude.ai`, no callback server, no token refresh.

Reference implementation: `@earendil-works/pi-ai` performs this in
`packages/ai/src/utils/oauth/anthropic.ts`.

The repo already has the building blocks needed to add this in Rust:

- `crates/motosan-ai-oauth` — generic PKCE / callback server / token exchange
- `crates/codex-oauth` — thin provider wrapper (`login()` / `refresh()`)
- `motosan-ai-oauth/src/providers/{codex,gemini}.rs` — feature-gated configs

This spec describes adding an **`anthropic-oauth` crate** in the same shape,
plus the minimal generalization of `motosan-ai-oauth` needed because the
Anthropic OAuth flow diverges from the current shared crate's assumptions in
four specific places (callback path, redirect URI host, token request body
format, and auth-URL extra parameters).

## Goals / Non-Goals

### Goals

- Provide `anthropic_oauth::login()` / `anthropic_oauth::refresh()` in Rust
  that yields a `Token { access_token, refresh_token, expires_in, ... }` usable
  directly by the existing `AnthropicProvider` setup-token code path.
- Generalize `motosan-ai-oauth::OAuthConfig` so the Anthropic provider can be
  expressed declaratively (no Anthropic-specific branches in the shared crate).
- Keep all existing `codex-oauth` and Gemini OAuth behavior bit-for-bit
  identical.
- Single-PR scope.

### Non-Goals

- Python parity. A follow-up PR will add `sdks/python/motosan_ai/oauth/anthropic.py`
  modeled on the existing `oauth/google.py`.
- Token storage / caching / `ensure_fresh_token` helpers. The new crate mirrors
  `codex-oauth`, which leaves persistence to the caller.
- Touching the existing `AnthropicProvider` chat code path (it already supports
  `sk-ant-oat01-*` tokens).
- Runtime ToS warnings or env-var opt-in gates. The disclosure lives in README.
- A `motosan-ai oauth login` CLI subcommand or any new binary.

## Architecture

### Change 1 — Extend `motosan-ai-oauth::OAuthConfig`

Add four fields to `OAuthConfig` in `crates/motosan-ai-oauth/src/lib.rs`. All
existing fields are untouched. Existing `codex` and `gemini` provider configs
fill the new fields explicitly to preserve current behavior; no `Default` impl
or hidden defaults — explicit over magical.

```rust
pub struct OAuthConfig {
    // === existing fields (unchanged) ===
    pub client_id: &'static str,
    pub client_secret: Option<&'static str>,
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub scopes: &'static [&'static str],
    pub redirect_port: Option<u16>,

    // === new fields ===
    pub callback_path: &'static str,
    pub redirect_uri_host: &'static str,
    pub token_body: TokenBodyFormat,
    pub extra_auth_params: &'static [(&'static str, &'static str)],
}

pub enum TokenBodyFormat { Form, Json }
```

| Field | Why Anthropic needs to override |
| --- | --- |
| `callback_path` | Anthropic registers `redirect_uri = http://localhost:53692/callback`. The callback server's request matcher must use this path; today `/auth/callback` is hardcoded. |
| `redirect_uri_host` | OAuth servers compare `redirect_uri` as a literal string. Anthropic's registered URI uses `localhost`; the shared crate currently builds the URI with `127.0.0.1` (`lib.rs::login`). The TCP listener still binds to `127.0.0.1` (the safer default that works without DNS); only the URI string sent to the auth server changes. pi-ai uses the same split. |
| `token_body` | The shared `post_token` uses `.form(&params)`. pi-ai's reference Anthropic implementation sends `application/json`; we match that known-working path. Anthropic's endpoint may also accept form bodies, but we have not verified that. |
| `extra_auth_params` | The current `build_auth_url` hardcodes `access_type=offline` (a Google-ism). Replacing it with a config-driven list lets Anthropic add its required `code=true` and Google keep its `access_type=offline`. |

**Note on `state` parameter** — pi-ai sends `state = verifier` (PKCE verifier reused as the OAuth state nonce) when talking to Anthropic. This is a coincidence of pi-ai's implementation, not an Anthropic server requirement: standard OAuth servers echo `state` back unchanged regardless of its value. The shared crate's existing `Random` state generation is fine for Anthropic; we do not need a `state_strategy` knob.

### Change 2 — Generalize the call sites

- `lib.rs::build_auth_url` — replace the hardcoded
  `.append_pair("access_type", "offline")` with iteration over
  `config.extra_auth_params`.
- `lib.rs::login` — build `redirect_uri` from `config.redirect_uri_host`
  instead of hardcoded `127.0.0.1`, and pass `config.callback_path` through to
  the callback server.
- `server.rs::is_callback_request` — accept the configured path; the function
  becomes `is_callback_request(request: &str, callback_path: &str) -> bool`.
  The TCP listener bind address stays `127.0.0.1`.
- `exchange.rs::post_token` — accept `body_format: TokenBodyFormat` and switch
  between `.form(&params)` and `.json(&map)` (where the JSON branch builds a
  `HashMap<&str, &str>` or `serde_json::json!` value from the same param
  tuples).

### Change 3 — Anthropic provider config

New file `crates/motosan-ai-oauth/src/providers/anthropic.rs`, gated behind a
new `anthropic` feature on `motosan-ai-oauth`:

```rust
use crate::{OAuthConfig, TokenBodyFormat};

pub fn claude_pro_max() -> OAuthConfig {
    OAuthConfig {
        client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
        client_secret: None,
        auth_url: "https://claude.ai/oauth/authorize",
        token_url: "https://platform.claude.com/v1/oauth/token",
        scopes: &[
            "org:create_api_key",
            "user:profile",
            "user:inference",
            "user:sessions:claude_code",
            "user:mcp_servers",
            "user:file_upload",
        ],
        redirect_port: Some(53692),
        callback_path: "/callback",
        redirect_uri_host: "localhost",
        token_body: TokenBodyFormat::Json,
        extra_auth_params: &[("code", "true")],
    }
}
```

**Source of `client_id`**: this value is extracted from the Claude Code CLI's
own OAuth flow; the same value is used by `@earendil-works/pi-ai`. Anthropic
has not published this client_id as a public app registration for third-party
use — see the ToS disclosure in README. Cited in code comment.

### Change 4 — `crates/anthropic-oauth/` (new)

Mirrors `crates/codex-oauth` exactly:

```rust
pub use motosan_ai_oauth::{Error, Token};

pub async fn login() -> Result<Token, Error> {
    motosan_ai_oauth::login(
        &motosan_ai_oauth::providers::anthropic::claude_pro_max(),
    ).await
}

pub async fn refresh(refresh_token: &str) -> Result<Token, Error> {
    motosan_ai_oauth::refresh(
        &motosan_ai_oauth::providers::anthropic::claude_pro_max(),
        refresh_token,
    ).await
}
```

`Cargo.toml` depends on `motosan-ai-oauth = { ..., features = ["anthropic"] }`,
matching how `codex-oauth` enables the `codex` feature.

### End-to-end usage

```rust
let token = anthropic_oauth::login().await?;

let provider = AnthropicProvider::new(&token.access_token, None, None);
// `AnthropicProvider::is_setup_token` sees the "sk-ant-oat01-" prefix and
// automatically applies Bearer auth, claude-code identity headers, and the
// Claude Code system prompt prefix.

let response = provider.chat(request).await?;
```

## Error Handling

No new error variants. All failure modes map to existing
`motosan_ai_oauth::Error` cases:

| Situation | Variant |
| --- | --- |
| Port 53692 already in use | `Error::Callback("port ... already in use")` (existing message) |
| User does not complete browser flow within 120 s | `Error::Callback("timed out waiting...")` |
| Anthropic token endpoint returns 4xx (bad client_id, scope rejected, etc.) | `Error::TokenExchange("HTTP {status}: {body}")` |
| `state != expected` (defense in depth, even with EqualsVerifier) | `Error::StateMismatch` |
| Anthropic omits `refresh_token` on refresh response | Handled by existing `RawTokenResponse::into_token(fallback)` |

JSON error bodies vs form error bodies: the existing
`format!("HTTP {status}: {body}")` formatting is body-agnostic, so no
adjustment is needed.

## Testing

| Layer | What it asserts | Tool |
| --- | --- | --- |
| Unit (`lib.rs`) | `build_auth_url` honors `extra_auth_params` (each tuple ends up as a query pair) and `redirect_uri_host` (URI string uses `localhost` vs `127.0.0.1` per config, while bind address stays `127.0.0.1`) | pure function tests |
| Unit (`exchange.rs`) | `TokenBodyFormat::Json` produces a JSON body, `Form` produces a form body | mockito `match_body` |
| Unit (`server.rs`) | `is_callback_request("/callback?code=...", "/callback")` is true; with default path `/auth/callback` it is false | pure function tests |
| Unit (`providers/anthropic.rs`) | `client_id`, scopes, `redirect_port`, `callback_path`, `redirect_uri_host`, `token_body`, `extra_auth_params` match expected values | mirrors `providers/codex.rs` tests |
| Integration (`anthropic-oauth`) | `refresh()` hits the mocked endpoint with a JSON body and decodes the response into a `Token` correctly | mockito |
| Live (CI-excluded) | `#[ignore]`'d smoke test of `login()` that requires a real browser interaction | `cargo test --ignored login_live` |

**Regression gates that must stay green without modifying assertions**:

- All existing `crates/motosan-ai-oauth` unit tests
- All existing `crates/codex-oauth` tests
- All existing Gemini OAuth tests
- `sdks/rust/tests/anthropic_oauth_usage.rs` (validates that an
  `sk-ant-oat01-*` token continues to drive the correct chat-side stealth
  headers)

The only legitimate diff in existing tests is adding the four new fields when
constructing `OAuthConfig` literals (e.g. inside `codex.rs` /  `gemini.rs`
provider tests and the `dummy_config()` fixture in `lib.rs` tests).

## Documentation

- Top-level `README.md`: add a short "Anthropic OAuth (Claude Pro/Max)"
  section showing the `login()` → `AnthropicProvider::new` flow.
- Same section carries the ToS disclosure: this feature uses the public Claude
  Code OAuth client and the resulting token authenticates as a Claude Code
  CLI session. Anthropic may change rules or rate-limit such usage at any
  time; users are responsible for compliance.
- `AGENTS.md`: add `crates/anthropic-oauth` to the crate inventory.
- `llms.txt`: add the new crate under the OAuth section and bump version
  references per the release checklist when the PR ships.
- `CHANGELOG.md` (Rust): new minor version entry summarizing the addition.

## Versioning

Per the release checklist in `CLAUDE.md` / `llms.txt`:

- `crates/motosan-ai-oauth`: **minor bump** — `OAuthConfig` grew one new
  public enum (`TokenBodyFormat`) and four required fields (`callback_path`,
  `redirect_uri_host`, `token_body`, `extra_auth_params`). Out-of-tree
  consumers constructing `OAuthConfig` literals will need to add the new
  fields. Document this in `CHANGELOG.md` under "Breaking changes if you
  construct OAuthConfig directly".
- `crates/codex-oauth`: **patch bump** — source touched (its `OAuthConfig`
  literal gains four new fields, with values that preserve current behavior)
  but the crate's public API is unchanged.
- `crates/anthropic-oauth`: **new crate**, initial version matches the
  current `codex-oauth` version at the time of release for consistency.

## Open Questions

None. All design questions were resolved during brainstorming:

- Architecture path: extend shared crate (vs duplicate or refactor-first).
- ToS disclosure: README-only.
- Token storage: out of scope.
- Python parity: deferred.
