//! Shared HTTP transport helpers, compiled ONCE behind the private `_http`
//! umbrella feature (gated at the mod decl in transport/mod.rs — items in
//! this file carry NO feature cfg of their own). Moved verbatim from
//! providers/mod.rs in M4 Task 1; provider files keep their old
//! `crate::providers::*` import paths via the pub(crate) re-export there.

use crate::error::MotosanError;
use crate::retry::{RetryCause, RetryEvent, RetryPolicy};
use crate::stream::BoxStream;
use crate::types::{ChatResponse, StopReason, ToolCall, Usage};
use reqwest::header::HeaderMap;
use serde_json::Value;
use std::time::Duration;

pub(crate) struct ChatResponseBuilder {
    content: String,
    thinking: Option<String>,
    tool_calls: Vec<ToolCall>,
    model: String,
    usage: Usage,
    stop_reason: StopReason,
}

impl ChatResponseBuilder {
    pub(crate) fn new(default_model: impl Into<String>) -> Self {
        Self {
            content: String::new(),
            thinking: None,
            tool_calls: Vec::new(),
            model: default_model.into(),
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
            stop_reason: StopReason::Other,
        }
    }

    pub(crate) fn content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    pub(crate) fn thinking(mut self, thinking: Option<String>) -> Self {
        self.thinking = thinking;
        self
    }

    pub(crate) fn model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub(crate) fn tool_calls(mut self, tool_calls: Vec<ToolCall>) -> Self {
        self.tool_calls = tool_calls;
        self
    }

    pub(crate) fn usage(mut self, input_tokens: u32, output_tokens: u32) -> Self {
        self.usage = Usage {
            input_tokens,
            output_tokens,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        };
        self
    }

    pub(crate) fn cache_usage(
        mut self,
        cache_creation_input_tokens: Option<u32>,
        cache_read_input_tokens: Option<u32>,
    ) -> Self {
        self.usage.cache_creation_input_tokens = cache_creation_input_tokens;
        self.usage.cache_read_input_tokens = cache_read_input_tokens;
        self
    }

    pub(crate) fn stop_reason(mut self, stop_reason: StopReason) -> Self {
        self.stop_reason = stop_reason;
        self
    }

    pub(crate) fn build(self) -> ChatResponse {
        ChatResponse {
            content: self.content,
            thinking: self.thinking,
            tool_calls: self.tool_calls,
            model: self.model,
            usage: self.usage,
            stop_reason: self.stop_reason,
            session_id: None,
        }
    }
}

pub(crate) fn extract_error_message(payload: &Value, fallback: &str) -> String {
    payload
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}

pub(crate) fn map_http_error(
    status_code: u16,
    message: String,
    retry_after: Option<Duration>,
    request_id: Option<String>,
) -> MotosanError {
    match status_code {
        401 => MotosanError::Auth {
            message,
            status_code: Some(status_code),
            retry_after,
            request_id,
        },
        429 => MotosanError::RateLimit {
            message,
            status_code: Some(status_code),
            retry_after,
            request_id,
        },
        400 => MotosanError::InvalidRequest {
            message,
            status_code: Some(status_code),
            retry_after,
            request_id,
        },
        _ => MotosanError::ProviderError {
            message,
            status_code: Some(status_code),
            retry_after,
            request_id,
        },
    }
}

pub(crate) fn is_retryable_status(status_code: u16) -> bool {
    status_code == 408 || status_code == 409 || status_code == 429 || status_code >= 500
}

pub(crate) fn is_retryable_network_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request() || error.is_body()
}

pub(crate) const RETRY_AFTER_CAP: Duration = Duration::from_secs(60);

pub(crate) fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let raw = headers.get("retry-after")?.to_str().ok()?.trim();
    let uncapped = if let Ok(seconds) = raw.parse::<f64>() {
        if !seconds.is_finite() || seconds < 0.0 {
            return None;
        }
        Duration::from_secs_f64(seconds.min(RETRY_AFTER_CAP.as_secs_f64()))
    } else {
        // RFC 7231 HTTP-date form (e.g. "Fri, 31 Dec 1999 23:59:59 GMT").
        let when = chrono::DateTime::parse_from_rfc2822(raw).ok()?;
        let remaining = when.signed_duration_since(chrono::Utc::now());
        Duration::from_secs(remaining.num_seconds().max(0) as u64)
    };
    Some(uncapped.min(RETRY_AFTER_CAP))
}

pub(crate) fn extract_request_id(headers: &HeaderMap) -> Option<String> {
    ["request-id", "x-request-id"].iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    })
}

async fn observe_and_sleep(
    policy: &RetryPolicy,
    attempt: u32,
    retry_after: Option<Duration>,
    cause: RetryCause,
) {
    // Compute once so the observer sees the exact delay that is slept.
    let delay = if policy.respect_retry_after {
        retry_after.unwrap_or_else(|| policy.delay_for_attempt(attempt))
    } else {
        policy.delay_for_attempt(attempt)
    };
    if let Some(on_retry) = policy.on_retry.as_deref() {
        on_retry(RetryEvent {
            attempt,
            delay,
            cause,
        });
    }
    tokio::time::sleep(delay).await;
}

/// One retry engine for every HTTP provider (normative contract: specs/retry.md).
///
/// `build` is awaited at the top of EVERY attempt — including retries — so
/// per-attempt credential resolution (`crate::auth::TokenSource`) sees a
/// fresh token each time. A `build` error aborts immediately: it is a
/// caller-side failure, not a network failure, and is never retried.
///
/// Returns success and terminal non-success responses with the body
/// untouched, leaving caller-side parsing and provider-specific error
/// shaping intact. `on_retry` observers fire only here (M2 choke point).
pub(crate) async fn send_with_retry_async_build<F, Fut>(
    policy: &RetryPolicy,
    build: F,
) -> Result<reqwest::Response, MotosanError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<reqwest::RequestBuilder, MotosanError>>,
{
    let mut attempt: u32 = 0;
    loop {
        let request = build().await?;
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                if attempt < policy.max_retries && is_retryable_network_error(&error) {
                    attempt += 1;
                    let cause = RetryCause::Network(error.to_string());
                    observe_and_sleep(policy, attempt, None, cause).await;
                    continue;
                }
                return Err(MotosanError::Network(error.to_string()));
            }
        };

        let status = response.status();
        if !status.is_success()
            && attempt < policy.max_retries
            && is_retryable_status(status.as_u16())
        {
            let retry_after = parse_retry_after(response.headers());
            attempt += 1;
            let cause = RetryCause::Status(status.as_u16());
            observe_and_sleep(policy, attempt, retry_after, cause).await;
            continue;
        }

        return Ok(response);
    }
}

/// Sync-build convenience over [`send_with_retry_async_build`] — the single
/// retry engine every HTTP provider shares (normative contract:
/// specs/retry.md).
pub(crate) async fn send_with_retry(
    policy: &RetryPolicy,
    build: impl Fn() -> reqwest::RequestBuilder,
) -> Result<reqwest::Response, MotosanError> {
    send_with_retry_async_build(policy, || {
        std::future::ready(Ok::<_, MotosanError>(build()))
    })
    .await
}

/// Apply the opt-in total timeout to a blocking-chat request.
pub(crate) fn apply_total_timeout(
    rb: reqwest::RequestBuilder,
    total: Option<Duration>,
) -> reqwest::RequestBuilder {
    match total {
        Some(timeout) => rb.timeout(timeout),
        None => rb,
    }
}

/// Total-timeout wrapper for providers whose `chat()` is stream+collect.
pub(crate) async fn collect_stream_with_total_timeout(
    stream: BoxStream,
    total: Option<Duration>,
) -> Result<ChatResponse, MotosanError> {
    match total {
        Some(timeout) => {
            match tokio::time::timeout(timeout, crate::stream::collect_stream(stream)).await {
                Ok(result) => result,
                Err(_) => Err(MotosanError::Network(format!(
                    "total timeout: chat did not complete within {}s",
                    timeout.as_secs()
                ))),
            }
        }
        None => crate::stream::collect_stream(stream).await,
    }
}

#[cfg(test)]
mod retry_after_tests {
    use super::{is_retryable_status, parse_retry_after, RETRY_AFTER_CAP};
    use reqwest::header::{HeaderMap, HeaderValue};
    use std::time::Duration;

    fn headers_with_retry_after(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "retry-after",
            HeaderValue::from_str(value).expect("ascii header value"),
        );
        headers
    }

    #[test]
    fn integer_seconds_parse() {
        let headers = headers_with_retry_after("5");
        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(5)));
    }

    #[test]
    fn integer_seconds_above_cap_clamp_to_cap() {
        let headers = headers_with_retry_after("120");
        assert_eq!(parse_retry_after(&headers), Some(RETRY_AFTER_CAP));
    }

    #[test]
    fn decimal_seconds_parse() {
        let headers = headers_with_retry_after("1.5");
        assert_eq!(
            parse_retry_after(&headers),
            Some(Duration::from_millis(1500))
        );
    }

    #[test]
    fn negative_numeric_value_returns_none() {
        let headers = headers_with_retry_after("-1");
        assert_eq!(parse_retry_after(&headers), None);
    }

    #[test]
    fn http_date_near_future_parses_to_remaining_seconds() {
        // Fixed offset from now via chrono arithmetic - no wall-clock strings.
        let when = chrono::Utc::now() + chrono::Duration::seconds(30);
        let headers = headers_with_retry_after(&when.to_rfc2822());
        let delay = parse_retry_after(&headers).expect("http-date should parse");
        // signed_duration_since truncates and the test itself takes time,
        // so allow a small window below 30s.
        assert!(
            (28..=30).contains(&delay.as_secs()),
            "expected ~30s, got {delay:?}"
        );
    }

    #[test]
    fn http_date_in_past_clamps_to_zero() {
        let when = chrono::Utc::now() - chrono::Duration::seconds(100);
        let headers = headers_with_retry_after(&when.to_rfc2822());
        assert_eq!(parse_retry_after(&headers), Some(Duration::ZERO));
    }

    #[test]
    fn http_date_far_future_clamps_to_cap() {
        let when = chrono::Utc::now() + chrono::Duration::seconds(300);
        let headers = headers_with_retry_after(&when.to_rfc2822());
        assert_eq!(parse_retry_after(&headers), Some(RETRY_AFTER_CAP));
    }

    #[test]
    fn garbage_value_returns_none() {
        let headers = headers_with_retry_after("soon");
        assert_eq!(parse_retry_after(&headers), None);
    }

    #[test]
    fn missing_header_returns_none() {
        assert_eq!(parse_retry_after(&HeaderMap::new()), None);
    }

    #[test]
    fn retryable_status_set_is_408_409_429_and_5xx() {
        assert!(is_retryable_status(408));
        assert!(is_retryable_status(409));
        assert!(is_retryable_status(429));
        assert!(is_retryable_status(500));
        assert!(is_retryable_status(503));
        assert!(!is_retryable_status(400));
        assert!(!is_retryable_status(404));
        assert!(!is_retryable_status(200));
    }
}

#[cfg(test)]
mod http_error_metadata_tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderName};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                HeaderName::from_bytes(name.as_bytes()).expect("header name"),
                value.parse().expect("header value"),
            );
        }
        map
    }

    #[test]
    fn extract_request_id_prefers_request_id_then_x_request_id() {
        let both = headers(&[("x-request-id", "xrid"), ("request-id", "rid")]);
        assert_eq!(extract_request_id(&both).as_deref(), Some("rid"));
        let fallback = headers(&[("x-request-id", "xrid")]);
        assert_eq!(extract_request_id(&fallback).as_deref(), Some("xrid"));
        assert_eq!(extract_request_id(&HeaderMap::new()), None);
    }

    #[test]
    fn map_http_error_populates_metadata_from_headers() {
        let map = headers(&[("retry-after", "7"), ("request-id", "req_123")]);
        let err = map_http_error(
            429,
            "too many".to_string(),
            parse_retry_after(&map),
            extract_request_id(&map),
        );
        assert!(matches!(err, MotosanError::RateLimit { .. }));
        assert_eq!(err.status_code(), Some(429));
        assert_eq!(err.retry_after(), Some(Duration::from_secs(7)));
        assert_eq!(err.request_id(), Some("req_123"));
        assert_eq!(err.to_string(), "rate limit error: too many");
    }

    #[test]
    fn map_http_error_maps_status_to_variant() {
        assert!(matches!(
            map_http_error(401, "m".into(), None, None),
            MotosanError::Auth { .. }
        ));
        assert!(matches!(
            map_http_error(400, "m".into(), None, None),
            MotosanError::InvalidRequest { .. }
        ));
        assert!(matches!(
            map_http_error(429, "m".into(), None, None),
            MotosanError::RateLimit { .. }
        ));
        assert!(matches!(
            map_http_error(500, "m".into(), None, None),
            MotosanError::ProviderError { .. }
        ));
        assert_eq!(
            map_http_error(500, "m".into(), None, None).status_code(),
            Some(500)
        );
    }
}

// Mirrors specs/retry.md — update BOTH or neither.
// Cross-SDK siblings assert the SAME tables:
//   sdks/python/tests/test_retry_conformance.py
//   sdks/typescript/tests/retry-conformance.test.ts
// A failure means the implementation drifted from specs/retry.md — fix the
// implementation (or the spec plus all three suites), never this file alone.
// In-crate unit test (NOT tests/): is_retryable_status / parse_retry_after are
// pub(crate) in this module, so an integration test could not
// import them. Compiled behind the module-level _http gate (default = []).
#[cfg(test)]
mod retry_conformance {
    use super::{is_retryable_status, parse_retry_after};
    use crate::retry::RetryPolicy;
    use reqwest::header::{HeaderMap, HeaderValue};
    use std::time::Duration;

    const RETRYABLE: [u16; 8] = [408, 409, 429, 500, 502, 503, 529, 599];
    const NON_RETRYABLE: [u16; 8] = [200, 301, 400, 401, 403, 404, 422, 499];

    /// parse_retry_after takes a &HeaderMap, so wrap the raw value in one.
    fn retry_after(value: &str) -> Option<Duration> {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_str(value).unwrap());
        parse_retry_after(&headers)
    }

    #[test]
    fn classification_retryable_statuses() {
        for status in RETRYABLE {
            assert!(
                is_retryable_status(status),
                "specs/retry.md: {status} must be retryable"
            );
        }
    }

    #[test]
    fn classification_non_retryable_statuses() {
        for status in NON_RETRYABLE {
            assert!(
                !is_retryable_status(status),
                "specs/retry.md: {status} must NOT be retryable"
            );
        }
    }

    #[test]
    fn retry_after_integer_seconds() {
        assert_eq!(retry_after("5"), Some(Duration::from_secs(5)));
        assert_eq!(retry_after("0"), Some(Duration::ZERO));
        assert_eq!(retry_after(" 7 "), Some(Duration::from_secs(7)));
    }

    #[test]
    fn retry_after_capped_at_60s() {
        assert_eq!(retry_after("61"), Some(Duration::from_secs(60)));
        assert_eq!(retry_after("86400"), Some(Duration::from_secs(60)));
    }

    #[test]
    fn retry_after_http_date_clamped() {
        // Deterministic: past date clamps to 0; far-future date clamps to the 60s cap.
        assert_eq!(
            retry_after("Wed, 21 Oct 2015 07:28:00 GMT"),
            Some(Duration::ZERO)
        );
        assert_eq!(
            retry_after("Fri, 31 Dec 2100 23:59:59 GMT"),
            Some(Duration::from_secs(60))
        );
    }

    #[test]
    fn retry_after_garbage_is_none() {
        // A negative integer is invalid, NOT clamp-to-0 (only past HTTP-dates clamp).
        assert_eq!(retry_after(""), None);
        assert_eq!(retry_after("soon"), None);
        assert_eq!(retry_after("-5"), None);
    }

    #[test]
    fn full_jitter_bounded_by_capped_exponential() {
        // specs/retry.md § backoff: full jitter — uniform in
        // [0, min(base * 2^(attempt-1), max_delay)]. The Rust RNG is &mut
        // fastrand::Rng (no fractional-scale injection), so assert the ceiling
        // over a seeded run instead of an exact scaled value.
        let policy = RetryPolicy::default(); // base 100ms, max 2000ms, jitter on
        let mut rng = fastrand::Rng::with_seed(42);
        // ceilings for attempts 1..=7 (attempt 6+ capped at max_delay = 2000ms)
        let ceilings_ms = [100u64, 200, 400, 800, 1600, 2000, 2000];
        for (idx, &ceiling) in ceilings_ms.iter().enumerate() {
            let attempt = (idx + 1) as u32;
            for _ in 0..200 {
                let delay = policy.delay_for_attempt_with_rng(attempt, &mut rng);
                assert!(
                    delay <= Duration::from_millis(ceiling),
                    "specs/retry.md: attempt {attempt} delay {delay:?} exceeds {ceiling}ms ceiling"
                );
            }
        }
        // Full jitter must spread draws, not pin to the ceiling (seed-deterministic).
        let mut rng = fastrand::Rng::with_seed(42);
        let draws: Vec<Duration> = (0..64)
            .map(|_| policy.delay_for_attempt_with_rng(3, &mut rng))
            .collect();
        assert!(
            draws.iter().any(|d| d != &draws[0]),
            "specs/retry.md: full jitter must vary across draws: {draws:?}"
        );
    }

    #[test]
    fn jitter_disabled_is_pure_exponential() {
        let policy = RetryPolicy::default().jitter(false);
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(100));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(200));
        assert_eq!(policy.delay_for_attempt(5), Duration::from_millis(1600));
        assert_eq!(policy.delay_for_attempt(6), Duration::from_millis(2000));
    }

    #[test]
    fn defaults_match_spec() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.base_delay_ms, 100);
        assert_eq!(policy.max_delay_ms, 2000);
        assert!(policy.jitter);
        assert!(policy.respect_retry_after);
    }
}

// Per-attempt async request construction (F5). The build future runs at the
// top of EVERY attempt so credential lookups (crate::auth::TokenSource) are
// re-resolved per retry. send_with_retry delegates here — single retry
// engine, M2 choke-point decision intact.
#[cfg(test)]
mod async_build_engine {
    use super::send_with_retry_async_build;
    use crate::error::MotosanError;
    use crate::retry::RetryPolicy;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn fast_retry(max: u32) -> RetryPolicy {
        RetryPolicy::new()
            .max_retries(max)
            .base_delay_ms(0)
            .max_delay_ms(0)
            .jitter(false)
    }

    #[tokio::test]
    async fn build_future_runs_once_per_attempt() {
        let mut server = mockito::Server::new_async().await;
        // Creation order + expect(1) saturation: attempt 1 -> 503, attempt 2
        // -> 200 (same precedent as tests/chatgpt_codex.rs
        // stream_fires_on_retry_via_shared_engine).
        server
            .mock("GET", "/probe")
            .with_status(503)
            .expect(1)
            .create_async()
            .await;
        server
            .mock("GET", "/probe")
            .with_status(200)
            .expect(1)
            .create_async()
            .await;

        let http = reqwest::Client::new();
        let url = format!("{}/probe", server.url());
        let builds = AtomicUsize::new(0);

        let response = send_with_retry_async_build(&fast_retry(1), || {
            let http = &http;
            let url = &url;
            let builds = &builds;
            async move {
                builds.fetch_add(1, Ordering::SeqCst);
                Ok(http.get(url))
            }
        })
        .await
        .expect("second attempt succeeds");

        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(builds.load(Ordering::SeqCst), 2, "one build per attempt");
    }

    #[tokio::test]
    async fn build_error_short_circuits_without_retry() {
        let builds = AtomicUsize::new(0);
        let err = send_with_retry_async_build(&fast_retry(3), || {
            let builds = &builds;
            async move {
                builds.fetch_add(1, Ordering::SeqCst);
                Err::<reqwest::RequestBuilder, _>(MotosanError::Auth {
                    message: "no token".into(),
                    status_code: None,
                    retry_after: None,
                    request_id: None,
                })
            }
        })
        .await
        .expect_err("build error propagates");

        assert!(matches!(err, MotosanError::Auth { ref message, .. } if message == "no token"));
        assert_eq!(
            builds.load(Ordering::SeqCst),
            1,
            "build errors are not retried"
        );
    }
}
