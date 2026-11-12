use rustget_core::ErrorClass;
use thiserror::Error;

pub type Result<T, E = NetError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum NetError {
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("network error: {0}")]
    Request(#[from] reqwest::Error),
    #[error("HTTP {status} for {url}: {message}")]
    Status {
        status: u16,
        url: String,
        message: String,
        class: ErrorClass,
    },
    #[error("server applied Content-Encoding '{0}'; byte-exact download is not possible")]
    ContentEncoding(String),
    #[error("server does not support byte ranges for this resource")]
    RangeUnsupported,
    #[error("remote resource changed during download ({0})")]
    RemoteChanged(String),
    #[error("unexpected Content-Range '{0}', expected start {1}")]
    ContentRangeMismatch(String, u64),
    #[error("response ended {0} bytes early")]
    ShortRead(u64),
    #[error("disk I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid response header: {0}")]
    Header(String),
    #[error("download cancelled")]
    Cancelled,
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("unsupported checksum algorithm")]
    UnsupportedChecksum,
    #[error("destination already exists: {0}")]
    DestinationExists(String),
    #[error("insufficient disk space: need {needed} bytes, {available} available")]
    InsufficientSpace { needed: u64, available: u64 },
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("FTP error: {0}")]
    Ftp(String),
    #[error("invalid configuration: {0}")]
    Config(String),
}

impl NetError {
    #[must_use]
    pub fn class(&self) -> ErrorClass {
        match self {
            Self::Request(error) if error.is_timeout() || error.is_connect() => {
                ErrorClass::Transient
            }
            Self::Request(_) => ErrorClass::Transient,
            Self::Status { class, .. } => *class,
            Self::ShortRead(_) | Self::Io(_) => ErrorClass::Transient,
            Self::Ftp(message) if message.contains("530") || message.contains("login") => {
                ErrorClass::Auth
            }
            Self::Ftp(_) => ErrorClass::Transient,
            Self::RangeUnsupported => ErrorClass::RangeUnsupported,
            Self::RemoteChanged(_) | Self::ContentRangeMismatch(..) => ErrorClass::RangeChanged,
            Self::Cancelled => ErrorClass::Fatal,
            Self::ContentEncoding(_) => ErrorClass::Fatal,
            Self::InvalidUrl(_)
            | Self::Header(_)
            | Self::ChecksumMismatch { .. }
            | Self::UnsupportedChecksum
            | Self::DestinationExists(_)
            | Self::InsufficientSpace { .. }
            | Self::Protocol(_)
            | Self::Config(_) => ErrorClass::Fatal,
        }
    }

    #[must_use]
    pub fn status(status: u16, url: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Status {
            status,
            url: url.into(),
            message: message.into(),
            class: rustget_core::RetryPolicy::classify_status(status),
        }
    }
}

impl From<NetError> for std::io::Error {
    fn from(error: NetError) -> Self {
        match error {
            NetError::Io(error) => error,
            other => Self::other(other),
        }
    }
}
