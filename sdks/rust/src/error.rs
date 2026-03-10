use std::fmt::{Display, Formatter};

#[derive(Debug, Clone)]
pub enum MotosanError {
    Config(String),
    ProviderError(String),
}

impl Display for MotosanError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(message) => write!(f, "config error: {message}"),
            Self::ProviderError(message) => write!(f, "provider error: {message}"),
        }
    }
}

impl std::error::Error for MotosanError {}

