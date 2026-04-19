# motosan-ai-oauth Design Spec

**Date:** 2026-04-20  
**Status:** Approved

## Goal

Generalize `codex-oauth` into a reusable `motosan-ai-oauth` crate that handles PKCE OAuth login and token refresh for any provider. Built-in Gemini and Codex configs ship behind Cargo feature flags. SDK consumers can pass their own `OAuthConfig` for custom providers without needing any feature.

## Scope

- PKCE S256 browser-based login flow
- Token refresh
- Built-in provider configs: `gemini`, `codex`
- **Out of scope:** token storage, token persistence, non-PKCE flows

## Crate Layout

`sdks/rust/crates/codex-oauth` is renamed to `sdks/rust/crates/motosan-ai-oauth`. The public API of `codex-oauth` is preserved via a thin re-export wrapper so existing callers are not broken.

```
sdks/rust/crates/motosan-ai-oauth/
  Cargo.toml
  src/
    lib.rs          # login(), refresh(), Token, OAuthConfig, re-exports
    pkce.rs         # verifier generation, S256 challenge (migrated from codex-oauth)
    server.rs       # local HTTP callback server (migrated from codex-oauth)
    exchange.rs     # code exchange + refresh HTTP POST (generalized from codex-oauth)
    error.rs        # Error enum (migrated from codex-oauth)
    providers/
      mod.rs        # pub mod codex, pub mod gemini (behind features)
      codex.rs      # #[cfg(feature = "codex")] — codex_config()
      gemini.rs     # #[cfg(feature = "gemini")] — gemini_config()
```

## Cargo Features

```toml
[features]
default = []
codex  = []   # enables providers::codex module
gemini = []   # enables providers::gemini module
```

No feature is required to use the crate with a custom `OAuthConfig`.

## Public API

### Types

```rust
pub struct OAuthConfig {
    pub client_id: &'static str,
    pub client_secret: Option<&'static str>, // installed app secret — safe to embed
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub scopes: &'static [&'static str],
    pub redirect_port: Option<u16>,          // None = OS dynamic port
}

pub struct Token {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: Option<String>,
    pub expires_in: u64,   // seconds
    pub issued_at: u64,    // unix seconds at time of issue
}

impl Token {
    pub fn is_expired(&self) -> bool;
}
```

### Entry Points

```rust
/// Open a browser-based PKCE login and return the resulting token.
/// Prints the auth URL to stdout if the browser cannot be opened automatically.
/// Times out after 120 seconds.
pub async fn login(config: &OAuthConfig) -> Result<Token, Error>;

/// Exchange a refresh token for a new Token.
pub async fn refresh(config: &OAuthConfig, refresh_token: &str) -> Result<Token, Error>;
```

### Built-in Provider Configs

```rust
// requires feature = "codex"
pub mod providers {
    pub fn codex() -> OAuthConfig;   // auth.openai.com, port 1455
    pub fn gemini() -> OAuthConfig;  // accounts.google.com, dynamic port
}
```

## Provider Credentials

### Codex (OpenAI)
- `client_id`: `app_EMoamEEZ73f0CkXaXp7hrann`
- `client_secret`: none (pure PKCE)
- `auth_url`: `https://auth.openai.com/oauth/authorize`
- `token_url`: `https://auth.openai.com/oauth/token`
- `scopes`: `["openid", "profile", "email", "offline_access"]`
- `redirect_port`: `Some(1455)` (hardcoded by OpenAI's app registration)

### Gemini (Google)
- `client_id`: `681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com`
- `client_secret`: `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` (installed app — safe to embed per Google's OAuth2 docs)
- `auth_url`: `https://accounts.google.com/o/oauth2/auth`
- `token_url`: `https://oauth2.googleapis.com/token`
- `scopes`: `["https://www.googleapis.com/auth/cloud-platform", "https://www.googleapis.com/auth/userinfo.email", "https://www.googleapis.com/auth/userinfo.profile"]`
- `redirect_port`: `None` (OS dynamic)

## Internal Flow

```
login(config)
  │
  ├── pkce::generate()          → (verifier, challenge)
  ├── random state bytes        → state
  ├── build auth URL            → open browser + print URL
  ├── server::wait_for_callback(port)  → (code, returned_state)
  ├── verify state == returned_state
  └── exchange::exchange_code(config, code, verifier) → Token

refresh(config, refresh_token)
  └── exchange::refresh_token(config, refresh_token) → Token
```

## Callback Server

- Binds to `127.0.0.1:{port}` where port is `config.redirect_port.unwrap_or(0)`
- Listens for one request to `/auth/callback?code=...&state=...`
- Returns a plain HTML success page, then shuts down
- Timeout: 120 seconds

## exchange.rs Changes vs codex-oauth

The existing `exchange.rs` only handles the Codex case (no client_secret, fixed URLs). The generalized version:
- Reads `auth_url`, `token_url`, `client_id` from `OAuthConfig`
- Adds `client_secret` to the POST body when `config.client_secret.is_some()`
- Otherwise identical

## Backward Compatibility: codex-oauth

`codex-oauth` crate stays in the workspace as a thin wrapper so existing dependents compile unchanged:

```rust
// codex-oauth/src/lib.rs
pub use motosan_oauth::{Error, Token};

pub async fn login() -> Result<Token, Error> {
    motosan_oauth::login(&motosan_oauth::providers::codex()).await
}

pub async fn refresh(refresh_token: &str) -> Result<Token, Error> {
    motosan_oauth::refresh(&motosan_oauth::providers::codex(), refresh_token).await
}
```

## Custom Provider Usage

No feature flag needed:

```rust
use motosan_oauth::{OAuthConfig, login};

let config = OAuthConfig {
    client_id: "my-client-id",
    client_secret: None,
    auth_url: "https://auth.openclaw.dev/oauth/authorize",
    token_url: "https://auth.openclaw.dev/oauth/token",
    scopes: &["read", "write"],
    redirect_port: Some(8080),
};
let token = login(&config).await?;
```

## Testing

- Unit tests: PKCE generation, S256 challenge correctness, `is_expired()`, `build_auth_url()` param checks
- Integration tests (`#[ignore]`): live login flow for `codex` and `gemini` providers, run manually
- `codex-oauth` wrapper tests: compile-time only, verify re-exports resolve
