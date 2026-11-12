//! Persistent download management.
//!
//! The manager is deliberately UI-agnostic: CLI, HTTP API and desktop shell all
//! observe the same SQLite rows and broadcast events.
#![forbid(unsafe_code)]

pub mod db;
pub mod manager;
pub mod model;
pub mod scheduler;

pub use db::Database;
pub use manager::{DownloadManager, ManagerConfig, ManagerEvent, NewDownload};
pub use model::{Category, DownloadRecord, DownloadStatus};
pub use scheduler::{PowerAction, Schedule};
