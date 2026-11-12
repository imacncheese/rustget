use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// Tuning for a single download. Values are clamped and validated by
/// [`EngineConfig::validate`] before a transfer starts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EngineConfig {
    /// Number of connections requested at the start of a ranged download.
    pub connections: u8,
    /// Hard ceiling for simultaneously active range workers.
    pub max_connections: u8,
    /// A range is never split if either resulting half would be smaller.
    pub min_segment_size: u64,
    /// Bytes requested per network chunk and written per positional write.
    pub read_buffer_size: usize,
    /// Retry attempts per segment after the first failure.
    pub max_retries: u32,
    /// Initial exponential-backoff delay.
    pub retry_base_ms: u64,
    /// Maximum backoff delay.
    pub retry_max_ms: u64,
    /// Global token-bucket rate. `None` means unlimited.
    pub max_speed_bytes_per_sec: Option<u64>,
    /// Fail a transfer when no byte arrives for this long.
    pub idle_timeout_secs: u64,
    /// Overall timeout for the probe request.
    pub probe_timeout_secs: u64,
    /// Overall timeout for one range request.
    pub request_timeout_secs: u64,
    /// Scale connections up when aggregate throughput improves.
    pub adaptive_connections: bool,
    /// Minimum time between adaptive growth decisions.
    pub adaptive_interval_ms: u64,
    /// How often runtime progress is durably checkpointed.
    pub checkpoint_interval_ms: u64,
    /// Sync file data before recording checkpoint progress.
    pub fsync_checkpoints: bool,
    /// Verify with If-Range when resuming and refuse a changed remote file.
    pub validate_on_resume: bool,
    /// Restart automatically instead of prompting when the remote changed.
    pub restart_on_change: bool,
    /// Follow redirects while probing and downloading.
    pub max_redirects: usize,
    /// User-Agent used when the caller does not supply one.
    pub user_agent: String,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            connections: 8,
            max_connections: 32,
            min_segment_size: 256 * 1024,
            read_buffer_size: 64 * 1024,
            max_retries: 6,
            retry_base_ms: 250,
            retry_max_ms: 30_000,
            max_speed_bytes_per_sec: None,
            idle_timeout_secs: 30,
            probe_timeout_secs: 30,
            request_timeout_secs: 0,
            adaptive_connections: true,
            adaptive_interval_ms: 2_000,
            checkpoint_interval_ms: 250,
            fsync_checkpoints: false,
            validate_on_resume: true,
            restart_on_change: false,
            max_redirects: 10,
            user_agent: format!("RustGet/{}", env!("CARGO_PKG_VERSION")),
        }
    }
}

impl EngineConfig {
    pub fn validate(&self) -> Result<()> {
        if self.connections == 0 {
            return Err(CoreError::InvalidConfig(
                "connections must be at least 1".into(),
            ));
        }
        if self.max_connections == 0 {
            return Err(CoreError::InvalidConfig(
                "max_connections must be at least 1".into(),
            ));
        }
        if self.connections > self.max_connections {
            return Err(CoreError::InvalidConfig(format!(
                "connections ({}) exceeds max_connections ({})",
                self.connections, self.max_connections
            )));
        }
        if self.max_connections > 32 {
            return Err(CoreError::InvalidConfig(
                "max_connections cannot exceed 32".into(),
            ));
        }
        if self.min_segment_size == 0 {
            return Err(CoreError::InvalidConfig(
                "min_segment_size must be greater than zero".into(),
            ));
        }
        if self.read_buffer_size < 1024 {
            return Err(CoreError::InvalidConfig(
                "read_buffer_size must be at least 1024".into(),
            ));
        }
        if self.checkpoint_interval_ms == 0 {
            return Err(CoreError::InvalidConfig(
                "checkpoint_interval_ms must be greater than zero".into(),
            ));
        }
        if self.adaptive_interval_ms < 100 {
            return Err(CoreError::InvalidConfig(
                "adaptive_interval_ms must be at least 100".into(),
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn effective_connections(&self, total: u64) -> u8 {
        let by_size = total.div_ceil(self.min_segment_size).max(1);
        self.connections
            .min(self.max_connections)
            .min(u8::try_from(by_size).unwrap_or(u8::MAX))
            .max(1)
    }
}
