use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use url::Url;

/// Process-wide per-host request budget shared by every download.
///
/// A permit is held for the lifetime of one HTTP response body, so this limits
/// real concurrent requests rather than merely queued workers.
#[derive(Debug)]
pub struct HostLimiter {
    max: usize,
    hosts: Mutex<HashMap<String, Arc<Semaphore>>>,
}

impl HostLimiter {
    #[must_use]
    pub fn new(max_per_host: usize) -> Self {
        Self {
            max: max_per_host.max(1),
            hosts: Mutex::new(HashMap::new()),
        }
    }

    pub async fn acquire(&self, url: &Url) -> OwnedSemaphorePermit {
        let key = host_key(url);
        let semaphore = {
            let mut hosts = self.hosts.lock();
            Arc::clone(
                hosts
                    .entry(key)
                    .or_insert_with(|| Arc::new(Semaphore::new(self.max))),
            )
        };
        semaphore
            .acquire_owned()
            .await
            .expect("host semaphore is never closed")
    }
}

fn host_key(url: &Url) -> String {
    format!(
        "{}:{}:{}",
        url.scheme(),
        url.host_str().unwrap_or_default(),
        url.port_or_known_default().unwrap_or(0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn caps_host_permits_and_releases() {
        let limiter = HostLimiter::new(1);
        let url = Url::parse("https://example.test/a").unwrap();
        let first = limiter.acquire(&url).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(50), limiter.acquire(&url))
                .await
                .is_err()
        );
        drop(first);
        assert!(
            tokio::time::timeout(Duration::from_millis(500), limiter.acquire(&url))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn separate_hosts_do_not_share_the_limit() {
        let limiter = HostLimiter::new(1);
        let a = Url::parse("https://a.test/x").unwrap();
        let b = Url::parse("https://b.test/x").unwrap();
        let _first = limiter.acquire(&a).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(100), limiter.acquire(&b))
                .await
                .is_ok()
        );
    }
}
