use thiserror::Error;

#[derive(Debug, Error)]
pub enum MotosanError {
    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Rate limit exceeded (retry after {retry_after:?}s)")]
    RateLimit { retry_after: Option<u64> },

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Provider error {status}: {message}")]
    ProviderError { status: u16, message: String },

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Stream error: {0}")]
    Stream(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Configuration error: {0}")]
    Config(String),
}
