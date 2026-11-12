//! HTTP transfer engine.
//!
//! Architecture note: this crate owns the async coordinator that binds the
//! pure `rustget-core` ledger to `reqwest` bodies and `rustget-storage`
//! positional writes. Keeping the coordinator here avoids a cyclic dependency
//! on a separate engine crate; see ADR-0003.
#![forbid(unsafe_code)]

pub mod checksum;
pub mod client;
pub mod engine;
pub mod error;
pub mod host;
pub mod probe;

pub use checksum::{ChecksumAlgorithm, ChecksumSpec, verify_checksum};
pub use client::{NetClient, RequestOptions};
pub use engine::{
    DownloadProgress, DownloadRequest, DownloadState, DownloadSummary, Downloader, ProgressHandle,
    SegmentProgress,
};
pub use error::{NetError, Result};
pub use host::HostLimiter;
pub use probe::{FileProbe, Validator};
