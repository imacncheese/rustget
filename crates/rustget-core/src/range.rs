use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// Inclusive byte range. `end` is the last byte, matching HTTP Range
/// semantics, not an exclusive cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

impl ByteRange {
    pub fn new(start: u64, end: u64) -> Result<Self> {
        if end < start {
            return Err(CoreError::InvalidRange(format!(
                "end {end} precedes start {start}"
            )));
        }
        Ok(Self { start, end })
    }

    #[must_use]
    pub const fn len(self) -> u64 {
        self.end - self.start + 1
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        false
    }

    #[must_use]
    pub const fn contains(self, offset: u64) -> bool {
        offset >= self.start && offset <= self.end
    }

    /// Split into contiguous halves. The first half is larger when odd.
    pub fn split(self) -> Result<(Self, Self)> {
        if self.len() < 2 {
            return Err(CoreError::NotSplittable {
                remaining: self.len(),
            });
        }
        let left_len = self.len().div_ceil(2);
        let mid = self.start + left_len - 1;
        Ok((Self::new(self.start, mid)?, Self::new(mid + 1, self.end)?))
    }

    /// HTTP header value for this range (`bytes=start-end`).
    #[must_use]
    pub fn http_header(self) -> String {
        format!("bytes={}-{}", self.start, self.end)
    }
}

/// Divide `total` bytes into at most `desired` contiguous ranges.
///
/// Every byte in `0..total` is covered exactly once. If the file is smaller
/// than `min_segment`, a single range is returned. The final range absorbs the
/// remainder so all ranges differ by at most one byte.
pub fn partition(total: u64, desired: u8, min_segment: u64) -> Vec<ByteRange> {
    if total == 0 {
        return Vec::new();
    }
    let by_size = total.div_ceil(min_segment.max(1)).max(1);
    let count = u64::from(desired.max(1)).min(by_size).min(total);
    let base = total / count;
    let extra = total % count;
    let mut ranges = Vec::with_capacity(usize::try_from(count).unwrap_or(0));
    let mut offset = 0_u64;
    for index in 0..count {
        let len = base + u64::from(index < extra);
        ranges.push(ByteRange {
            start: offset,
            end: offset + len - 1,
        });
        offset += len;
    }
    debug_assert_eq!(offset, total);
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_length_has_no_ranges() {
        assert!(partition(0, 8, 1).is_empty());
    }

    #[test]
    fn tiny_file_uses_one_range() {
        assert_eq!(partition(10, 8, 1024), vec![ByteRange { start: 0, end: 9 }]);
    }

    #[test]
    fn partition_is_contiguous() {
        for total in [1, 2, 7, 1024, 1_000_001] {
            for desired in [1, 2, 3, 8, 32] {
                let ranges = partition(total, desired, 1);
                assert_eq!(ranges.first().unwrap().start, 0);
                assert_eq!(ranges.last().unwrap().end, total - 1);
                for pair in ranges.windows(2) {
                    assert_eq!(pair[0].end + 1, pair[1].start);
                }
            }
        }
    }
}
