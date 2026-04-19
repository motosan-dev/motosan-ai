# codex-oauth

PKCE OAuth login for OpenAI Codex — obtains an access token via a browser-based login against `auth.openai.com`, usable as a Bearer token with the ChatGPT backend API.

- crates.io: https://crates.io/crates/codex-oauth
- GitHub: https://github.com/motosan-dev/motosan-ai

## Install

```toml
[dependencies]
codex-oauth = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

## Usage

### Login

```rust
#[tokio::main]
async fn main() -> Result<(), codex_oauth::Error> {
    let token = codex_oauth::login().await?;
    println!("{}", token.access_token);
    Ok(())
}
```

`login()` opens the browser to `auth.openai.com` (and prints the URL for manual opening) then listens on `http://localhost:1455/auth/callback` for the redirect. Times out after 120 seconds.

### Refresh

```rust
let new_token = codex_oauth::refresh(&token.refresh_token).await?;
```

### Expiry check

```rust
if token.is_expired() {
    let token = codex_oauth::refresh(&token.refresh_token).await?;
}
```

### Persist to disk

`Token` implements `serde::Serialize` / `Deserialize`:

```rust
let json = serde_json::to_string(&token)?;
// later:
let token: codex_oauth::Token = serde_json::from_str(&json)?;
```

### Use with OpenAI provider (motosan-ai)

```rust
let token = codex_oauth::login().await?;

let client = motosan_ai::Client::builder()
    .provider(motosan_ai::Provider::OpenAI)
    .api_key(token.access_token)
    .base_url("https://chatgpt.com/backend-api")
    .build()?;
```

## Notes

- Port `1455` must be free — it is hardcoded by OpenAI's app registration.
- `CLIENT_ID` is OpenAI's public Codex app registration (`app_EMoamEEZ73f0CkXaXp7hrann`) and is not configurable.
- Requires a ChatGPT account.
