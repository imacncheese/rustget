use thiserror::Error;

pub type Result<T, E = CoreError> = std::result::Result<T, E>;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CoreError {
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("invalid byte range: {0}")]
    InvalidRange(String),
    #[error("unknown segment {0}")]
    UnknownSegment(u64),
    #[error("unknown worker {0}")]
    UnknownWorker(u64),
    #[error("worker {worker} is already assigned")]
    WorkerBusy { worker: u64 },
    #[error("progress {progress} is outside segment [{start}, {end}]")]
    ProgressOutOfRange { progress: u64, start: u64, end: u64 },
    #[error("cannot split a segment with {remaining} bytes remaining")]
    NotSplittable { remaining: u64 },
}
