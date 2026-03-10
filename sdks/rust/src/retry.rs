use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub jitter: bool,
    pub respect_retry_after: bool,
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

    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1).min(31);
        let exp_factor = 1_u64 << exponent;
        let mut delay_ms = self.base_delay_ms.saturating_mul(exp_factor);
        delay_ms = delay_ms.min(self.max_delay_ms);

        if self.jitter {
            let jitter_seed = attempt as u64 * 1_103_515_245 + 12_345;
            let jitter_percent = jitter_seed % 100;
            let jittered = delay_ms + (delay_ms.saturating_mul(jitter_percent) / 100);
            delay_ms = jittered.min(self.max_delay_ms);
        }

        Duration::from_millis(delay_ms)
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
        }
    }
}
