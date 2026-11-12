use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::time::Instant;

/// A cancellation-safe token bucket shared by every active transfer.
///
/// A rate of zero means unlimited. Requests larger than the bucket capacity
/// are served in capacity-sized slices so no single caller can deadlock.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    state: Mutex<Bucket>,
    capacity: u64,
    unlimited: bool,
}

#[derive(Debug)]
struct Bucket {
    rate: f64,
    tokens: f64,
    last: Instant,
    capacity: f64,
}

impl RateLimiter {
    /// `bytes_per_sec == 0` disables limiting.
    #[must_use]
    pub fn new(bytes_per_sec: u64) -> Self {
        let capacity = bytes_per_sec.clamp(64 * 1024, 16 * 1024 * 1024);
        let now = Instant::now();
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(Bucket {
                    rate: bytes_per_sec as f64,
                    tokens: capacity as f64,
                    last: now,
                    capacity: capacity as f64,
                }),
                capacity,
                unlimited: bytes_per_sec == 0,
            }),
        }
    }

    #[must_use]
    pub fn unlimited() -> Self {
        Self::new(0)
    }

    pub fn set_rate(&self, bytes_per_sec: u64) {
        let mut state = self.inner.state.lock();
        state.rate = bytes_per_sec as f64;
        state.capacity = if bytes_per_sec == 0 {
            state.capacity.max(64.0 * 1024.0)
        } else {
            (bytes_per_sec as f64).clamp(64.0 * 1024.0, 16.0 * 1024.0 * 1024.0)
        };
        state.tokens = state.tokens.min(state.capacity);
        state.last = Instant::now();
    }

    #[must_use]
    pub fn rate(&self) -> u64 {
        let state = self.inner.state.lock();
        state.rate as u64
    }

    /// Wait until `bytes` may be sent. Handles arbitrary sizes and adapts if the
    /// rate is changed while waiting.
    pub async fn acquire(&self, bytes: u64) {
        if self.inner.unlimited || bytes == 0 {
            return;
        }
        let mut remaining = bytes;
        while remaining > 0 {
            let slice = remaining.min(self.inner.capacity);
            self.acquire_slice(slice).await;
            remaining -= slice;
        }
    }

    async fn acquire_slice(&self, bytes: u64) {
        loop {
            let wait = {
                let mut state = self.inner.state.lock();
                let now = Instant::now();
                let elapsed = now.saturating_duration_since(state.last);
                state.last = now;
                state.tokens =
                    (state.tokens + elapsed.as_secs_f64() * state.rate).min(state.capacity);
                if state.rate <= 0.0 {
                    return;
                }
                if state.tokens >= bytes as f64 {
                    state.tokens -= bytes as f64;
                    return;
                }
                let missing = bytes as f64 - state.tokens;
                Duration::from_secs_f64((missing / state.rate).max(0.000_001))
            };
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn unlimited_returns_immediately() {
        let limiter = RateLimiter::unlimited();
        let start = Instant::now();
        limiter.acquire(10_000_000).await;
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn bucket_delays_after_initial_burst() {
        let limiter = RateLimiter::new(100_000);
        // Bucket starts full: 100 KiB is free.
        limiter.acquire(100_000).await;
        let start = Instant::now();
        limiter.acquire(100_000).await;
        assert!(start.elapsed() >= Duration::from_secs(1));
    }

    #[tokio::test(start_paused = true)]
    async fn large_request_is_split_into_capacity_slices() {
        let limiter = RateLimiter::new(64 * 1024);
        limiter.acquire(64 * 1024).await;
        let start = Instant::now();
        limiter.acquire(128 * 1024).await;
        assert!(start.elapsed() >= Duration::from_secs(2));
    }
}
