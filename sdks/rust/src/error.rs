use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum MotosanError {
    #[error("auth error: {message}")]
    Auth {
        message: String,
        status_code: Option<u16>,
        retry_after: Option<Duration>,
        request_id: Option<String>,
    },
    #[error("rate limit error: {message}")]
    RateLimit {
        message: String,
        status_code: Option<u16>,
        retry_after: Option<Duration>,
        request_id: Option<String>,
    },
    #[error("invalid request: {message}")]
    InvalidRequest {
        message: String,
        status_code: Option<u16>,
        retry_after: Option<Duration>,
        request_id: Option<String>,
    },
    #[error("config error: {0}")]
    Config(String),
    #[error("provider error: {message}")]
    ProviderError {
        message: String,
        status_code: Option<u16>,
        retry_after: Option<Duration>,
        request_id: Option<String>,
    },
    #[error("network error: {0}")]
    Network(String),
    #[error("stream error: {0}")]
    Stream(String),
    #[error("stream read timeout: no data received within {0} seconds")]
    StreamReadTimeout(u64),
    #[error("unsupported feature: {0}")]
    UnsupportedFeature(String),
}

impl MotosanError {
    /// HTTP status that produced this error, when it came from an HTTP response.
    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::Auth { status_code, .. }
            | Self::RateLimit { status_code, .. }
            | Self::InvalidRequest { status_code, .. }
            | Self::ProviderError { status_code, .. } => *status_code,
            _ => None,
        }
    }

    /// Parsed `Retry-After` from the failing HTTP response, if present.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Auth { retry_after, .. }
            | Self::RateLimit { retry_after, .. }
            | Self::InvalidRequest { retry_after, .. }
            | Self::ProviderError { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// Provider request id (`request-id` / `x-request-id` header), if present.
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Auth { request_id, .. }
            | Self::RateLimit { request_id, .. }
            | Self::InvalidRequest { request_id, .. }
            | Self::ProviderError { request_id, .. } => request_id.as_deref(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn display_strings_unchanged() {
        let cases: Vec<(MotosanError, &str)> = vec![
            (
                MotosanError::Auth {
                    message: "bad key".into(),
                    status_code: None,
                    retry_after: None,
                    request_id: None,
                },
                "auth error: bad key",
            ),
            (
                MotosanError::RateLimit {
                    message: "too many".into(),
                    status_code: None,
                    retry_after: None,
                    request_id: None,
                },
                "rate limit error: too many",
            ),
            (
                MotosanError::InvalidRequest {
                    message: "bad field".into(),
                    status_code: None,
                    retry_after: None,
                    request_id: None,
                },
                "invalid request: bad field",
            ),
            (
                MotosanError::ProviderError {
                    message: "boom".into(),
                    status_code: None,
                    retry_after: None,
                    request_id: None,
                },
                "provider error: boom",
            ),
            (MotosanError::Config("c".into()), "config error: c"),
            (MotosanError::Network("n".into()), "network error: n"),
            (MotosanError::Stream("s".into()), "stream error: s"),
            (
                MotosanError::StreamReadTimeout(5),
                "stream read timeout: no data received within 5 seconds",
            ),
            (
                MotosanError::UnsupportedFeature("u".into()),
                "unsupported feature: u",
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
        }
    }

    #[test]
    fn accessors_expose_metadata_on_http_variants() {
        let err = MotosanError::RateLimit {
            message: "too many".into(),
            status_code: Some(429),
            retry_after: Some(Duration::from_secs(7)),
            request_id: Some("req_123".into()),
        };
        assert_eq!(err.status_code(), Some(429));
        assert_eq!(err.retry_after(), Some(Duration::from_secs(7)));
        assert_eq!(err.request_id(), Some("req_123"));
    }

    #[test]
    fn accessors_return_none_on_non_http_variants() {
        let errors = [
            MotosanError::Config("c".into()),
            MotosanError::Network("n".into()),
            MotosanError::Stream("s".into()),
            MotosanError::StreamReadTimeout(5),
            MotosanError::UnsupportedFeature("u".into()),
        ];
        for err in errors {
            assert_eq!(err.status_code(), None);
            assert_eq!(err.retry_after(), None);
            assert_eq!(err.request_id(), None);
        }
    }
}
