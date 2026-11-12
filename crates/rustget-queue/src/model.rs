use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Queued,
    Probing,
    Downloading,
    Finalizing,
    Paused,
    Completed,
    Failed,
    Removed,
}

impl DownloadStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Probing => "probing",
            Self::Downloading => "downloading",
            Self::Finalizing => "finalizing",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Removed => "removed",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "queued" => Self::Queued,
            "probing" => Self::Probing,
            "downloading" => Self::Downloading,
            "finalizing" => Self::Finalizing,
            "paused" => Self::Paused,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "removed" => Self::Removed,
            _ => Self::Failed,
        }
    }

    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Probing | Self::Downloading | Self::Finalizing)
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Removed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRecord {
    pub id: Uuid,
    pub url: String,
    #[serde(default)]
    pub mirrors: Vec<String>,
    pub final_url: Option<String>,
    pub destination: PathBuf,
    pub filename: String,
    pub category: String,
    pub status: DownloadStatus,
    pub total_size: Option<u64>,
    pub downloaded: u64,
    pub speed_bps: f64,
    pub connections: u8,
    pub speed_limit: Option<u64>,
    pub headers: Vec<(String, String)>,
    pub checksum_algorithm: Option<String>,
    pub checksum_value: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub error: Option<String>,
    pub priority: i32,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub completed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    pub directory: PathBuf,
    pub extensions: Vec<String>,
}

impl DownloadRecord {
    #[must_use]
    pub fn progress_ratio(&self) -> f64 {
        match self.total_size {
            Some(total) if total > 0 => (self.downloaded as f64 / total as f64).clamp(0.0, 1.0),
            Some(_) => 1.0,
            None => 0.0,
        }
    }
}
