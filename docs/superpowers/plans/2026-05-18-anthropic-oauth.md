# Anthropic OAuth Login (Claude Pro/Max) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `crates/anthropic-oauth` crate exposing `login()` and `refresh()` for Anthropic's Claude Pro/Max OAuth flow, so users can obtain a `sk-ant-oat01-*` token that the existing `AnthropicProvider` already knows how to use.

**Architecture:** Generalize the shared `motosan-ai-oauth` crate with four new `OAuthConfig` fields (`callback_path`, `redirect_uri_host`, `token_body`, `extra_auth_params`) and one new public enum (`TokenBodyFormat`). Express the Anthropic provider declaratively via a new feature-gated provider config. The new `anthropic-oauth` wrapper mirrors `codex-oauth` exactly.

**Tech Stack:** Rust 2021, `reqwest` 0.12 (rustls), `tokio` 1, `mockito` 1 (dev-dep, already available in `sdks/rust`). No new runtime dependencies.

**Spec:** `docs/superpowers/specs/2026-05-18-anthropic-oauth-design.md`

**Engineer pre-flight:** Read the spec end-to-end before starting Task 1. The "Why this knob exists" rationale in the spec is not repeated in each task.

---

## File Map

**Modify:**
- `Cargo.toml` (workspace root) — add `sdks/rust/crates/anthropic-oauth` to `members`
- `sdks/rust/crates/motosan-ai-oauth/Cargo.toml` — minor bump (0.1.0 → 0.2.0), add `anthropic` feature, add `mockito` dev-dep
- `sdks/rust/crates/motosan-ai-oauth/src/lib.rs` — `OAuthConfig` struct + `TokenBodyFormat` enum + `build_auth_url` + `login`
- `sdks/rust/crates/motosan-ai-oauth/src/server.rs` — parametrize callback path
- `sdks/rust/crates/motosan-ai-oauth/src/exchange.rs` — parametrize token body format
- `sdks/rust/crates/motosan-ai-oauth/src/providers/mod.rs` — register `anthropic` module
- `sdks/rust/crates/motosan-ai-oauth/src/providers/codex.rs` — fill 4 new fields, preserve current behavior
- `sdks/rust/crates/motosan-ai-oauth/src/providers/gemini.rs` — fill 4 new fields, preserve current behavior
- `sdks/rust/crates/codex-oauth/Cargo.toml` — patch bump (0.1.0 → 0.1.1)
- `llms.txt` — add `## anthropic-oauth` section, release-tag table row, Release Steps block, CI Pipeline bullet; bump in-line `motosan-ai-oauth = "0.1"` references to `"0.2"`
- `README.md` — Anthropic OAuth section with ToS disclosure
- `AGENTS.md` — add `crates/anthropic-oauth` to crate inventory

**Not modified** (`sdks/rust/CHANGELOG.md` is `motosan-ai`-crate-scoped — verified via commit `d008b96`, the original motosan-ai-oauth v0.1.0 PR, which did not touch it).

**Create:**
- `sdks/rust/crates/motosan-ai-oauth/src/providers/anthropic.rs` — provider config
- `sdks/rust/crates/motosan-ai-oauth/CHANGELOG.md` — per-crate changelog (does not exist today; created on first version bump after 0.1.0)
- `sdks/rust/crates/codex-oauth/CHANGELOG.md` — per-crate changelog (does not exist today)
- `sdks/rust/crates/anthropic-oauth/Cargo.toml`
- `sdks/rust/crates/anthropic-oauth/CHANGELOG.md`
- `sdks/rust/crates/anthropic-oauth/src/lib.rs`
- `sdks/rust/crates/anthropic-oauth/tests/refresh_integration.rs` — mockito test for `refresh()`
- `sdks/rust/crates/anthropic-oauth/tests/login_live.rs` — `#[ignore]`'d live smoke test
- `.github/workflows/publish-anthropic-oauth.yml` — per-crate publish workflow (mirrors `publish-codex-oauth.yml`)

---

## Branching

- [ ] **Step 0: Create feature branch**

```bash
git checkout -b feat/anthropic-oauth
```

Per project rules in `~/.claude/projects/.../memory/feedback_workflow_pr_vs_direct.md`: every `.rs` / `Cargo.toml` change MUST go through PR + CI. This plan creates a single PR at the end.

---

## Task 1: Add `extra_auth_params` + `TokenBodyFormat` enum (no behavior change yet)

This task introduces both new struct fields needed by `build_auth_url` (`extra_auth_params`) and the enum needed later (`TokenBodyFormat`, defined here so subsequent tasks reference it), updates `build_auth_url`, and fills the new field on existing provider configs to preserve current behavior.

**Files:**
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/lib.rs`
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/providers/codex.rs`
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/providers/gemini.rs`

- [ ] **Step 1: Add failing test for `extra_auth_params` honored**

Append to `sdks/rust/crates/motosan-ai-oauth/src/lib.rs` `tests` module (after the existing `build_auth_url_*` tests):

```rust
#[test]
fn build_auth_url_appends_extra_auth_params() {
    let config = OAuthConfig {
        extra_auth_params: &[("foo", "bar"), ("baz", "qux")],
        ..dummy_config()
    };
    let url = build_auth_url(&config, "c", "s", "http://127.0.0.1:1234/auth/callback");
    assert!(url.contains("foo=bar"));
    assert!(url.contains("baz=qux"));
}

#[test]
fn build_auth_url_no_longer_hardcodes_access_type() {
    // Empty extra_auth_params should produce no access_type query pair.
    let config = OAuthConfig {
        extra_auth_params: &[],
        ..dummy_config()
    };
    let url = build_auth_url(&config, "c", "s", "http://127.0.0.1:1234/auth/callback");
    assert!(!url.contains("access_type="));
}
```

- [ ] **Step 2: Run tests to confirm they fail to compile (struct field missing)**

```bash
cargo test -p motosan-ai-oauth build_auth_url_appends_extra_auth_params 2>&1 | head -20
```

Expected: compile error mentioning `extra_auth_params`.

- [ ] **Step 3: Add `TokenBodyFormat` enum + `extra_auth_params` field**

In `sdks/rust/crates/motosan-ai-oauth/src/lib.rs`, replace the `OAuthConfig` struct and add the enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenBodyFormat {
    Form,
    Json,
}

#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub client_id: &'static str,
    pub client_secret: Option<&'static str>,
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub scopes: &'static [&'static str],
    pub redirect_port: Option<u16>,
    pub extra_auth_params: &'static [(&'static str, &'static str)],
}
```

(The other three new fields — `callback_path`, `redirect_uri_host`, `token_body` — are added in Tasks 2–4. Adding them all at once would cascade compile errors across every file.)

- [ ] **Step 4: Update `build_auth_url` to use `extra_auth_params`**

Replace the existing `build_auth_url` body in `lib.rs`:

```rust
pub(crate) fn build_auth_url(
    config: &OAuthConfig,
    challenge: &str,
    state: &str,
    redirect_uri: &str,
) -> String {
    let mut url = reqwest::Url::parse(config.auth_url).expect("auth_url must be valid");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", config.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", &config.scopes.join(" "))
            .append_pair("state", state)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256");
        for (k, v) in config.extra_auth_params {
            q.append_pair(k, v);
        }
    }
    url.to_string()
}
```

- [ ] **Step 5: Update `dummy_config()` test helper**

In the `tests` module of `lib.rs`, update `dummy_config()`:

```rust
fn dummy_config() -> OAuthConfig {
    OAuthConfig {
        client_id: "test-client",
        client_secret: None,
        auth_url: "https://auth.example.com/oauth/authorize",
        token_url: "https://auth.example.com/oauth/token",
        scopes: &["openid", "profile"],
        redirect_port: None,
        extra_auth_params: &[],
    }
}
```

- [ ] **Step 6: Update `providers/codex.rs` to preserve current behavior**

The current `build_auth_url` always appended `access_type=offline`. To keep Codex behavior identical, add the field:

```rust
pub fn codex() -> OAuthConfig {
    OAuthConfig {
        client_id: "app_EMoamEEZ73f0CkXaXp7hrann",
        client_secret: None,
        auth_url: "https://auth.openai.com/oauth/authorize",
        token_url: "https://auth.openai.com/oauth/token",
        scopes: &["openid", "profile", "email", "offline_access"],
        redirect_port: Some(1455),
        extra_auth_params: &[("access_type", "offline")],
    }
}
```

- [ ] **Step 7: Update `providers/gemini.rs` to preserve current behavior**

```rust
pub fn gemini() -> OAuthConfig {
    OAuthConfig {
        client_id: "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com",
        client_secret: Some("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"),
        auth_url: "https://accounts.google.com/o/oauth2/auth",
        token_url: "https://oauth2.googleapis.com/token",
        scopes: &[
            "https://www.googleapis.com/auth/cloud-platform",
            "https://www.googleapis.com/auth/userinfo.email",
            "https://www.googleapis.com/auth/userinfo.profile",
        ],
        redirect_port: None,
        extra_auth_params: &[("access_type", "offline")],
    }
}
```

- [ ] **Step 8: Run all tests to confirm green**

```bash
cargo test -p motosan-ai-oauth 2>&1 | tail -20
cargo test -p codex-oauth 2>&1 | tail -10
```

Expected: all green. Includes the two new `build_auth_url_*` tests.

- [ ] **Step 9: Format**

```bash
cargo fmt --all
```

- [ ] **Step 10: Commit**

```bash
git add sdks/rust/crates/motosan-ai-oauth/src/lib.rs \
        sdks/rust/crates/motosan-ai-oauth/src/providers/codex.rs \
        sdks/rust/crates/motosan-ai-oauth/src/providers/gemini.rs
git commit -m "feat(oauth): extra_auth_params + TokenBodyFormat enum

Replaces hardcoded access_type=offline in build_auth_url with a
config-driven list. Existing codex/gemini provider configs fill the
field with [('access_type', 'offline')] to preserve current behavior.

TokenBodyFormat enum is added here for type-checking but not used
until Task 4."
```

---

## Task 2: Add `redirect_uri_host` field

**Files:**
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/lib.rs`
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/providers/codex.rs`
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/providers/gemini.rs`

- [ ] **Step 1: Add failing test**

In the `tests` module of `lib.rs`:

```rust
#[test]
fn build_auth_url_uses_redirect_uri_host_in_uri() {
    // The redirect_uri is built by login(); we test the helper that constructs it.
    // The host substring comes from config.redirect_uri_host, not from the bind addr.
    let uri = build_redirect_uri("localhost", 53692, "/callback");
    assert_eq!(uri, "http://localhost:53692/callback");

    let uri = build_redirect_uri("127.0.0.1", 1455, "/auth/callback");
    assert_eq!(uri, "http://127.0.0.1:1455/auth/callback");
}
```

- [ ] **Step 2: Run test to confirm it fails**

```bash
cargo test -p motosan-ai-oauth build_auth_url_uses_redirect_uri_host_in_uri 2>&1 | head -10
```

Expected: compile error (`build_redirect_uri` not found, `redirect_uri_host` not a field).

- [ ] **Step 3: Add `redirect_uri_host` field to `OAuthConfig`**

In `lib.rs`:

```rust
pub struct OAuthConfig {
    pub client_id: &'static str,
    pub client_secret: Option<&'static str>,
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub scopes: &'static [&'static str],
    pub redirect_port: Option<u16>,
    pub redirect_uri_host: &'static str,
    pub extra_auth_params: &'static [(&'static str, &'static str)],
}
```

- [ ] **Step 4: Add `build_redirect_uri` helper and use it in `login`**

Add the helper near `build_auth_url` in `lib.rs`:

```rust
pub(crate) fn build_redirect_uri(host: &str, port: u16, path: &str) -> String {
    format!("http://{host}:{port}{path}")
}
```

Replace the existing line in `login`:

```rust
let redirect_uri = format!("http://127.0.0.1:{}/auth/callback", server.port);
```

with:

```rust
let redirect_uri = build_redirect_uri(
    config.redirect_uri_host,
    server.port,
    "/auth/callback", // Task 3 will replace this with config.callback_path
);
```

- [ ] **Step 5: Update `dummy_config()` and existing provider configs**

In `lib.rs` `tests::dummy_config`:

```rust
redirect_port: None,
redirect_uri_host: "127.0.0.1",
extra_auth_params: &[],
```

In `providers/codex.rs`:

```rust
redirect_port: Some(1455),
redirect_uri_host: "127.0.0.1",
extra_auth_params: &[("access_type", "offline")],
```

In `providers/gemini.rs`:

```rust
redirect_port: None,
redirect_uri_host: "127.0.0.1",
extra_auth_params: &[("access_type", "offline")],
```

- [ ] **Step 6: Run all tests**

```bash
cargo test -p motosan-ai-oauth 2>&1 | tail -20
cargo test -p codex-oauth 2>&1 | tail -10
```

Expected: all green, including the new `build_auth_url_uses_redirect_uri_host_in_uri`.

- [ ] **Step 7: Format**

```bash
cargo fmt --all
```

- [ ] **Step 8: Commit**

```bash
git add sdks/rust/crates/motosan-ai-oauth/src/lib.rs \
        sdks/rust/crates/motosan-ai-oauth/src/providers/codex.rs \
        sdks/rust/crates/motosan-ai-oauth/src/providers/gemini.rs
git commit -m "feat(oauth): redirect_uri_host knob

Splits the bind address (still 127.0.0.1) from the redirect_uri
string (now config-driven). Anthropic registers its redirect URI
with hostname 'localhost'; without this knob we would hit
redirect_uri_mismatch on token exchange."
```

---

## Task 3: Parametrize `callback_path`

**Files:**
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/lib.rs`
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/server.rs`
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/providers/codex.rs`
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/providers/gemini.rs`

- [ ] **Step 1: Add failing test in `server.rs`**

In the `tests` module of `server.rs`, replace the existing `non_callback_path_is_not_callback` and `callback_path_with_code_is_callback` tests with path-parametrized versions, and add one for the Anthropic path:

```rust
#[test]
fn non_callback_path_is_not_callback() {
    assert!(!is_callback_request(&get("/favicon.ico"), "/auth/callback"));
    assert!(!is_callback_request(&get("/"), "/auth/callback"));
}

#[test]
fn callback_path_with_code_is_callback() {
    assert!(is_callback_request(
        &get("/auth/callback?code=abc&state=xyz"),
        "/auth/callback"
    ));
}

#[test]
fn anthropic_callback_path_is_callback() {
    assert!(is_callback_request(
        &get("/callback?code=abc&state=xyz"),
        "/callback"
    ));
}

#[test]
fn auth_callback_path_does_not_match_anthropic_request() {
    // A request to /callback must not be accepted when callback_path is /auth/callback.
    assert!(!is_callback_request(
        &get("/callback?code=abc&state=xyz"),
        "/auth/callback"
    ));
}
```

- [ ] **Step 2: Run tests to confirm compile failure**

```bash
cargo test -p motosan-ai-oauth --lib server:: 2>&1 | head -10
```

Expected: compile error (`is_callback_request` arity mismatch).

- [ ] **Step 3: Update `is_callback_request` signature**

In `server.rs`:

```rust
fn is_callback_request(request: &str, callback_path: &str) -> bool {
    let path = request
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .unwrap_or("");
    path.starts_with(callback_path) && path.contains("code=")
}
```

- [ ] **Step 4: Update `wait_for_callback` to accept the path**

In `server.rs`, change the signature:

```rust
pub async fn wait_for_callback(
    server: BoundServer,
    callback_path: &str,
) -> Result<(String, String), Error> {
    let BoundServer { listener, .. } = server;
    loop {
        let (mut stream, _) = listener.accept().await?;
        let buf = read_headers(&mut stream).await?;
        let request = String::from_utf8_lossy(&buf);

        if !is_callback_request(&request, callback_path) {
            let _ = stream
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .await;
            continue;
        }
        // ... rest unchanged
```

- [ ] **Step 5: Add `callback_path` to `OAuthConfig`**

In `lib.rs`:

```rust
pub struct OAuthConfig {
    pub client_id: &'static str,
    pub client_secret: Option<&'static str>,
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub scopes: &'static [&'static str],
    pub redirect_port: Option<u16>,
    pub callback_path: &'static str,
    pub redirect_uri_host: &'static str,
    pub extra_auth_params: &'static [(&'static str, &'static str)],
}
```

- [ ] **Step 6: Wire `callback_path` through `login`**

In `lib.rs::login`:

```rust
let redirect_uri = build_redirect_uri(
    config.redirect_uri_host,
    server.port,
    config.callback_path,
);

// ...

let (code, returned_state) = tokio::time::timeout(
    std::time::Duration::from_secs(LOGIN_TIMEOUT_SECS),
    server::wait_for_callback(server, config.callback_path),
)
.await
.map_err(|_| {
    Error::Callback(format!(
        "timed out waiting for browser callback ({LOGIN_TIMEOUT_SECS}s)"
    ))
})??;
```

- [ ] **Step 7: Fill `callback_path` in `dummy_config()` and existing provider configs**

In `lib.rs::tests::dummy_config`: add `callback_path: "/auth/callback",`
In `providers/codex.rs`: add `callback_path: "/auth/callback",`
In `providers/gemini.rs`: add `callback_path: "/auth/callback",`

- [ ] **Step 8: Run all tests**

```bash
cargo test -p motosan-ai-oauth 2>&1 | tail -20
cargo test -p codex-oauth 2>&1 | tail -10
```

Expected: all green. The four `*_callback_path_*` tests pass.

- [ ] **Step 9: Format**

```bash
cargo fmt --all
```

- [ ] **Step 10: Commit**

```bash
git add sdks/rust/crates/motosan-ai-oauth/src/lib.rs \
        sdks/rust/crates/motosan-ai-oauth/src/server.rs \
        sdks/rust/crates/motosan-ai-oauth/src/providers/codex.rs \
        sdks/rust/crates/motosan-ai-oauth/src/providers/gemini.rs
git commit -m "feat(oauth): config-driven callback_path

is_callback_request and wait_for_callback now accept the path from
OAuthConfig instead of hardcoding /auth/callback. Anthropic registers
its callback at /callback."
```

---

## Task 4: Parametrize token-body format (`TokenBodyFormat::Form` vs `Json`)

**Files:**
- Modify: `sdks/rust/crates/motosan-ai-oauth/Cargo.toml`
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/lib.rs`
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/exchange.rs`
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/providers/codex.rs`
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/providers/gemini.rs`

- [ ] **Step 1: Add `mockito` as a dev-dep on `motosan-ai-oauth`**

In `sdks/rust/crates/motosan-ai-oauth/Cargo.toml`, replace the `[dev-dependencies]` block with:

```toml
[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt"] }
mockito = "1"
```

No new runtime dependencies are needed: `reqwest`'s existing `json` feature pulls in serialization support, and `HashMap<&str, &str>` already implements `Serialize` via `serde`.

- [ ] **Step 2: Add failing tests in `exchange.rs`**

Append to `sdks/rust/crates/motosan-ai-oauth/src/exchange.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::TokenBodyFormat;
    use mockito::Matcher;

    #[tokio::test]
    async fn exchange_code_sends_form_body_when_format_is_form() {
        let mut server = mockito::Server::new_async().await;
        let token_url: &'static str =
            Box::leak(format!("{}/token", server.url()).into_boxed_str());

        let mock = server
            .mock("POST", "/token")
            .match_header(
                "content-type",
                Matcher::Regex("application/x-www-form-urlencoded".into()),
            )
            .match_body(Matcher::AllOf(vec![
                Matcher::UrlEncoded("grant_type".into(), "authorization_code".into()),
                Matcher::UrlEncoded("code".into(), "AUTHCODE".into()),
                Matcher::UrlEncoded("client_id".into(), "test-client".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"access_token":"AT","refresh_token":"RT","expires_in":3600}"#)
            .create_async()
            .await;

        let cfg = crate::OAuthConfig {
            client_id: "test-client",
            client_secret: None,
            auth_url: "https://unused/",
            token_url,
            scopes: &[],
            redirect_port: None,
            callback_path: "/auth/callback",
            redirect_uri_host: "127.0.0.1",
            token_body: TokenBodyFormat::Form,
            extra_auth_params: &[],
        };
        let token = exchange_code(&cfg, "AUTHCODE", "VERIFIER", "http://127.0.0.1/cb")
            .await
            .expect("ok");
        assert_eq!(token.access_token, "AT");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn exchange_code_sends_json_body_when_format_is_json() {
        let mut server = mockito::Server::new_async().await;
        let token_url: &'static str =
            Box::leak(format!("{}/token", server.url()).into_boxed_str());

        let mock = server
            .mock("POST", "/token")
            .match_header("content-type", Matcher::Regex("application/json".into()))
            // Matcher::JsonString compares parsed JSON, so key order in the
            // serialized HashMap does not matter.
            .match_body(Matcher::JsonString(
                r#"{"grant_type":"authorization_code","code":"AUTHCODE","redirect_uri":"http://127.0.0.1/cb","code_verifier":"VERIFIER","client_id":"test-client"}"#
                .into(),
            ))
            .with_status(200)
            .with_body(r#"{"access_token":"AT","refresh_token":"RT","expires_in":3600}"#)
            .create_async()
            .await;

        let cfg = crate::OAuthConfig {
            client_id: "test-client",
            client_secret: None,
            auth_url: "https://unused/",
            token_url,
            scopes: &[],
            redirect_port: None,
            callback_path: "/auth/callback",
            redirect_uri_host: "127.0.0.1",
            token_body: TokenBodyFormat::Json,
            extra_auth_params: &[],
        };
        let token = exchange_code(&cfg, "AUTHCODE", "VERIFIER", "http://127.0.0.1/cb")
            .await
            .expect("ok");
        assert_eq!(token.access_token, "AT");
        mock.assert_async().await;
    }
}
```

The `Box::leak` trick is required because `OAuthConfig::token_url` is `&'static str`; mockito's server URL is a runtime value, so we leak the formatted string to give it `'static` lifetime. This is acceptable in unit tests (process exits soon after).

- [ ] **Step 3: Run tests to confirm compile failure**

```bash
cargo test -p motosan-ai-oauth --lib exchange::tests:: 2>&1 | head -20
```

Expected: compile error (`token_body` field missing).

- [ ] **Step 4: Add `token_body` field to `OAuthConfig`**

In `lib.rs`:

```rust
pub struct OAuthConfig {
    pub client_id: &'static str,
    pub client_secret: Option<&'static str>,
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub scopes: &'static [&'static str],
    pub redirect_port: Option<u16>,
    pub callback_path: &'static str,
    pub redirect_uri_host: &'static str,
    pub token_body: TokenBodyFormat,
    pub extra_auth_params: &'static [(&'static str, &'static str)],
}
```

- [ ] **Step 5: Update `post_token` in `exchange.rs`**

Replace the `post_token` function:

```rust
async fn post_token(
    token_url: &str,
    body_format: crate::TokenBodyFormat,
    params: Vec<(&str, &str)>,
    fallback_refresh: Option<&str>,
) -> Result<Token, Error> {
    let client = reqwest::Client::new();
    let req = client
        .post(token_url)
        .header("User-Agent", "Mozilla/5.0 (compatible; motosan-ai-oauth)");

    let req = match body_format {
        crate::TokenBodyFormat::Form => req.form(&params),
        crate::TokenBodyFormat::Json => {
            let map: std::collections::HashMap<&str, &str> = params.iter().copied().collect();
            req.json(&map)
        }
    };

    let resp = req.send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(Error::TokenExchange(format!("HTTP {status}: {body}")));
    }

    Ok(resp
        .json::<RawTokenResponse>()
        .await?
        .into_token(fallback_refresh))
}
```

Update `exchange_code` and `refresh_token` to pass `config.token_body`:

```rust
pub async fn exchange_code(
    config: &OAuthConfig,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<Token, Error> {
    let mut params = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("code_verifier", verifier),
        ("client_id", config.client_id),
    ];
    if let Some(secret) = config.client_secret {
        params.push(("client_secret", secret));
    }
    post_token(config.token_url, config.token_body, params, None).await
}

pub async fn refresh_token(config: &OAuthConfig, refresh_token: &str) -> Result<Token, Error> {
    let mut params = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", config.client_id),
    ];
    if let Some(secret) = config.client_secret {
        params.push(("client_secret", secret));
    }
    post_token(
        config.token_url,
        config.token_body,
        params,
        Some(refresh_token),
    )
    .await
}
```

- [ ] **Step 6: Fill `token_body` in `dummy_config()` and existing provider configs**

In `lib.rs::tests::dummy_config`: add `token_body: TokenBodyFormat::Form,`
In `providers/codex.rs`: add `token_body: TokenBodyFormat::Form,` and add `use crate::TokenBodyFormat;` at the top.
In `providers/gemini.rs`: same.

- [ ] **Step 7: Run all tests**

```bash
cargo test -p motosan-ai-oauth 2>&1 | tail -25
cargo test -p codex-oauth 2>&1 | tail -10
```

Expected: all green, including both `exchange_code_sends_*_body_when_format_is_*` tests.

- [ ] **Step 8: Format**

```bash
cargo fmt --all
```

- [ ] **Step 9: Commit**

```bash
git add sdks/rust/crates/motosan-ai-oauth/
git commit -m "feat(oauth): TokenBodyFormat::Form|Json switch in post_token

exchange_code and refresh_token now use config.token_body to pick
either application/x-www-form-urlencoded or application/json bodies.
Codex and Gemini stay on Form to preserve current behavior."
```

---

## Task 5: Add Anthropic provider config (feature-gated)

**Files:**
- Modify: `sdks/rust/crates/motosan-ai-oauth/Cargo.toml`
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/providers/mod.rs`
- Create: `sdks/rust/crates/motosan-ai-oauth/src/providers/anthropic.rs`

- [ ] **Step 1: Add `anthropic` feature to `motosan-ai-oauth/Cargo.toml`**

In `[features]`:

```toml
[features]
default = []
codex = []
gemini = []
anthropic = []
```

- [ ] **Step 2: Register the module**

In `sdks/rust/crates/motosan-ai-oauth/src/providers/mod.rs`, append:

```rust
#[cfg(feature = "anthropic")]
pub mod anthropic;
```

- [ ] **Step 3: Create the provider config**

Write `sdks/rust/crates/motosan-ai-oauth/src/providers/anthropic.rs`:

```rust
//! Anthropic Claude Pro/Max OAuth provider config.
//!
//! The `client_id` below is extracted from Anthropic's Claude Code CLI
//! authentication flow; the same value is used by the reference
//! implementation `@earendil-works/pi-ai`. Anthropic has not published this
//! client_id as a public app registration for third-party use — see the ToS
//! disclosure in the top-level README.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_pro_max_has_expected_client_id() {
        assert_eq!(
            claude_pro_max().client_id,
            "9d1c250a-e61b-44d9-88ed-5944d1962f5e"
        );
    }

    #[test]
    fn claude_pro_max_has_no_client_secret() {
        assert!(claude_pro_max().client_secret.is_none());
    }

    #[test]
    fn claude_pro_max_redirect_port_is_53692() {
        assert_eq!(claude_pro_max().redirect_port, Some(53692));
    }

    #[test]
    fn claude_pro_max_callback_path_is_slash_callback() {
        assert_eq!(claude_pro_max().callback_path, "/callback");
    }

    #[test]
    fn claude_pro_max_redirect_uri_host_is_localhost() {
        // Anthropic registers redirect_uri with literal "localhost", not "127.0.0.1".
        assert_eq!(claude_pro_max().redirect_uri_host, "localhost");
    }

    #[test]
    fn claude_pro_max_token_body_is_json() {
        assert_eq!(claude_pro_max().token_body, TokenBodyFormat::Json);
    }

    #[test]
    fn claude_pro_max_extra_auth_params_include_code_true() {
        let params = claude_pro_max().extra_auth_params;
        assert!(params.iter().any(|(k, v)| *k == "code" && *v == "true"));
    }

    #[test]
    fn claude_pro_max_scopes_include_claude_code_session() {
        assert!(claude_pro_max()
            .scopes
            .iter()
            .any(|s| *s == "user:sessions:claude_code"));
    }

    #[test]
    fn claude_pro_max_auth_url_is_claude_ai() {
        assert!(claude_pro_max().auth_url.contains("claude.ai"));
    }
}
```

- [ ] **Step 4: Run the new tests**

```bash
cargo test -p motosan-ai-oauth --features anthropic providers::anthropic 2>&1 | tail -20
```

Expected: 9 tests pass.

- [ ] **Step 5: Verify default-feature build still works**

```bash
cargo build -p motosan-ai-oauth 2>&1 | tail -5
cargo test -p motosan-ai-oauth 2>&1 | tail -5
```

Expected: green. `anthropic` module is gated off so its absence doesn't break anything.

- [ ] **Step 6: Format**

```bash
cargo fmt --all
```

- [ ] **Step 7: Commit**

```bash
git add sdks/rust/crates/motosan-ai-oauth/Cargo.toml \
        sdks/rust/crates/motosan-ai-oauth/src/providers/mod.rs \
        sdks/rust/crates/motosan-ai-oauth/src/providers/anthropic.rs
git commit -m "feat(oauth): add anthropic provider config

Feature-gated under 'anthropic'. Declarative — Anthropic's OAuth
divergences (callback path, redirect_uri host, JSON token body,
'code=true' auth param) are all expressed via the new OAuthConfig
fields added in Tasks 1-4. client_id extracted from Claude Code CLI;
ToS disclosure deferred to README (Task 7)."
```

---

## Task 6: New `anthropic-oauth` wrapper crate

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Create: `sdks/rust/crates/anthropic-oauth/Cargo.toml`
- Create: `sdks/rust/crates/anthropic-oauth/src/lib.rs`
- Create: `sdks/rust/crates/anthropic-oauth/tests/refresh_integration.rs`

- [ ] **Step 1: Register crate in workspace**

In `Cargo.toml` (workspace root):

```toml
[workspace]
members = [
  "sdks/rust",
  "sdks/rust/crates/anthropic-oauth",
  "sdks/rust/crates/codex-oauth",
  "sdks/rust/crates/motosan-ai-oauth",
]
resolver = "2"
```

- [ ] **Step 2: Write `Cargo.toml` for the new crate**

Create `sdks/rust/crates/anthropic-oauth/Cargo.toml`:

```toml
[package]
name = "anthropic-oauth"
version = "0.1.0"
edition = "2021"
rust-version = "1.82"
license = "MIT"
description = "OAuth login for Anthropic Claude Pro/Max (Claude Code session)"
repository = "https://github.com/motosan-dev/motosan-ai"
homepage = "https://github.com/motosan-dev/motosan-ai"
documentation = "https://docs.rs/anthropic-oauth"
keywords = ["anthropic", "claude", "oauth", "authentication"]
categories = ["authentication", "api-bindings"]

[dependencies]
motosan-ai-oauth = { path = "../motosan-ai-oauth", features = ["anthropic"] }

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt"] }
mockito = "1"
```

- [ ] **Step 3: Write the failing unit test inside `src/lib.rs`**

Create `sdks/rust/crates/anthropic-oauth/src/lib.rs`:

```rust
//! PKCE OAuth login for Anthropic Claude Pro/Max.
//!
//! Thin wrapper around [`motosan_ai_oauth`] with the Anthropic provider
//! pre-configured.
//!
//! # Usage
//!
//! ```no_run
//! #[tokio::main]
//! async fn main() -> Result<(), anthropic_oauth::Error> {
//!     let token = anthropic_oauth::login().await?;
//!     // `token.access_token` is a "sk-ant-oat01-*" string usable directly
//!     // with motosan_ai's AnthropicProvider (which auto-detects the prefix
//!     // and applies Claude Code identity headers).
//!     println!("{}", token.access_token);
//!     Ok(())
//! }
//! ```
//!
//! # Notes
//!
//! - Requires port 53692 to be free on localhost (registered with Anthropic's
//!   Claude Code app).
//! - `client_id` is hardcoded to Anthropic's Claude Code app registration and
//!   is not configurable. See the project README for the associated ToS
//!   disclosure before depending on this crate.

pub use motosan_ai_oauth::{Error, Token};

/// Open a browser-based PKCE login flow and return the resulting OAuth token.
pub async fn login() -> Result<Token, Error> {
    motosan_ai_oauth::login(&motosan_ai_oauth::providers::anthropic::claude_pro_max()).await
}

/// Exchange a stored refresh token for a new [`Token`].
pub async fn refresh(refresh_token: &str) -> Result<Token, Error> {
    motosan_ai_oauth::refresh(
        &motosan_ai_oauth::providers::anthropic::claude_pro_max(),
        refresh_token,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn re_exports_compile() {
        let _: fn() -> bool = || {
            let t = Token {
                access_token: String::new(),
                refresh_token: String::new(),
                id_token: None,
                expires_in: 0,
                issued_at: 0,
            };
            t.is_expired()
        };
    }
}
```

- [ ] **Step 4: Write the refresh integration test**

Create `sdks/rust/crates/anthropic-oauth/tests/refresh_integration.rs`:

```rust
//! Integration test: refresh() POSTs JSON to the configured token_url.
//!
//! We can't easily redirect `claude_pro_max().token_url` to a mock server
//! (it's a `&'static str`), so this test exercises the same code path by
//! calling `motosan_ai_oauth::refresh` directly with a mock-server config.

use mockito::Matcher;
use motosan_ai_oauth::{OAuthConfig, TokenBodyFormat};

#[tokio::test]
async fn refresh_posts_json_body_when_token_body_is_json() {
    let mut server = mockito::Server::new_async().await;
    let token_url: &'static str =
        Box::leak(format!("{}/token", server.url()).into_boxed_str());

    let mock = server
        .mock("POST", "/token")
        .match_header("content-type", Matcher::Regex("application/json".into()))
        .match_body(Matcher::JsonString(
            r#"{"grant_type":"refresh_token","refresh_token":"OLD_REFRESH","client_id":"test-client"}"#
            .into(),
        ))
        .with_status(200)
        .with_body(
            r#"{"access_token":"NEW_AT","refresh_token":"NEW_RT","expires_in":3600}"#,
        )
        .create_async()
        .await;

    let cfg = OAuthConfig {
        client_id: "test-client",
        client_secret: None,
        auth_url: "https://unused/",
        token_url,
        scopes: &[],
        redirect_port: None,
        callback_path: "/callback",
        redirect_uri_host: "localhost",
        token_body: TokenBodyFormat::Json,
        extra_auth_params: &[],
    };

    let token = motosan_ai_oauth::refresh(&cfg, "OLD_REFRESH")
        .await
        .expect("refresh ok");
    assert_eq!(token.access_token, "NEW_AT");
    assert_eq!(token.refresh_token, "NEW_RT");
    mock.assert_async().await;
}
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p anthropic-oauth 2>&1 | tail -15
```

Expected: 2 tests pass (`re_exports_compile`, `refresh_posts_json_body_when_token_body_is_json`).

- [ ] **Step 6: Format**

```bash
cargo fmt --all
```

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml \
        sdks/rust/crates/anthropic-oauth/
git commit -m "feat(anthropic-oauth): new wrapper crate

Mirrors codex-oauth's shape exactly. Exposes login() / refresh() /
Token / Error pre-configured for Anthropic's Claude Pro/Max flow.
Integration test verifies refresh sends JSON, not form."
```

---

## Task 7: Docs, version bumps, per-crate CHANGELOGs, and release tooling

This task is large (lots of small edits) but logically one commit: "everything needed to make `anthropic-oauth` releasable the same way `codex-oauth` is."

**Files:**
- Modify: `sdks/rust/crates/motosan-ai-oauth/Cargo.toml` (minor bump 0.1.0 → 0.2.0)
- Modify: `sdks/rust/crates/codex-oauth/Cargo.toml` (patch bump 0.1.0 → 0.1.1)
- Create: `sdks/rust/crates/motosan-ai-oauth/CHANGELOG.md`
- Create: `sdks/rust/crates/codex-oauth/CHANGELOG.md`
- Create: `sdks/rust/crates/anthropic-oauth/CHANGELOG.md`
- Create: `.github/workflows/publish-anthropic-oauth.yml`
- Modify: `llms.txt` (add anthropic-oauth section + release-tag entry + Release Steps + CI Pipeline mention; bump in-line `motosan-ai-oauth = "0.1"` reference to `"0.2"`)
- Modify: `README.md` (top-level — add Anthropic OAuth section + ToS disclosure)
- Modify: `AGENTS.md` (crate inventory)

**Note on the SDK-level `sdks/rust/CHANGELOG.md`**: that file is **not** touched. It is `motosan-ai`-crate-scoped — verified by inspecting commit `d008b96` (motosan-ai-oauth v0.1.0 PR), which did not modify it. OAuth crates each maintain their own per-crate CHANGELOG, per the release process documented in `llms.txt`.

### Version bumps

- [ ] **Step 1: Bump `motosan-ai-oauth` to `0.2.0`**

In `sdks/rust/crates/motosan-ai-oauth/Cargo.toml`:

```toml
version = "0.2.0"
```

Rationale: `OAuthConfig` grew four required fields and one new public enum — out-of-tree consumers that construct `OAuthConfig` literals need to add the new fields, so this is a breaking change for them.

- [ ] **Step 2: Bump `codex-oauth` to `0.1.1`**

In `sdks/rust/crates/codex-oauth/Cargo.toml`:

```toml
version = "0.1.1"
```

Rationale: source touched but public API unchanged. Patch bump.

### Per-crate CHANGELOG files

These files do not currently exist (verified: `ls crates/{motosan-ai-oauth,codex-oauth}/CHANGELOG.md` returns "No such file or directory"). They are referenced by the release process in `llms.txt` and are created on the first version bump after `0.1.0`. This task creates all three.

- [ ] **Step 3: Create `sdks/rust/crates/motosan-ai-oauth/CHANGELOG.md`**

```markdown
# Changelog

All notable changes to `motosan-ai-oauth` are documented in this file.

## [0.2.0] - YYYY-MM-DD

### Added
- `TokenBodyFormat` enum (`Form` | `Json`) controlling the body encoding
  used by `exchange_code` and `refresh_token`.
- `OAuthConfig::callback_path` — the HTTP path the callback server matches
  against. Defaults vary per provider config (codex/gemini: `/auth/callback`,
  Anthropic: `/callback`).
- `OAuthConfig::redirect_uri_host` — the host string used inside the
  `redirect_uri` query parameter. Separate from the bind address (which is
  always `127.0.0.1`) so providers like Anthropic that register their
  redirect URI as `http://localhost:...` work without changing the listen
  socket.
- `OAuthConfig::token_body` — selects `TokenBodyFormat::Form` (existing
  behavior) or `Json`.
- `OAuthConfig::extra_auth_params` — replaces the previously hardcoded
  `access_type=offline` query parameter with a config-driven list.
- `anthropic` feature flag exposing `providers::anthropic::claude_pro_max()`.

### Changed
- **Breaking (for out-of-tree consumers constructing `OAuthConfig` literals
  directly):** four new required fields on `OAuthConfig`. The built-in
  provider configs (`providers::codex`, `providers::gemini`) are updated to
  set values that preserve previous behavior; consumers using them are
  unaffected.

## [0.1.0] - 2026-04-20

Initial release: generic PKCE OAuth login + refresh with built-in Codex
and Gemini provider configs.
```

Replace `YYYY-MM-DD` with the date of the release commit when you tag.

- [ ] **Step 4: Create `sdks/rust/crates/codex-oauth/CHANGELOG.md`**

```markdown
# Changelog

All notable changes to `codex-oauth` are documented in this file.

## [0.1.1] - YYYY-MM-DD

### Changed
- Internal: provider config updated for `motosan-ai-oauth` v0.2.0's
  expanded `OAuthConfig` (new `callback_path`, `redirect_uri_host`,
  `token_body`, `extra_auth_params` fields). Values preserve previous
  Codex behavior (form-encoded token body, `/auth/callback` path,
  `127.0.0.1` redirect host, `access_type=offline` auth param). Public
  API of `codex-oauth` is unchanged.

## [0.1.0] - 2026-04-19

Initial release.
```

- [ ] **Step 5: Create `sdks/rust/crates/anthropic-oauth/CHANGELOG.md`**

```markdown
# Changelog

All notable changes to `anthropic-oauth` are documented in this file.

## [0.1.0] - YYYY-MM-DD

Initial release. PKCE OAuth login and refresh for Anthropic Claude
Pro/Max. The resulting `sk-ant-oat01-*` access token is consumed
directly by `motosan-ai`'s `AnthropicProvider` (the setup-token code
path applies Bearer auth + Claude Code identity headers
automatically).

See the project README for the ToS disclosure regarding use of
Anthropic's Claude Code OAuth `client_id`.
```

### Publish workflow

- [ ] **Step 6: Create `.github/workflows/publish-anthropic-oauth.yml`**

Mirror `.github/workflows/publish-codex-oauth.yml` exactly:

```yaml
name: publish-anthropic-oauth

on:
  push:
    tags:
      - "anthropic-oauth-v*"
  workflow_dispatch:

jobs:
  publish:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: sdks/rust/crates/anthropic-oauth
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: sdks/rust/crates/anthropic-oauth
      - run: cargo fmt --all -- --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test
      - name: Publish to crates.io
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
        run: cargo publish
```

### llms.txt updates

`llms.txt` is the release-tooling source-of-truth. It already has a `## codex-oauth` section, an entry in the release-tag table, full Release Steps, and a CI Pipeline mention. Mirror all four for `anthropic-oauth`.

- [ ] **Step 7: Add `## anthropic-oauth` section to `llms.txt`**

Find the existing `## codex-oauth` block (currently around line 785, between the `---` after the Codex CLI notes and the `## Release` heading). Insert a new section **after** the closing `---` of `## codex-oauth`:

````markdown
---

## anthropic-oauth

Standalone crate for browser-based PKCE OAuth login against `claude.ai`. Returns an `sk-ant-oat01-*` access token usable with `motosan-ai`'s `AnthropicProvider` (which auto-detects the prefix and applies Claude Code identity headers).

- crates.io: https://crates.io/crates/anthropic-oauth
- Version: 0.1.0

### Install

```toml
anthropic-oauth = "0.1"
```

### API

```rust
// Login (opens browser, listens on 127.0.0.1:53692, times out after 120s).
// The redirect URI registered with Anthropic uses hostname "localhost"; the
// bind address is still 127.0.0.1 — the OAuthConfig handles this split.
let token = anthropic_oauth::login().await?;

// Refresh
let token = anthropic_oauth::refresh(&token.refresh_token).await?;

// Expiry check
if token.is_expired() { /* refresh */ }

// Token fields (same shape as codex-oauth)
token.access_token   // "sk-ant-oat01-..." — Bearer for Anthropic API
token.refresh_token  // long-lived, use with refresh()
token.expires_in     // lifetime in seconds
token.issued_at      // Unix timestamp of issue time
```

`Token` implements `Serialize`/`Deserialize` for disk persistence.

**ToS disclosure**: this crate uses the OAuth `client_id` registered by Anthropic's Claude Code CLI. The resulting access token authenticates as a Claude Code CLI session. Anthropic has not published this `client_id` for third-party use; using it for purposes other than running `claude` CLI may be subject to change, rate limited, or violate Anthropic's terms of service. See the project README for the full disclosure.
````

- [ ] **Step 8: Add `anthropic-oauth` row to the release-tag table in `llms.txt`**

Find the table around the line that contains `| codex-oauth  | \`codex-oauth-vX.Y.Z\`  | \`codex-oauth-v0.1.0\`  |` and add a row below it:

```
| anthropic-oauth | `anthropic-oauth-vX.Y.Z` | `anthropic-oauth-v0.1.0` |
```

(Column alignment may need a couple of extra spaces — match the table's existing style; markdown doesn't care about exact spacing.)

- [ ] **Step 9: Add `### Release Steps (anthropic-oauth)` block to `llms.txt`**

Mirror the existing `### Release Steps (codex-oauth)` block. Insert immediately after it:

````markdown
### Release Steps (anthropic-oauth)

```bash
# 1. Bump version
#    sdks/rust/crates/anthropic-oauth/Cargo.toml → version = "0.1.1"

# 2. Update CHANGELOG
#    sdks/rust/crates/anthropic-oauth/CHANGELOG.md → ## [0.1.1] - YYYY-MM-DD

# 3. Commit
git add sdks/rust/crates/anthropic-oauth/Cargo.toml sdks/rust/crates/anthropic-oauth/CHANGELOG.md
git commit -m "chore: release anthropic-oauth-v0.1.1"

# 4. Tag + push (triggers publish-anthropic-oauth.yml → crates.io)
git tag -a anthropic-oauth-v0.1.1 -m "anthropic-oauth-v0.1.1 — summary"
git push origin main anthropic-oauth-v0.1.1
```
````

- [ ] **Step 10: Add `publish-anthropic-oauth.yml` to the CI Pipeline bullet list in `llms.txt`**

Find the `### CI Pipeline` section. There is a bullet list documenting each publish workflow. Add a line below the `publish-codex-oauth.yml` bullet:

```markdown
- **publish-anthropic-oauth.yml**: `cargo fmt --check` → `cargo clippy` → `cargo test` → `cargo publish` (secret: `CARGO_REGISTRY_TOKEN`)
```

- [ ] **Step 11: Bump in-line `motosan-ai-oauth` version reference in `llms.txt`**

Around line 548 there is a code snippet showing:

```
motosan-ai-oauth = { version = "0.1", features = ["gemini"] }
```

Update to `version = "0.2"`. If there are other `motosan-ai-oauth = "0.1"` references in the file, update each.

Verify with:

```bash
grep -n 'motosan-ai-oauth.*=.*"0\.' llms.txt
```

All matches should now show `"0.2"`.

### User-facing docs

- [ ] **Step 12: Add Anthropic OAuth section to top-level `README.md`**

Find an appropriate insertion point (near the existing setup-token example around line 87 of `README.md` — search for `sk-ant-oat01`). Insert:

````markdown
### Anthropic OAuth (Claude Pro/Max)

The `anthropic-oauth` crate lets you obtain an Anthropic OAuth token tied to a
Claude Pro/Max subscription. The resulting `sk-ant-oat01-*` token is consumed
directly by `AnthropicProvider`, which auto-detects the prefix and applies the
Claude Code identity headers.

```rust
use anthropic_oauth;
use motosan_ai::providers::anthropic::AnthropicProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = anthropic_oauth::login().await?;
    let provider = AnthropicProvider::new(&token.access_token, None, None);
    // Use `provider` as usual.
    Ok(())
}
```

**⚠️ Important ToS disclosure**

This crate uses the OAuth `client_id` registered by Anthropic's Claude Code CLI.
The resulting access token authenticates your requests **as a Claude Code CLI
session**, not as an API-key holder. Anthropic has not published this
`client_id` as a public app registration for third-party use; using it for
purposes other than running `claude` CLI may be subject to change, may be rate
limited, and may violate Anthropic's terms of service. You are responsible for
ensuring your usage complies with Anthropic's terms.

If you have an API key (`sk-ant-api03-*`), prefer that path — it does not
require this crate.
````

This is a new precedent (`codex-oauth` currently has no README section). Approved by the spec; revisit whether to backfill a `codex-oauth` README section in a follow-up.

- [ ] **Step 13: Update `AGENTS.md`**

Open `AGENTS.md`. Search for `codex-oauth`. Mirror the existing `codex-oauth` entry/entries with a parallel `anthropic-oauth` entry. The exact wording depends on the section ("Crate inventory", "Releasing", etc.) — copy the surrounding format and adapt.

### Finalize

- [ ] **Step 14: Format**

```bash
cargo fmt --all
```

(No `.rs` changed here, but harmless. Catches any stray formatting drift from earlier tasks.)

- [ ] **Step 15: Commit**

```bash
git add sdks/rust/crates/motosan-ai-oauth/Cargo.toml \
        sdks/rust/crates/motosan-ai-oauth/CHANGELOG.md \
        sdks/rust/crates/codex-oauth/Cargo.toml \
        sdks/rust/crates/codex-oauth/CHANGELOG.md \
        sdks/rust/crates/anthropic-oauth/CHANGELOG.md \
        .github/workflows/publish-anthropic-oauth.yml \
        llms.txt README.md AGENTS.md
git commit -m "docs: anthropic-oauth release tooling + README/ToS

- motosan-ai-oauth: 0.1.0 -> 0.2.0 (breaking OAuthConfig change)
- codex-oauth: 0.1.0 -> 0.1.1 (source-only follow-up)
- anthropic-oauth: 0.1.0 initial release
- Per-crate CHANGELOG.md files created (none existed prior).
- llms.txt updated with anthropic-oauth section, release-tag entry,
  Release Steps, and CI Pipeline mention.
- New publish-anthropic-oauth.yml workflow mirrors codex-oauth's.
- README.md gains an Anthropic OAuth section with explicit ToS
  disclosure. AGENTS.md crate inventory updated."
```

---

## Task 8: Live `#[ignore]`'d smoke test

Per memory `feedback_plan_writing_lessons`: HTTP-touching plans should include a `#[ignore]`'d live test in the Done Criteria.

**Files:**
- Create: `sdks/rust/crates/anthropic-oauth/tests/login_live.rs`

- [ ] **Step 1: Write the live test**

Create `sdks/rust/crates/anthropic-oauth/tests/login_live.rs`:

```rust
//! Live login smoke test.
//!
//! `#[ignore]`'d by default — running it pops open a browser window,
//! requires a Claude Pro/Max account, and binds to port 53692 on
//! localhost. It is intended to be run manually before releasing a new
//! version of `anthropic-oauth`:
//!
//! ```bash
//! cargo test -p anthropic-oauth --test login_live -- --ignored
//! ```
//!
//! Success criterion: the returned `Token.access_token` starts with the
//! `sk-ant-oat01-` prefix and the refresh token is non-empty.

#[tokio::test]
#[ignore = "interactive: opens a browser and requires Claude Pro/Max account"]
async fn live_login_returns_setup_token() {
    let token = anthropic_oauth::login()
        .await
        .expect("interactive login must succeed");

    assert!(
        token.access_token.starts_with("sk-ant-oat01-"),
        "access_token did not have expected setup-token prefix; got: {}",
        &token.access_token.chars().take(20).collect::<String>(),
    );
    assert!(
        !token.refresh_token.is_empty(),
        "refresh_token must be non-empty for subsequent refresh()"
    );
    assert!(
        token.expires_in > 0,
        "expires_in must be positive; got {}",
        token.expires_in
    );

    eprintln!("Live login OK. expires_in={}s", token.expires_in);
}
```

- [ ] **Step 2: Verify the test compiles but is skipped by default**

```bash
cargo test -p anthropic-oauth --test login_live 2>&1 | tail -10
```

Expected: the test target compiles cleanly. The summary line reads
`test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out`.
The `1 ignored` is what proves the `#[ignore]` test was discovered (i.e., it
compiled) but was correctly skipped.

- [ ] **Step 3: Run the test manually (engineer action, not automated)**

```bash
cargo test -p anthropic-oauth --test login_live -- --ignored
```

Expected: browser opens to `claude.ai/oauth/authorize`. Log in with a Claude Pro/Max account. Approve. Browser shows "Login successful". Test passes.

**Stop and report** if any of these fail:
- Browser opens to a 404 or "page not found" — likely a wrong `auth_url`.
- After approval the browser stays on the consent screen — `redirect_uri` is being rejected; check `redirect_uri_host` and `callback_path` values match the spec exactly.
- Test fails with `TokenExchange("HTTP 400: ...")` — the body contains the error; common cases include invalid `client_id` (typo) or scopes Anthropic no longer accepts.

- [ ] **Step 4: Format**

```bash
cargo fmt --all
```

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/crates/anthropic-oauth/tests/login_live.rs
git commit -m "test(anthropic-oauth): live login smoke test (#[ignore])

Not run in CI. Manual gate before publishing a new anthropic-oauth
version: ensures the full PKCE round trip against claude.ai still
produces a valid sk-ant-oat01-* token."
```

---

## Task 9: Full-repo sanity check + PR

- [ ] **Step 1: Run the full Rust check (`check-rust` from `CLAUDE.md`)**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace --all-features
```

Expected: all green.

- [ ] **Step 2: Verify the regression gates from the spec**

```bash
cargo test -p motosan-ai-oauth --all-features 2>&1 | grep "test result"
cargo test -p codex-oauth 2>&1 | grep "test result"
cargo test -p motosan-ai --features anthropic --test anthropic_oauth_usage 2>&1 | grep "test result"
```

Expected: each line `test result: ok. ... 0 failed` with at least 1 passed
on the `anthropic_oauth_usage` line. **Critical**: the `--features anthropic`
flag on the third command is required — without it, `#![cfg(feature = "anthropic")]`
at the top of `tests/anthropic_oauth_usage.rs` strips the entire file to
zero tests, producing a misleading green `0 passed; 0 failed`.

- [ ] **Step 3: Push the branch and open a PR**

```bash
git push -u origin feat/anthropic-oauth
gh pr create --title "feat(oauth): anthropic-oauth crate (Claude Pro/Max login)" --body "$(cat <<'EOF'
## Summary
- New `anthropic-oauth` crate: PKCE login + refresh against `claude.ai`, returns a `sk-ant-oat01-*` token usable directly with the existing `AnthropicProvider`.
- `motosan-ai-oauth` v0.2.0: `OAuthConfig` gained `callback_path`, `redirect_uri_host`, `token_body`, `extra_auth_params` fields and a new `TokenBodyFormat` enum so Anthropic's quirks (callback at `/callback`, `redirect_uri` host string `localhost`, JSON token body, `code=true` auth param) are expressed declaratively. **Breaking** for out-of-tree consumers constructing `OAuthConfig` literals.
- `codex-oauth` v0.1.1: source-only follow-up to fill the new `OAuthConfig` fields with values preserving current behavior.

Spec: `docs/superpowers/specs/2026-05-18-anthropic-oauth-design.md`
Plan: `docs/superpowers/plans/2026-05-18-anthropic-oauth.md`

⚠️ The README adds an explicit ToS disclosure: the `client_id` is extracted from Claude Code CLI; Anthropic has not published it for third-party use.

## Test plan
- [ ] CI green: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace --all-features`
- [ ] Existing tests untouched in assertions: `motosan-ai-oauth`, `codex-oauth`, `sdks/rust/tests/anthropic_oauth_usage.rs`
- [ ] New tests pass: `build_auth_url_appends_extra_auth_params`, `*_callback_path_*`, `exchange_code_sends_{form,json}_body_*`, all `providers::anthropic::tests::*`, `refresh_posts_json_body_*`
- [ ] Live test passed locally: `cargo test -p anthropic-oauth --test login_live -- --ignored` returned a `sk-ant-oat01-` token (paste-out of test stderr in PR comments)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 4: Report the PR URL**

Paste the PR URL in the chat.

---

## Done Criteria

- [ ] All 9 tasks above complete and committed
- [ ] CI passes on the PR (fmt + clippy + test)
- [ ] `cargo test -p anthropic-oauth --test login_live -- --ignored` was run locally and returned a `sk-ant-oat01-*` token
- [ ] PR URL reported

---

## Self-Review Notes

The plan covers every section of the spec:

- **Goals**: `anthropic_oauth::login()` / `refresh()` → Tasks 5–6. Generalization of `OAuthConfig` → Tasks 1–4. Behavior preservation for codex/gemini → Tasks 1–4 step "fill provider configs". Single-PR scope → Tasks 0 + 9.
- **Non-goals**: Python parity, token storage, runtime warning, CLI binary — all absent from the plan as intended.
- **Architecture Change 1** (extend `OAuthConfig`): Tasks 1, 2, 3, 4 each add one field.
- **Architecture Change 2** (generalize call sites): `build_auth_url` in Task 1; `lib.rs::login` in Tasks 2 and 3; `server.rs::is_callback_request` in Task 3; `exchange.rs::post_token` in Task 4.
- **Architecture Change 3** (Anthropic provider config): Task 5.
- **Architecture Change 4** (`anthropic-oauth` crate): Task 6.
- **Error Handling**: no new variants; covered by reusing existing `Error` enum. No dedicated task — verified by the existing error-mapping behavior in `exchange.rs::post_token`.
- **Testing matrix**: every row mapped — unit `lib.rs` tests (Tasks 1, 2), unit `exchange.rs` mockito (Task 4), unit `server.rs` (Task 3), unit `providers/anthropic.rs` (Task 5), integration mockito (Task 6), live `#[ignore]` (Task 8), regression gates (Task 9 Step 2).
- **Documentation**: README + AGENTS + per-crate CHANGELOGs + `llms.txt` (release tooling SSOT) + new `publish-anthropic-oauth.yml` GitHub workflow — Task 7. `sdks/rust/CHANGELOG.md` is intentionally **not** touched: it is scoped to the `motosan-ai` crate, as verified by commit `d008b96` (motosan-ai-oauth v0.1.0 PR, which did not modify it).
- **Versioning**: minor / patch / new — Task 7. Per-crate CHANGELOG files created in the same task; previously these did not exist for any of the three crates.
