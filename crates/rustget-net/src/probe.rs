use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Validator {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl Validator {
    /// `If-Range` requires a strong ETag; weak validators and dates use the
    /// Last-Modified value instead.
    #[must_use]
    pub fn if_range(&self) -> Option<&str> {
        if let Some(etag) = &self.etag {
            if !etag.starts_with("W/") {
                return Some(etag);
            }
        }
        self.last_modified.as_deref()
    }

    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        self.etag == other.etag && self.last_modified == other.last_modified
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.etag.is_none() && self.last_modified.is_none()
    }
}

#[derive(Debug, Clone)]
pub struct FileProbe {
    pub requested_url: Url,
    pub final_url: Url,
    pub total_size: Option<u64>,
    pub range_supported: bool,
    pub validator: Validator,
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
    pub filename: String,
    pub http_version: String,
    pub status: u16,
    pub set_cookies: Vec<String>,
}

/// Parse `bytes start-end/total` (total may be `*`).
#[must_use]
pub fn parse_content_range(value: &str) -> Option<(u64, u64, Option<u64>)> {
    let rest = value.strip_prefix("bytes ")?.trim();
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start = start.trim().parse().ok()?;
    let end = end.trim().parse().ok()?;
    let total = if total.trim() == "*" {
        None
    } else {
        Some(total.trim().parse().ok()?)
    };
    if end < start {
        return None;
    }
    Some((start, end, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strong_and_weak_etag_selection() {
        let strong = Validator {
            etag: Some("\"abc\"".into()),
            last_modified: Some("date".into()),
        };
        assert_eq!(strong.if_range(), Some("\"abc\""));
        let weak = Validator {
            etag: Some("W/\"abc\"".into()),
            last_modified: Some("date".into()),
        };
        assert_eq!(weak.if_range(), Some("date"));
    }

    #[test]
    fn content_ranges() {
        assert_eq!(
            parse_content_range("bytes 10-19/100"),
            Some((10, 19, Some(100)))
        );
        assert_eq!(parse_content_range("bytes 0-0/*"), Some((0, 0, None)));
        assert_eq!(parse_content_range("bytes 10-9/100"), None);
    }
}
