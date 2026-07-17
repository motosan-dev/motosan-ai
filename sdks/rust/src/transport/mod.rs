//! Transport strata shared across provider implementations (M4 Task 1).
//!
//! - [`http`]: helpers shared by every HTTP provider, compiled ONCE behind
//!   the private `_http` umbrella feature.
//! - [`cli`]: helpers shared by every CLI backend, compiled ONCE behind the
//!   private `_cli` umbrella feature.
//!
//! Rule: new providers route through `_http`/`_cli` in `Cargo.toml`. Adding
//! a per-provider `#[cfg(any(...))]` enumeration in shared code is
//! review-blocking (see sdks/rust/README.md, "Feature architecture rules").

#[cfg(feature = "_cli")]
pub(crate) mod cli;
#[cfg(feature = "_http")]
pub(crate) mod http;

use std::time::Duration;

pub(crate) const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const DEFAULT_READ_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// Unified timeout model:
/// - `connect`: TCP/TLS connect deadline on the shared reqwest client.
/// - `read_idle`: max gap between HTTP stream chunks before
///   `MotosanError::StreamReadTimeout`.
/// - `total`: opt-in wall-clock budget per blocking `chat()` attempt.
///
/// Lives at the ungated transport root (not inside `http`) because the
/// public accessors `Client::connect_timeout/read_idle_timeout/total_timeout`
/// exist on every feature combination, including `--no-default-features`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimeoutConfig {
    pub(crate) connect: Duration,
    pub(crate) read_idle: Duration,
    pub(crate) total: Option<Duration>,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            connect: DEFAULT_CONNECT_TIMEOUT,
            read_idle: DEFAULT_READ_IDLE_TIMEOUT,
            total: None,
        }
    }
}
