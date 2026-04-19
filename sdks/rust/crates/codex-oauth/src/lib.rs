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
    motosan_ai_oauth::refresh(&motosan_ai_oauth::providers::codex::codex(), refresh_token).await
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
