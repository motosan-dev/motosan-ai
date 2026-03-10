use std::fmt::{Display, Formatter};

#[derive(Debug, Clone)]
pub enum MotosanError {
    Auth(String),
    RateLimit(String),
    InvalidRequest(String),
    Config(String),
    ProviderError(String),
    Network(String),
    Stream(String),
}

impl Display for MotosanError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auth(message) => write!(f, "auth error: {message}"),
            Self::RateLimit(message) => write!(f, "rate limit error: {message}"),
            Self::InvalidRequest(message) => write!(f, "invalid request: {message}"),
            Self::Config(message) => write!(f, "config error: {message}"),
            Self::ProviderError(message) => write!(f, "provider error: {message}"),
            Self::Network(message) => write!(f, "network error: {message}"),
            Self::Stream(message) => write!(f, "stream error: {message}"),
        }
    }
}

impl std::error::Error for MotosanError {}
