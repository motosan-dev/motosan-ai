use std::fmt;
use std::sync::Arc;
use std::time::Duration;

/// Why a retry is happening. Passed to `RetryPolicy::on_retry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryCause {
    /// Retryable HTTP status code received from the provider.
    Status(u16),
    /// Retryable transport/network error (message from the HTTP client).
    Network(String),
}

/// Fired via `RetryPolicy::on_retry` before each retry sleep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryEvent {
    pub attempt: u32,
    pub delay: Duration,
    pub cause: RetryCause,
}

#[derive(Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub jitter: bool,
    pub respect_retry_after: bool,
    /// Observability hook: called once before each retry sleep.
    pub on_retry: Option<Arc<dyn Fn(RetryEvent) + Send + Sync>>,
}

impl RetryPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn base_delay_ms(mut self, base_delay_ms: u64) -> Self {
        self.base_delay_ms = base_delay_ms;
        self
    }

    pub fn max_delay_ms(mut self, max_delay_ms: u64) -> Self {
        self.max_delay_ms = max_delay_ms;
        self
    }

    pub fn jitter(mut self, jitter: bool) -> Self {
        self.jitter = jitter;
        self
    }

    pub fn respect_retry_after(mut self, respect_retry_after: bool) -> Self {
        self.respect_retry_after = respect_retry_after;
        self
    }

    pub fn on_retry(mut self, on_retry: impl Fn(RetryEvent) + Send + Sync + 'static) -> Self {
        self.on_retry = Some(Arc::new(on_retry));
        self
    }

    /// Full-jitter backoff: uniform in [0, min(base * 2^(attempt-1), max_delay)].
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        self.delay_for_attempt_with_rng(attempt, &mut fastrand::Rng::new())
    }

    /// Injectable-RNG variant so tests can seed the jitter deterministically.
    pub(crate) fn delay_for_attempt_with_rng(
        &self,
        attempt: u32,
        rng: &mut fastrand::Rng,
    ) -> Duration {
        let exponent = attempt.saturating_sub(1).min(31);
        let exp_factor = 1_u64 << exponent;
        let exp_delay_ms = self
            .base_delay_ms
            .saturating_mul(exp_factor)
            .min(self.max_delay_ms);

        let delay_ms = if self.jitter {
            rng.u64(0..=exp_delay_ms)
        } else {
            exp_delay_ms
        };

        Duration::from_millis(delay_ms)
    }
}

impl fmt::Debug for RetryPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RetryPolicy")
            .field("max_retries", &self.max_retries)
            .field("base_delay_ms", &self.base_delay_ms)
            .field("max_delay_ms", &self.max_delay_ms)
            .field("jitter", &self.jitter)
            .field("respect_retry_after", &self.respect_retry_after)
            .field("on_retry", &self.on_retry.is_some())
            .finish()
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 100,
            max_delay_ms: 2_000,
            jitter: true,
            respect_retry_after: true,
            on_retry: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exp_delay_ms(policy: &RetryPolicy, attempt: u32) -> u64 {
        let exp_factor = 1_u64 << attempt.saturating_sub(1).min(31);
        policy
            .base_delay_ms
            .saturating_mul(exp_factor)
            .min(policy.max_delay_ms)
    }

    #[test]
    fn full_jitter_is_bounded_by_exponential_delay() {
        let policy = RetryPolicy::new();
        let mut rng = fastrand::Rng::with_seed(42);
        for attempt in 1..=8 {
            let bound = exp_delay_ms(&policy, attempt);
            for _ in 0..200 {
                let delay = policy.delay_for_attempt_with_rng(attempt, &mut rng);
                assert!(
                    delay <= Duration::from_millis(bound),
                    "attempt {attempt}: {delay:?} exceeds {bound}ms"
                );
            }
        }
    }

    #[test]
    fn full_jitter_varies_and_is_seed_deterministic() {
        let policy = RetryPolicy::new();
        let mut a = fastrand::Rng::with_seed(7);
        let mut b = fastrand::Rng::with_seed(7);
        let draws_a: Vec<Duration> = (0..32)
            .map(|_| policy.delay_for_attempt_with_rng(4, &mut a))
            .collect();
        let draws_b: Vec<Duration> = (0..32)
            .map(|_| policy.delay_for_attempt_with_rng(4, &mut b))
            .collect();
        assert_eq!(draws_a, draws_b, "same seed must produce same delays");
        assert!(
            draws_a.iter().any(|d| d != &draws_a[0]),
            "full jitter must vary across draws: {draws_a:?}"
        );
    }

    #[test]
    fn jitter_disabled_is_exact_exponential() {
        let policy = RetryPolicy::new().jitter(false);
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(100));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(200));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(400));
        assert_eq!(policy.delay_for_attempt(4), Duration::from_millis(800));
        assert_eq!(policy.delay_for_attempt(5), Duration::from_millis(1600));
        assert_eq!(policy.delay_for_attempt(6), Duration::from_millis(2000));
        assert_eq!(policy.delay_for_attempt(99), Duration::from_millis(2000));
    }

    #[test]
    fn debug_skips_on_retry_closure() {
        let with_hook = RetryPolicy::new().on_retry(|_evt| {});
        assert!(format!("{with_hook:?}").contains("on_retry: true"));
        assert!(format!("{:?}", RetryPolicy::new()).contains("on_retry: false"));
    }
}
