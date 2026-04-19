# motosan-ai-oauth Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generalize `codex-oauth` into `motosan-ai-oauth` — a provider-agnostic PKCE OAuth crate with built-in Gemini and Codex configs behind feature flags, leaving `codex-oauth` as a backward-compatible thin wrapper.

**Architecture:** New crate `sdks/rust/crates/motosan-ai-oauth` contains all PKCE machinery (pkce, server, exchange) plus an `OAuthConfig` struct and top-level `login(config)`/`refresh(config, token)` entry points. Provider configs live in `src/providers/` behind `#[cfg(feature)]`. `codex-oauth` becomes a three-function re-export wrapper that calls into `motosan-ai-oauth`.

**Tech Stack:** Rust, tokio (async), reqwest (HTTP), sha2 + base64 + rand (PKCE), percent-encoding (callback parsing), thiserror (errors).

---

## File Map

| Path | Action | Responsibility |
|---|---|---|
| `sdks/rust/crates/motosan-ai-oauth/Cargo.toml` | Create | Crate manifest, features: `codex`, `gemini` |
| `sdks/rust/crates/motosan-ai-oauth/src/lib.rs` | Create | `OAuthConfig`, `Token`, `login()`, `refresh()`, `build_auth_url()` |
| `sdks/rust/crates/motosan-ai-oauth/src/error.rs` | Create | `Error` enum (same variants as codex-oauth) |
| `sdks/rust/crates/motosan-ai-oauth/src/pkce.rs` | Create | `Pkce::generate()` — copied verbatim from codex-oauth |
| `sdks/rust/crates/motosan-ai-oauth/src/server.rs` | Create | `bind(port) -> BoundServer`, `wait_for_callback(server)` |
| `sdks/rust/crates/motosan-ai-oauth/src/exchange.rs` | Create | `exchange_code(config, code, verifier, redirect_uri)`, `refresh_token(config, token)` |
| `sdks/rust/crates/motosan-ai-oauth/src/providers/mod.rs` | Create | `pub mod codex`, `pub mod gemini` behind features |
| `sdks/rust/crates/motosan-ai-oauth/src/providers/codex.rs` | Create | `pub fn codex() -> OAuthConfig` |
| `sdks/rust/crates/motosan-ai-oauth/src/providers/gemini.rs` | Create | `pub fn gemini() -> OAuthConfig` |
| `sdks/rust/crates/codex-oauth/src/lib.rs` | Modify | Replace with thin wrapper over `motosan-ai-oauth` |
| `sdks/rust/crates/codex-oauth/Cargo.toml` | Modify | Add `motosan-ai-oauth` dependency with feature `codex` |
| `Cargo.toml` (workspace root) | Modify | Add `sdks/rust/crates/motosan-ai-oauth` to members |

---

## Task 1: Crate Skeleton + Workspace Registration

**Files:**
- Create: `sdks/rust/crates/motosan-ai-oauth/Cargo.toml`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Create the Cargo.toml for motosan-ai-oauth**

```toml
# sdks/rust/crates/motosan-ai-oauth/Cargo.toml
[package]
name = "motosan-ai-oauth"
version = "0.1.0"
edition = "2021"
rust-version = "1.82"
license = "MIT"
description = "Provider-agnostic PKCE OAuth login and token refresh"
repository = "https://github.com/motosan-dev/motosan-ai"
homepage = "https://github.com/motosan-dev/motosan-ai"
documentation = "https://docs.rs/motosan-ai-oauth"
keywords = ["oauth", "pkce", "authentication", "gemini", "openai"]
categories = ["authentication", "api-bindings"]

[features]
default = []
codex  = []
gemini = []

[dependencies]
thiserror = "2"
serde = { version = "1", features = ["derive"] }
base64 = "0.22"
sha2 = "0.10"
rand = "0.9"
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
percent-encoding = "2"
tokio = { version = "1", features = [
  "net",
  "io-util",
  "rt-multi-thread",
  "macros",
  "time",
] }

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt"] }
```

- [ ] **Step 2: Create src directory placeholder**

```bash
mkdir -p sdks/rust/crates/motosan-ai-oauth/src/providers
touch sdks/rust/crates/motosan-ai-oauth/src/lib.rs
```

- [ ] **Step 3: Register in workspace**

Edit `Cargo.toml` (root):

```toml
[workspace]
members = [
  "sdks/rust",
  "sdks/rust/crates/codex-oauth",
  "sdks/rust/crates/motosan-ai-oauth",
]
resolver = "2"
```

- [ ] **Step 4: Verify workspace resolves**

```bash
cargo metadata --no-deps --manifest-path Cargo.toml | grep motosan-ai-oauth
```

Expected: line containing `"motosan-ai-oauth"`

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml sdks/rust/crates/motosan-ai-oauth/
git commit -m "chore: scaffold motosan-ai-oauth crate"
```

---

## Task 2: error.rs + pkce.rs (direct copy)

**Files:**
- Create: `sdks/rust/crates/motosan-ai-oauth/src/error.rs`
- Create: `sdks/rust/crates/motosan-ai-oauth/src/pkce.rs`

- [ ] **Step 1: Write error.rs**

```rust
// sdks/rust/crates/motosan-ai-oauth/src/error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Callback error: {0}")]
    Callback(String),

    #[error("State mismatch (possible CSRF): received unexpected state value")]
    StateMismatch,

    #[error("Token exchange failed: {0}")]
    TokenExchange(String),
}
```

- [ ] **Step 2: Write pkce.rs (verbatim from codex-oauth)**

```rust
// sdks/rust/crates/motosan-ai-oauth/src/pkce.rs
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore as _;
use sha2::{Digest, Sha256};

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 64];
        rand::rng().fill_bytes(&mut bytes);
        let verifier = URL_SAFE_NO_PAD.encode(bytes);

        let hash = Sha256::digest(verifier.as_bytes());
        let challenge = URL_SAFE_NO_PAD.encode(hash);

        Pkce { verifier, challenge }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifier_is_base64url_no_pad() {
        let pkce = Pkce::generate();
        assert!(
            pkce.verifier
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "verifier contains non-base64url chars: {}",
            pkce.verifier
        );
        assert!(!pkce.verifier.contains('='), "verifier must not have padding");
        assert_eq!(pkce.verifier.len(), 86);
    }

    #[test]
    fn challenge_matches_s256_of_verifier() {
        let pkce = Pkce::generate();
        let hash = Sha256::digest(pkce.verifier.as_bytes());
        let expected = URL_SAFE_NO_PAD.encode(hash);
        assert_eq!(pkce.challenge, expected);
    }

    #[test]
    fn challenge_is_base64url_no_pad() {
        let pkce = Pkce::generate();
        assert!(pkce.challenge.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert!(!pkce.challenge.contains('='));
    }

    #[test]
    fn each_generate_is_unique() {
        let a = Pkce::generate();
        let b = Pkce::generate();
        assert_ne!(a.verifier, b.verifier);
    }
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test -p motosan-ai-oauth
```

Expected: 4 tests pass (`verifier_is_base64url_no_pad`, `challenge_matches_s256_of_verifier`, `challenge_is_base64url_no_pad`, `each_generate_is_unique`)

- [ ] **Step 4: Commit**

```bash
git add sdks/rust/crates/motosan-ai-oauth/src/error.rs \
        sdks/rust/crates/motosan-ai-oauth/src/pkce.rs
git commit -m "feat(motosan-ai-oauth): add error and pkce modules"
```

---

## Task 3: server.rs (refactored for dynamic port)

**Files:**
- Create: `sdks/rust/crates/motosan-ai-oauth/src/server.rs`

The key difference from `codex-oauth`: the server now binds first (returning `BoundServer` with the actual port), so `login()` can build the `redirect_uri` with the real port before opening the browser.

- [ ] **Step 1: Write failing test**

Add to bottom of `sdks/rust/crates/motosan-ai-oauth/src/server.rs`:

```rust
// sdks/rust/crates/motosan-ai-oauth/src/server.rs
use percent_encoding::percent_decode_str;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use crate::error::Error;

pub struct BoundServer {
    pub port: u16,
    listener: TcpListener,
}

pub async fn bind(port: Option<u16>) -> Result<BoundServer, Error> {
    todo!()
}

pub async fn wait_for_callback(server: BoundServer) -> Result<(String, String), Error> {
    todo!()
}

fn is_callback_request(request: &str) -> bool {
    let path = request
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .unwrap_or("");
    path.starts_with("/auth/callback") && path.contains("code=")
}

fn parse_callback(request: &str) -> Result<(String, String), Error> {
    let first_line = request.lines().next().unwrap_or("");
    let path = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| Error::Callback("malformed HTTP request".into()))?;

    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");

    let mut code = None;
    let mut state = None;

    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            let decoded = percent_decode_str(v)
                .decode_utf8()
                .map_err(|_| Error::Callback(format!("param '{k}' is not valid UTF-8")))?
                .into_owned();
            match k {
                "code" => code = Some(decoded),
                "state" => state = Some(decoded),
                _ => {}
            }
        }
    }

    let code = code.ok_or_else(|| Error::Callback("missing code param".into()))?;
    let state = state.ok_or_else(|| Error::Callback("missing state param".into()))?;
    Ok((code, state))
}

async fn read_headers(stream: &mut tokio::net::TcpStream) -> Result<Vec<u8>, Error> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 512];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 { break; }
        if buf.len() + n >= 16384 {
            return Err(Error::Callback("request too large".into()));
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") { break; }
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(path: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n")
    }

    #[test]
    fn parses_normal_callback() {
        let (code, state) = parse_callback(&get("/auth/callback?code=abc123&state=xyz")).unwrap();
        assert_eq!(code, "abc123");
        assert_eq!(state, "xyz");
    }

    #[test]
    fn decodes_percent_encoded_params() {
        let (code, state) =
            parse_callback(&get("/auth/callback?code=ab%2Bcd&state=x%3Dy")).unwrap();
        assert_eq!(code, "ab+cd");
        assert_eq!(state, "x=y");
    }

    #[test]
    fn extra_params_are_ignored() {
        let (code, state) =
            parse_callback(&get("/auth/callback?code=c&state=s&session_state=ignored")).unwrap();
        assert_eq!(code, "c");
        assert_eq!(state, "s");
    }

    #[test]
    fn missing_code_returns_error() {
        let err = parse_callback(&get("/auth/callback?state=s")).unwrap_err();
        assert!(err.to_string().contains("missing code"));
    }

    #[test]
    fn missing_state_returns_error() {
        let err = parse_callback(&get("/auth/callback?code=c")).unwrap_err();
        assert!(err.to_string().contains("missing state"));
    }

    #[test]
    fn non_callback_path_is_not_callback() {
        assert!(!is_callback_request(&get("/favicon.ico")));
        assert!(!is_callback_request(&get("/")));
    }

    #[test]
    fn callback_path_with_code_is_callback() {
        assert!(is_callback_request(&get("/auth/callback?code=abc&state=xyz")));
    }

    #[tokio::test]
    async fn bind_dynamic_port_returns_nonzero_port() {
        let server = bind(None).await.expect("bind should succeed");
        assert!(server.port > 0, "dynamic port should be nonzero");
    }

    #[tokio::test]
    async fn bind_specific_port_returns_that_port() {
        // Use port 0 first to find a free port, then test binding to it explicitly
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let free_port = probe.local_addr().unwrap().port();
        drop(probe);

        let server = bind(Some(free_port)).await.expect("bind should succeed");
        assert_eq!(server.port, free_port);
    }
}
```

- [ ] **Step 2: Run tests — expect compile error (todo!)**

```bash
cargo test -p motosan-ai-oauth server 2>&1 | head -20
```

Expected: compile error or `todo!()` panic

- [ ] **Step 3: Implement `bind` and `wait_for_callback`**

Replace the two `todo!()` stubs:

```rust
pub async fn bind(port: Option<u16>) -> Result<BoundServer, Error> {
    let addr = format!("127.0.0.1:{}", port.unwrap_or(0));
    let listener = TcpListener::bind(&addr).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            Error::Callback(format!(
                "port {} is already in use; close other instances and retry",
                port.unwrap_or(0)
            ))
        } else {
            Error::Io(e)
        }
    })?;
    let actual_port = listener.local_addr().map_err(Error::Io)?.port();
    Ok(BoundServer { port: actual_port, listener })
}

pub async fn wait_for_callback(server: BoundServer) -> Result<(String, String), Error> {
    let BoundServer { listener, .. } = server;
    loop {
        let (mut stream, _) = listener.accept().await?;
        let buf = read_headers(&mut stream).await?;
        let request = String::from_utf8_lossy(&buf);

        if !is_callback_request(&request) {
            let _ = stream
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .await;
            continue;
        }

        let html = "<html><body>Login successful. You can close this tab.</body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            html.len(),
            html
        );
        let _ = stream.write_all(response.as_bytes()).await;
        return parse_callback(&request);
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p motosan-ai-oauth server
```

Expected: all 9 tests pass

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/crates/motosan-ai-oauth/src/server.rs
git commit -m "feat(motosan-ai-oauth): add server module with dynamic port binding"
```

---

## Task 4: exchange.rs (generalized)

**Files:**
- Create: `sdks/rust/crates/motosan-ai-oauth/src/exchange.rs`

The difference from `codex-oauth/exchange.rs`: takes `&OAuthConfig` and `redirect_uri: &str` instead of hardcoded constants. Adds `client_secret` to POST body when present.

- [ ] **Step 1: Write exchange.rs**

```rust
// sdks/rust/crates/motosan-ai-oauth/src/exchange.rs
use serde::Deserialize;

use crate::{error::Error, unix_now, OAuthConfig, Token};

#[derive(Deserialize)]
struct RawTokenResponse {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    id_token: Option<String>,
    expires_in: u64,
}

impl RawTokenResponse {
    fn into_token(self) -> Token {
        Token {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            id_token: self.id_token,
            expires_in: self.expires_in,
            issued_at: unix_now(),
        }
    }
}

async fn post_token(token_url: &str, params: Vec<(&str, &str)>) -> Result<Token, Error> {
    let client = reqwest::Client::new();
    let resp = client
        .post(token_url)
        .header("User-Agent", "Mozilla/5.0 (compatible; motosan-ai-oauth)")
        .form(&params)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(Error::TokenExchange(format!("HTTP {status}: {body}")));
    }

    Ok(resp.json::<RawTokenResponse>().await?.into_token())
}

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
    post_token(config.token_url, params).await
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
    post_token(config.token_url, params).await
}
```

- [ ] **Step 2: Run tests (compile check only — no unit tests for exchange, live tests are #[ignore])**

```bash
cargo build -p motosan-ai-oauth 2>&1
```

Expected: compile error because `lib.rs` stubs don't define `OAuthConfig` and `Token` yet. That's fine — proceed to Task 5 which defines them.

---

## Task 5: lib.rs — OAuthConfig, Token, login(), refresh()

**Files:**
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/lib.rs`

- [ ] **Step 1: Write the failing tests first**

```rust
// sdks/rust/crates/motosan-ai-oauth/src/lib.rs
mod error;
mod exchange;
mod pkce;
mod server;
pub mod providers;

pub use error::Error;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore as _;

const LOGIN_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub client_id: &'static str,
    pub client_secret: Option<&'static str>,
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub scopes: &'static [&'static str],
    pub redirect_port: Option<u16>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: Option<String>,
    pub expires_in: u64,
    pub issued_at: u64,
}

impl Token {
    pub fn is_expired(&self) -> bool {
        unix_now() >= self.issued_at + self.expires_in
    }
}

pub async fn login(config: &OAuthConfig) -> Result<Token, Error> {
    let pkce = pkce::Pkce::generate();

    let mut state_bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut state_bytes);
    let state = URL_SAFE_NO_PAD.encode(state_bytes);

    let server = server::bind(config.redirect_port).await?;
    let redirect_uri = format!("http://127.0.0.1:{}/auth/callback", server.port);

    let auth_url = build_auth_url(config, &pkce.challenge, &state, &redirect_uri);

    println!("Open this URL to log in:\n\n  {auth_url}\n");
    let _ = open_browser(&auth_url);

    let (code, returned_state) = tokio::time::timeout(
        std::time::Duration::from_secs(LOGIN_TIMEOUT_SECS),
        server::wait_for_callback(server),
    )
    .await
    .map_err(|_| {
        Error::Callback(format!(
            "timed out waiting for browser callback ({LOGIN_TIMEOUT_SECS}s)"
        ))
    })??;

    if returned_state != state {
        return Err(Error::StateMismatch);
    }

    exchange::exchange_code(config, &code, &pkce.verifier, &redirect_uri).await
}

pub async fn refresh(config: &OAuthConfig, refresh_token: &str) -> Result<Token, Error> {
    exchange::refresh_token(config, refresh_token).await
}

pub(crate) fn build_auth_url(
    config: &OAuthConfig,
    challenge: &str,
    state: &str,
    redirect_uri: &str,
) -> String {
    let mut url = reqwest::Url::parse(config.auth_url).expect("auth_url must be valid");
    url.query_pairs_mut()
        .append_pair("client_id", config.client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", &config.scopes.join(" "))
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("access_type", "offline");
    url.to_string()
}

pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(url).spawn()?;
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open").arg(url).spawn()?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("cmd")
        .args(["/c", "start", "", url])
        .spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_config() -> OAuthConfig {
        OAuthConfig {
            client_id: "test-client",
            client_secret: None,
            auth_url: "https://auth.example.com/oauth/authorize",
            token_url: "https://auth.example.com/oauth/token",
            scopes: &["openid", "profile"],
            redirect_port: None,
        }
    }

    #[test]
    fn build_auth_url_contains_required_params() {
        let config = dummy_config();
        let url = build_auth_url(&config, "challenge123", "state456", "http://127.0.0.1:9999/auth/callback");
        assert!(url.contains("client_id=test-client"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge=challenge123"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=state456"));
        assert!(url.contains("redirect_uri="));
        assert!(url.contains("scope="));
    }

    #[test]
    fn build_auth_url_includes_scopes_joined() {
        let config = dummy_config();
        let url = build_auth_url(&config, "c", "s", "http://127.0.0.1:1234/auth/callback");
        assert!(url.contains("scope=openid+profile") || url.contains("scope=openid%20profile"));
    }

    #[test]
    fn build_auth_url_parses_as_valid_url() {
        let config = dummy_config();
        let url = build_auth_url(&config, "c", "s", "http://127.0.0.1:1234/auth/callback");
        reqwest::Url::parse(&url).expect("auth URL must be valid");
    }

    #[test]
    fn token_is_expired_when_issued_at_zero() {
        let token = Token {
            access_token: "a".into(),
            refresh_token: "r".into(),
            id_token: None,
            expires_in: 3600,
            issued_at: 0,
        };
        assert!(token.is_expired());
    }

    #[test]
    fn token_not_expired_when_just_issued() {
        let token = Token {
            access_token: "a".into(),
            refresh_token: "r".into(),
            id_token: None,
            expires_in: 3600,
            issued_at: unix_now(),
        };
        assert!(!token.is_expired());
    }
}
```

- [ ] **Step 2: Write providers/mod.rs stub so it compiles**

```rust
// sdks/rust/crates/motosan-ai-oauth/src/providers/mod.rs
#[cfg(feature = "codex")]
pub mod codex;

#[cfg(feature = "gemini")]
pub mod gemini;
```

Also create empty files so the module tree resolves:

```bash
touch sdks/rust/crates/motosan-ai-oauth/src/providers/codex.rs
touch sdks/rust/crates/motosan-ai-oauth/src/providers/gemini.rs
```

- [ ] **Step 3: Run tests**

```bash
cargo test -p motosan-ai-oauth
```

Expected: 5 new lib tests pass + 4 pkce tests + 9 server tests = 18 total

- [ ] **Step 4: Commit**

```bash
git add sdks/rust/crates/motosan-ai-oauth/src/
git commit -m "feat(motosan-ai-oauth): add OAuthConfig, Token, login(), refresh()"
```

---

## Task 6: providers/codex.rs

**Files:**
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/providers/codex.rs`

- [ ] **Step 1: Write failing test**

```rust
// sdks/rust/crates/motosan-ai-oauth/src/providers/codex.rs
use crate::OAuthConfig;

pub fn codex() -> OAuthConfig {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_config_has_correct_client_id() {
        let c = codex();
        assert_eq!(c.client_id, "app_EMoamEEZ73f0CkXaXp7hrann");
    }

    #[test]
    fn codex_config_has_no_client_secret() {
        let c = codex();
        assert!(c.client_secret.is_none());
    }

    #[test]
    fn codex_config_redirect_port_is_1455() {
        let c = codex();
        assert_eq!(c.redirect_port, Some(1455));
    }

    #[test]
    fn codex_config_auth_url_is_openai() {
        let c = codex();
        assert!(c.auth_url.contains("auth.openai.com"));
    }
}
```

- [ ] **Step 2: Run test to verify failure**

```bash
cargo test -p motosan-ai-oauth --features codex providers::codex 2>&1 | tail -5
```

Expected: panics with `not yet implemented`

- [ ] **Step 3: Implement**

```rust
use crate::OAuthConfig;

pub fn codex() -> OAuthConfig {
    OAuthConfig {
        client_id: "app_EMoamEEZ73f0CkXaXp7hrann",
        client_secret: None,
        auth_url: "https://auth.openai.com/oauth/authorize",
        token_url: "https://auth.openai.com/oauth/token",
        scopes: &["openid", "profile", "email", "offline_access"],
        redirect_port: Some(1455),
    }
}

#[cfg(test)]
mod tests { /* same as above */ }
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p motosan-ai-oauth --features codex
```

Expected: all tests pass including 4 new codex tests

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/crates/motosan-ai-oauth/src/providers/codex.rs
git commit -m "feat(motosan-ai-oauth): add codex provider config"
```

---

## Task 7: providers/gemini.rs

**Files:**
- Modify: `sdks/rust/crates/motosan-ai-oauth/src/providers/gemini.rs`

- [ ] **Step 1: Write failing test**

```rust
// sdks/rust/crates/motosan-ai-oauth/src/providers/gemini.rs
use crate::OAuthConfig;

pub fn gemini() -> OAuthConfig {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_config_has_google_client_id() {
        let c = gemini();
        assert!(c.client_id.contains("681255809395"));
    }

    #[test]
    fn gemini_config_has_client_secret() {
        let c = gemini();
        assert!(c.client_secret.is_some());
    }

    #[test]
    fn gemini_config_redirect_port_is_dynamic() {
        let c = gemini();
        assert!(c.redirect_port.is_none());
    }

    #[test]
    fn gemini_config_auth_url_is_google() {
        let c = gemini();
        assert!(c.auth_url.contains("accounts.google.com"));
    }

    #[test]
    fn gemini_config_scopes_include_cloud_platform() {
        let c = gemini();
        assert!(c.scopes.iter().any(|s| s.contains("cloud-platform")));
    }
}
```

- [ ] **Step 2: Run test to verify failure**

```bash
cargo test -p motosan-ai-oauth --features gemini providers::gemini 2>&1 | tail -5
```

Expected: panics with `not yet implemented`

- [ ] **Step 3: Implement**

```rust
use crate::OAuthConfig;

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
    }
}

#[cfg(test)]
mod tests { /* same as above */ }
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p motosan-ai-oauth --features gemini
```

Expected: all tests pass including 5 new gemini tests

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/crates/motosan-ai-oauth/src/providers/gemini.rs
git commit -m "feat(motosan-ai-oauth): add gemini provider config"
```

---

## Task 8: Update codex-oauth as thin wrapper

**Files:**
- Modify: `sdks/rust/crates/codex-oauth/Cargo.toml`
- Modify: `sdks/rust/crates/codex-oauth/src/lib.rs`

- [ ] **Step 1: Update codex-oauth/Cargo.toml to depend on motosan-ai-oauth**

```toml
[package]
name = "codex-oauth"
version = "0.1.0"
edition = "2021"
rust-version = "1.82"
license = "MIT"
description = "OAuth login for OpenAI Codex (ChatGPT account)"
repository = "https://github.com/motosan-dev/motosan-ai"
homepage = "https://github.com/motosan-dev/motosan-ai"
documentation = "https://docs.rs/codex-oauth"
keywords = ["openai", "codex", "oauth", "authentication"]
categories = ["authentication", "api-bindings"]

[dependencies]
motosan-ai-oauth = { path = "../motosan-ai-oauth", features = ["codex"] }

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt"] }
```

- [ ] **Step 2: Replace codex-oauth/src/lib.rs with the thin wrapper**

```rust
//! PKCE OAuth login for OpenAI Codex.
//!
//! Thin wrapper around [`motosan_ai_oauth`] with the Codex provider pre-configured.
//!
//! # Usage
//!
//! ```no_run
//! #[tokio::main]
//! async fn main() -> Result<(), codex_oauth::Error> {
//!     let token = codex_oauth::login().await?;
//!     println!("{}", token.access_token);
//!     Ok(())
//! }
//! ```
//!
//! # Notes
//!
//! - Requires port 1455 to be free on localhost (hardcoded by OpenAI's app registration).
//! - `CLIENT_ID` is hardcoded to OpenAI's public Codex app registration and is not configurable.

pub use motosan_ai_oauth::{Error, Token};

/// Open a browser-based PKCE login flow and return the resulting OAuth token.
pub async fn login() -> Result<Token, Error> {
    motosan_ai_oauth::login(&motosan_ai_oauth::providers::codex::codex()).await
}

/// Exchange a stored refresh token for a new [`Token`].
pub async fn refresh(refresh_token: &str) -> Result<Token, Error> {
    motosan_ai_oauth::refresh(
        &motosan_ai_oauth::providers::codex::codex(),
        refresh_token,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn re_exports_compile() {
        // Verify the public surface compiles: Token and Error are accessible.
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

- [ ] **Step 3: Run codex-oauth tests**

```bash
cargo test -p codex-oauth
```

Expected: `re_exports_compile` passes; all previously-passing tests still pass

- [ ] **Step 4: Run full workspace check**

```bash
cargo check --workspace
```

Expected: no errors

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/crates/codex-oauth/
git commit -m "refactor(codex-oauth): make thin wrapper over motosan-ai-oauth"
```

---

## Task 9: Final verification

- [ ] **Step 1: Run all tests with both features enabled**

```bash
cargo test -p motosan-ai-oauth --features codex,gemini
```

Expected: all unit tests pass (pkce ×4, server ×9, lib ×5, codex ×4, gemini ×5 = 27 total)

- [ ] **Step 2: Run full workspace test suite**

```bash
cargo test --workspace
```

Expected: no regressions in `codex-oauth`, `motosan-ai`, or any other crate

- [ ] **Step 3: Check with no default features**

```bash
cargo check -p motosan-ai-oauth
cargo check -p motosan-ai-oauth --features codex
cargo check -p motosan-ai-oauth --features gemini
cargo check -p motosan-ai-oauth --features codex,gemini
```

Expected: all four compile cleanly

- [ ] **Step 4: Final commit**

```bash
git add -u
git commit -m "feat: motosan-ai-oauth v0.1.0 — generic PKCE OAuth with Codex + Gemini providers"
```
