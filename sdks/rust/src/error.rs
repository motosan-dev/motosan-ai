use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum MotosanError {
    #[error("auth error: {0}")]
    Auth(String),
    #[error("rate limit error: {0}")]
    RateLimit(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("config error: {0}")]
    Config(String),
    #[error("provider error: {0}")]
    ProviderError(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("stream error: {0}")]
    Stream(String),
    #[error("unsupported feature: {0}")]
    UnsupportedFeature(String),
}
