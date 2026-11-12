//! Pure, deterministic scheduling primitives for RustGet.
//!
//! This crate deliberately knows nothing about HTTP, files or UI. It owns the
//! segment ledger: byte ranges, worker assignment, progress accounting and the
//! work-stealing split rule. Keeping it pure makes the gap/overlap invariants
//! exhaustively testable.
#![forbid(unsafe_code)]

pub mod config;
pub mod error;
pub mod ledger;
pub mod range;
pub mod rate;
pub mod retry;

pub use config::EngineConfig;
pub use error::{CoreError, Result};
pub use ledger::{
    Assignment, LedgerSnapshot, Segment, SegmentId, SegmentLedger, SegmentSnapshot, SegmentState,
    SplitDecision, WorkerId,
};
pub use range::ByteRange;
pub use rate::RateLimiter;
pub use retry::{ErrorClass, RetryPolicy};
