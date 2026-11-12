use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Classification drives whether a request is retried, how long we wait, and
/// whether the whole download must restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    /// DNS failure, reset, timeout, 5xx. Retry the same byte range.
    Transient,
    /// Explicit backpressure (429/503). Retry with a longer delay.
    RateLimited,
    /// Credentials absent or rejected (401/403). Do not retry silently.
    Auth,
    /// Permanent HTTP condition (404/410).
    NotFound,
    /// The remote file or range changed (416, validator mismatch).
    RangeChanged,
    /// The server does not implement ranges after all.
    RangeUnsupported,
    /// Local disk/path/configuration error. Never retry.
    Fatal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    /// Multiplicative jitter in `[1-jitter, 1+jitter]`.
    pub jitter: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 6,
            base_delay_ms: 250,
            max_delay_ms: 30_000,
            jitter: 0.25,
        }
    }
}

impl RetryPolicy {
    #[must_use]
    pub fn from_engine(config: &crate::EngineConfig) -> Self {
        Self {
            max_retries: config.max_retries,
            base_delay_ms: config.retry_base_ms,
            max_delay_ms: config.retry_max_ms,
            jitter: 0.25,
        }
    }

    /// Classify an HTTP status observed while requesting a byte range.
    #[must_use]
    pub fn classify_status(status: u16) -> ErrorClass {
        match status {
            401 | 407 => ErrorClass::Auth,
            403 => ErrorClass::Auth,
            404 | 410 => ErrorClass::NotFound,
            408 | 425 | 500 | 502 | 504 => ErrorClass::Transient,
            429 | 503 => ErrorClass::RateLimited,
            416 => ErrorClass::RangeChanged,
            _ if status >= 500 => ErrorClass::Transient,
            _ => ErrorClass::Fatal,
        }
    }

    #[must_use]
    pub fn should_retry(&self, class: ErrorClass, attempt: u32) -> bool {
        matches!(
            class,
            ErrorClass::Transient | ErrorClass::RateLimited | ErrorClass::RangeChanged
        ) && attempt <= self.max_retries
    }

    /// Exponential backoff with full-period jitter. Rate limits start at four
    /// times the base delay because servers expect meaningful quiet periods.
    #[must_use]
    pub fn delay(&self, class: ErrorClass, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1).min(20);
        let base = if class == ErrorClass::RateLimited {
            self.base_delay_ms.saturating_mul(4)
        } else {
            self.base_delay_ms
        };
        let exponential = base.saturating_mul(1_u64 << exponent);
        let capped = exponential.min(self.max_delay_ms);
        let multiplier = 1.0 + (fastrand::f64() * 2.0 - 1.0) * self.jitter;
        Duration::from_millis((capped as f64 * multiplier).max(1.0) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_classes() {
        assert_eq!(RetryPolicy::classify_status(429), ErrorClass::RateLimited);
        assert_eq!(RetryPolicy::classify_status(503), ErrorClass::RateLimited);
        assert_eq!(RetryPolicy::classify_status(403), ErrorClass::Auth);
        assert_eq!(RetryPolicy::classify_status(404), ErrorClass::NotFound);
        assert_eq!(RetryPolicy::classify_status(416), ErrorClass::RangeChanged);
    }

    #[test]
    fn backoff_is_capped_and_jittered() {
        let policy = RetryPolicy {
            max_retries: 10,
            base_delay_ms: 100,
            max_delay_ms: 2_000,
            jitter: 0.0,
        };
        assert_eq!(
            policy.delay(ErrorClass::Transient, 1),
            Duration::from_millis(100)
        );
        assert_eq!(
            policy.delay(ErrorClass::Transient, 4),
            Duration::from_millis(800)
        );
        assert_eq!(
            policy.delay(ErrorClass::Transient, 9),
            Duration::from_millis(2_000)
        );
    }

    #[test]
    fn auth_is_not_retried() {
        let policy = RetryPolicy::default();
        assert!(!policy.should_retry(ErrorClass::Auth, 1));
        assert!(policy.should_retry(ErrorClass::Transient, 1));
        assert!(!policy.should_retry(ErrorClass::Transient, 99));
    }
}
