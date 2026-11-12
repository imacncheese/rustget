//! Media manifest parsing and optional FFmpeg muxing.
//!
//! Parsing is pure and testable. Muxing is an explicit sidecar boundary: RustGet
//! never bundles FFmpeg and never executes a URL; it only invokes a verified
//! local binary with a fixed argument vector.
#![forbid(unsafe_code)]

pub mod dash;
pub mod ffmpeg;
pub mod hls;

pub use dash::{DashManifest, DashRepresentation, parse_mpd};
pub use ffmpeg::{Ffmpeg, find_ffmpeg};
pub use hls::{HlsPlaylist, HlsVariant, MediaPlaylist, Segment, parse_hls};
