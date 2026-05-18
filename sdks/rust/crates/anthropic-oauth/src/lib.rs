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
