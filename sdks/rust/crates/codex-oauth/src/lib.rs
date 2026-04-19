//! PKCE OAuth login for OpenAI Codex.
//!
//! Guides the user through a browser-based login against `auth.openai.com`
//! and returns a [`Token`] that can be used as a Bearer token with the
//! ChatGPT backend API (`https://chatgpt.com/backend-api`).
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

mod error;
mod exchange;
mod pkce;
mod server;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
pub use error::Error;
use rand::RngCore as _;

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
pub(crate) const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const CALLBACK_PORT: u16 = 1455;
const LOGIN_TIMEOUT_SECS: u64 = 120;

/// OAuth token returned after a successful login or refresh.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    /// Lifetime in seconds as reported by the server.
    pub expires_in: u64,
    /// Unix timestamp (seconds since epoch) when the token was issued.
    /// Use with `expires_in` to determine whether the token is still valid.
    pub issued_at: u64,
}

impl Token {
    /// Returns `true` if the token has passed its expiry time.
    pub fn is_expired(&self) -> bool {
        unix_now() >= self.issued_at + self.expires_in
    }
}

/// Open a browser-based PKCE login flow and return the resulting OAuth token.
///
/// Prints the authorization URL to stdout so the user can open it manually
/// if the browser does not open automatically. Times out after 120 seconds.
pub async fn login() -> Result<Token, Error> {
    let pkce = pkce::Pkce::generate();

    let mut state_bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut state_bytes);
    let state = URL_SAFE_NO_PAD.encode(state_bytes);

    let auth_url = build_auth_url(&pkce.challenge, &state);

    println!("Open this URL to log in:\n\n  {auth_url}\n");
    let _ = open_browser(&auth_url);

    let (code, returned_state) = tokio::time::timeout(
        std::time::Duration::from_secs(LOGIN_TIMEOUT_SECS),
        server::wait_for_callback(CALLBACK_PORT),
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

    exchange::exchange_code(&code, &pkce.verifier).await
}

/// Exchange a stored refresh token for a new [`Token`].
pub async fn refresh(refresh_token: &str) -> Result<Token, Error> {
    exchange::refresh_token(refresh_token).await
}

pub(crate) fn build_auth_url(challenge: &str, state: &str) -> String {
    let mut url = reqwest::Url::parse(AUTH_URL).expect("AUTH_URL is valid");
    url.query_pairs_mut()
        .append_pair("client_id", CLIENT_ID)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("scope", "openid profile email offline_access")
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
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
    // Wrap URL in quotes to prevent cmd.exe from interpreting metacharacters.
    std::process::Command::new("cmd")
        .args(["/c", "start", "", url])
        .spawn()?;
    // On unsupported platforms the user opens the printed URL manually.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_url_contains_required_params() {
        let url = build_auth_url("challenge123", "state456");
        assert!(url.contains("client_id=app_EMoamEEZ73f0CkXaXp7hrann"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge=challenge123"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=state456"));
        assert!(url.contains("scope="));
    }

    #[test]
    fn redirect_uri_is_percent_encoded_in_auth_url() {
        let url = build_auth_url("c", "s");
        assert!(url.contains("redirect_uri=http%3A%2F%2F"));
    }

    #[test]
    fn auth_url_parses_as_valid_url() {
        let url = build_auth_url("challenge", "state");
        reqwest::Url::parse(&url).expect("auth URL must be valid");
    }

    #[test]
    fn token_expiry_detection() {
        let expired = Token {
            access_token: "a".into(),
            refresh_token: "r".into(),
            id_token: "i".into(),
            expires_in: 3600,
            issued_at: 0,
        };
        assert!(expired.is_expired());

        let valid = Token {
            issued_at: unix_now(),
            expires_in: 3600,
            ..expired.clone()
        };
        assert!(!valid.is_expired());
    }
}
