//! Credential seams for providers that authenticate with short-lived tokens.
//!
//! [`TokenSource`] decouples *how* a bearer token is obtained (static string,
//! OAuth refresh flow, keychain, ...) from the provider that spends it. The
//! provider asks the source for a token at the top of **every** HTTP attempt
//! (including retries), so a refreshing source can hand out a new token
//! mid-retry-loop.
//!
//! # Security
//!
//! Tokens are credentials: implementations MUST NOT log, `Debug`-print, or
//! otherwise persist the strings they return. `TokenSource` requires
//! [`std::fmt::Debug`] so providers embedding a source stay debuggable, but
//! your `Debug` impl must redact all token material (as
//! [`StaticTokenSource`]'s does).

use crate::error::MotosanError;

pub use async_trait::async_trait;

/// Async source of bearer access tokens.
///
/// Implementations must be cheap to call repeatedly: providers call
/// [`access_token`](TokenSource::access_token) once per HTTP attempt.
#[async_trait]
pub trait TokenSource: Send + Sync + std::fmt::Debug {
    /// Return the bearer token to use for the next HTTP attempt.
    ///
    /// Never log the returned value.
    async fn access_token(&self) -> Result<String, MotosanError>;
}

/// A [`TokenSource`] that always returns the same fixed token.
///
/// This is what `ChatGptCodexProvider::new` wraps its `access_token`
/// argument in. Its `Debug` impl redacts the token.
pub struct StaticTokenSource(String);

impl StaticTokenSource {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }
}

impl std::fmt::Debug for StaticTokenSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("StaticTokenSource")
            .field(&"<redacted>")
            .finish()
    }
}

#[async_trait]
impl TokenSource for StaticTokenSource {
    async fn access_token(&self) -> Result<String, MotosanError> {
        Ok(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{StaticTokenSource, TokenSource};

    #[tokio::test]
    async fn static_source_returns_the_string() {
        let source = StaticTokenSource::new("tok-abc");
        assert_eq!(source.access_token().await.unwrap(), "tok-abc");
    }

    #[test]
    fn static_source_debug_redacts_the_token() {
        let source = StaticTokenSource::new("super-secret");
        let debug = format!("{source:?}");
        assert!(!debug.contains("super-secret"), "Debug leaked: {debug}");
        assert!(debug.contains("StaticTokenSource"));
    }
}
