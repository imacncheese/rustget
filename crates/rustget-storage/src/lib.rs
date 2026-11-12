//! Durable file and resume-metadata primitives.
//!
//! The output file is written with positional writes, so segments can complete
//! out of order without locks or a merger pass. A separate append-only journal
//! records the last known-safe cursor for every segment. Recovery takes the
//! last parseable snapshot; bytes written past that point are safely fetched
//! again.
#![forbid(unsafe_code)]

pub mod journal;
pub mod part;
pub mod paths;

pub use journal::{Journal, JournalRecord, JournalState, JournalStore};
pub use part::{PartFile, PreallocateMode};
pub use paths::{
    DownloadPaths, content_disposition_filename, filename_from_url, sanitize_filename,
    unique_destination,
};
