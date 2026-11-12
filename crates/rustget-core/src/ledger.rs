use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::EngineConfig;
use crate::error::{CoreError, Result};
use crate::range::{ByteRange, partition};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SegmentId(pub u64);

impl std::fmt::Display for SegmentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkerId(pub u64);

impl std::fmt::Display for WorkerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentState {
    Pending,
    Active,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub id: SegmentId,
    pub range: ByteRange,
    /// Absolute offset of the next byte to write. Never decreases.
    pub written: u64,
    pub state: SegmentState,
    /// Number of times this segment was handed to a worker.
    pub attempts: u32,
}

impl Segment {
    #[must_use]
    pub const fn remaining(&self) -> u64 {
        if self.written > self.range.end {
            0
        } else {
            self.range.end - self.written + 1
        }
    }

    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.written > self.range.end
    }
}

#[derive(Debug, Clone)]
pub struct Assignment {
    pub worker: WorkerId,
    pub segment: SegmentId,
    /// Effective request range, begins at the durable written offset.
    pub range: ByteRange,
    pub attempt: u32,
}

#[derive(Debug, Clone)]
pub struct SplitDecision {
    pub victim: WorkerId,
    pub idle_worker: WorkerId,
    pub stop_at: u64,
    /// The victim's new (truncated) inclusive end.
    pub victim_end: u64,
    pub assignment: Assignment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentSnapshot {
    pub id: SegmentId,
    pub start: u64,
    pub end: u64,
    pub written: u64,
    pub state: SegmentState,
    pub attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerSnapshot {
    pub total_size: u64,
    pub segments: Vec<SegmentSnapshot>,
}

/// Authoritative, single-threaded state machine for one download's segments.
///
/// The async coordinator is the only mutator. Workers observe progress through
/// their own atomics and report snapshots back; the ledger never performs I/O.
#[derive(Debug, Clone)]
pub struct SegmentLedger {
    total_size: u64,
    config: EngineConfig,
    segments: Vec<Segment>,
    active: BTreeMap<WorkerId, SegmentId>,
    next_id: u64,
}

impl SegmentLedger {
    pub fn new(total_size: u64, config: EngineConfig) -> Result<Self> {
        config.validate()?;
        let ranges = partition(
            total_size,
            config.effective_connections(total_size),
            config.min_segment_size,
        );
        Self::from_ranges(total_size, ranges, config)
    }

    pub fn from_ranges(
        total_size: u64,
        ranges: Vec<ByteRange>,
        config: EngineConfig,
    ) -> Result<Self> {
        config.validate()?;
        if total_size == 0 {
            if !ranges.is_empty() {
                return Err(CoreError::InvalidConfig(
                    "zero-length download cannot have segments".into(),
                ));
            }
        } else {
            validate_coverage(total_size, &ranges)?;
        }
        let segments: Vec<Segment> = ranges
            .into_iter()
            .enumerate()
            .map(|(index, range)| Segment {
                id: SegmentId(index as u64),
                written: range.start,
                range,
                state: SegmentState::Pending,
                attempts: 0,
            })
            .collect();
        let next_id = segments.len() as u64;
        Ok(Self {
            total_size,
            config,
            segments,
            active: BTreeMap::new(),
            next_id,
        })
    }

    pub fn from_snapshot(snapshot: LedgerSnapshot, config: EngineConfig) -> Result<Self> {
        config.validate()?;
        if snapshot.total_size == 0 {
            if !snapshot.segments.is_empty() {
                return Err(CoreError::InvalidConfig(
                    "zero-length snapshot cannot have segments".into(),
                ));
            }
            return Self::new(0, config);
        }
        if snapshot.segments.is_empty() {
            return Err(CoreError::InvalidConfig("snapshot has no segments".into()));
        }
        let mut seen_ids = std::collections::BTreeSet::new();
        let mut segments = Vec::with_capacity(snapshot.segments.len());
        let mut expected_start = 0_u64;
        let mut sorted = snapshot.segments;
        sorted.sort_by_key(|s| s.start);
        for item in sorted {
            if !seen_ids.insert(item.id) {
                return Err(CoreError::InvalidConfig(format!(
                    "duplicate segment id {}",
                    item.id
                )));
            }
            if item.end < item.start || item.end >= snapshot.total_size {
                return Err(CoreError::InvalidRange(format!(
                    "segment [{}, {}] outside total {}",
                    item.start, item.end, snapshot.total_size
                )));
            }
            if item.start != expected_start {
                return Err(CoreError::InvalidConfig(format!(
                    "snapshot gap at byte {expected_start}; next segment starts at {}",
                    item.start
                )));
            }
            if item.written < item.start || item.written > item.end + 1 {
                return Err(CoreError::InvalidRange(format!(
                    "written {} outside segment [{}, {}]",
                    item.written, item.start, item.end
                )));
            }
            let state = if item.state == SegmentState::Complete || item.written > item.end {
                SegmentState::Complete
            } else {
                SegmentState::Pending
            };
            let written = if state == SegmentState::Complete {
                item.end + 1
            } else {
                item.written
            };
            segments.push(Segment {
                id: item.id,
                range: ByteRange {
                    start: item.start,
                    end: item.end,
                },
                written,
                state,
                attempts: item.attempts,
            });
            expected_start = item.end + 1;
        }
        if expected_start != snapshot.total_size {
            return Err(CoreError::InvalidConfig(format!(
                "snapshot ends at {expected_start}, expected {}",
                snapshot.total_size
            )));
        }
        let next_id = segments.iter().map(|s| s.id.0 + 1).max().unwrap_or(0);
        Ok(Self {
            total_size: snapshot.total_size,
            config,
            segments,
            active: BTreeMap::new(),
            next_id,
        })
    }

    #[must_use]
    pub const fn total_size(&self) -> u64 {
        self.total_size
    }

    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    #[must_use]
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|s| s.state == SegmentState::Pending)
            .count()
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.total_size == 0
            || self
                .segments
                .iter()
                .all(|s| s.state == SegmentState::Complete)
    }

    /// Bytes whose writes have been reported for completed or active segments.
    #[must_use]
    pub fn downloaded_bytes(&self) -> u64 {
        self.segments
            .iter()
            .map(|s| s.written.saturating_sub(s.range.start))
            .sum()
    }

    #[must_use]
    pub fn progress_ratio(&self) -> f64 {
        if self.total_size == 0 {
            1.0
        } else {
            self.downloaded_bytes() as f64 / self.total_size as f64
        }
    }

    /// Assign pending ranges to `count` new workers, up to the configured
    /// initial connection count.
    pub fn initial_assignments(&mut self) -> Vec<Assignment> {
        let mut out = Vec::new();
        for worker_index in 0..u64::from(self.config.connections) {
            if let Some(assignment) = self.claim(WorkerId(worker_index)) {
                out.push(assignment);
            }
        }
        out
    }

    /// Claim the first pending segment for `worker`.
    pub fn claim(&mut self, worker: WorkerId) -> Option<Assignment> {
        if self.active.contains_key(&worker) {
            return None;
        }
        let index = self
            .segments
            .iter()
            .position(|s| s.state == SegmentState::Pending)?;
        let segment = &mut self.segments[index];
        segment.state = SegmentState::Active;
        segment.attempts = segment.attempts.saturating_add(1);
        let assignment = Assignment {
            worker,
            segment: segment.id,
            range: ByteRange {
                start: segment.written,
                end: segment.range.end,
            },
            attempt: segment.attempts,
        };
        self.active.insert(worker, segment.id);
        Some(assignment)
    }

    /// Report the absolute exclusive write cursor for a worker's segment.
    pub fn report_progress(&mut self, worker: WorkerId, absolute: u64) -> Result<()> {
        let segment_id = *self
            .active
            .get(&worker)
            .ok_or(CoreError::UnknownWorker(worker.0))?;
        let segment = self.segment_mut(segment_id)?;
        if absolute < segment.range.start || absolute > segment.range.end + 1 {
            return Err(CoreError::ProgressOutOfRange {
                progress: absolute,
                start: segment.range.start,
                end: segment.range.end,
            });
        }
        if absolute > segment.written {
            segment.written = absolute;
        }
        Ok(())
    }

    /// Finish a worker's range. Any completed segment becomes immutable.
    pub fn finish(&mut self, worker: WorkerId) -> Result<SegmentId> {
        let segment_id = self
            .active
            .remove(&worker)
            .ok_or(CoreError::UnknownWorker(worker.0))?;
        let segment = self.segment_mut(segment_id)?;
        segment.written = segment.range.end + 1;
        segment.state = SegmentState::Complete;
        Ok(segment_id)
    }

    /// Release a worker without completing its segment (error, pause, shutdown).
    /// The durable progress is retained and the range returns to the queue.
    pub fn release(&mut self, worker: WorkerId) -> Result<SegmentId> {
        let segment_id = self
            .active
            .remove(&worker)
            .ok_or(CoreError::UnknownWorker(worker.0))?;
        let segment = self.segment_mut(segment_id)?;
        if segment.written > segment.range.end {
            segment.state = SegmentState::Complete;
        } else {
            segment.state = SegmentState::Pending;
        }
        Ok(segment_id)
    }

    /// Pick the active segment with the most bytes remaining. Ties prefer the
    /// lower segment id, making tests and journal diffs deterministic.
    #[must_use]
    pub fn choose_victim(&self) -> Option<WorkerId> {
        self.active
            .iter()
            .filter_map(|(worker, id)| {
                let segment = self.segments.iter().find(|s| s.id == *id)?;
                (segment.remaining() > 0).then_some((*worker, segment.remaining(), *id))
            })
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.2.cmp(&a.2)))
            .map(|(worker, _, _)| worker)
    }

    /// Split an active segment so an idle worker can help. The victim's range
    /// is truncated immediately and a new contiguous range is assigned to
    /// `idle`. The caller must publish `stop_at` to the victim before its next
    /// write; the ledger never assumes the victim has stopped yet.
    pub fn try_split(&mut self, victim: WorkerId, idle: WorkerId) -> Result<Option<SplitDecision>> {
        if self.active.contains_key(&idle) {
            return Err(CoreError::WorkerBusy { worker: idle.0 });
        }
        if self.active.len() >= usize::from(self.config.max_connections) {
            return Ok(None);
        }
        let Some(&victim_segment_id) = self.active.get(&victim) else {
            return Err(CoreError::UnknownWorker(victim.0));
        };
        let index = self
            .segments
            .iter()
            .position(|s| s.id == victim_segment_id)
            .ok_or(CoreError::UnknownSegment(victim_segment_id.0))?;
        let remaining = self.segments[index].remaining();
        if remaining < self.config.min_segment_size.saturating_mul(2) {
            return Ok(None);
        }
        let written = self.segments[index].written;
        let old_end = self.segments[index].range.end;
        let stop_at = written + remaining / 2;
        // Both halves must be at least the configured minimum.
        if stop_at <= written || old_end.saturating_sub(stop_at) + 1 < self.config.min_segment_size
        {
            return Ok(None);
        }
        self.segments[index].range.end = stop_at - 1;
        let new_id = SegmentId(self.next_id);
        self.next_id += 1;
        let new_segment = Segment {
            id: new_id,
            range: ByteRange {
                start: stop_at,
                end: old_end,
            },
            written: stop_at,
            state: SegmentState::Active,
            attempts: 1,
        };
        self.segments.push(new_segment);
        self.segments.sort_by_key(|s| s.range.start);
        self.active.insert(idle, new_id);
        let attempt = self
            .segments
            .iter()
            .find(|s| s.id == new_id)
            .map_or(1, |s| s.attempts);
        Ok(Some(SplitDecision {
            victim,
            idle_worker: idle,
            stop_at,
            victim_end: stop_at - 1,
            assignment: Assignment {
                worker: idle,
                segment: new_id,
                range: ByteRange {
                    start: stop_at,
                    end: old_end,
                },
                attempt,
            },
        }))
    }

    /// Convenience for a coordinator with an idle worker: choose a victim and
    /// split it. Returns `None` when no useful split exists.
    pub fn steal(&mut self, idle: WorkerId) -> Result<Option<SplitDecision>> {
        let Some(victim) = self.choose_victim() else {
            return Ok(None);
        };
        if victim == idle {
            return Ok(None);
        }
        self.try_split(victim, idle)
    }

    #[must_use]
    pub fn snapshot(&self) -> LedgerSnapshot {
        LedgerSnapshot {
            total_size: self.total_size,
            segments: self
                .segments
                .iter()
                .map(|s| SegmentSnapshot {
                    id: s.id,
                    start: s.range.start,
                    end: s.range.end,
                    written: s.written,
                    state: s.state,
                    attempts: s.attempts,
                })
                .collect(),
        }
    }

    #[must_use]
    pub fn segment(&self, id: SegmentId) -> Option<&Segment> {
        self.segments.iter().find(|s| s.id == id)
    }

    fn segment_mut(&mut self, id: SegmentId) -> Result<&mut Segment> {
        self.segments
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(CoreError::UnknownSegment(id.0))
    }

    #[must_use]
    pub fn active_segments(&self) -> Vec<(WorkerId, SegmentId)> {
        self.active.iter().map(|(w, s)| (*w, *s)).collect()
    }
}

fn validate_coverage(total: u64, ranges: &[ByteRange]) -> Result<()> {
    if ranges.is_empty() {
        return Err(CoreError::InvalidRange(
            "no ranges for non-empty file".into(),
        ));
    }
    let mut expected = 0_u64;
    for range in ranges {
        if range.start != expected {
            return Err(CoreError::InvalidRange(format!(
                "range starts at {}, expected {expected}",
                range.start
            )));
        }
        if range.end >= total {
            return Err(CoreError::InvalidRange(format!(
                "range end {} exceeds total {total}",
                range.end
            )));
        }
        expected = range.end + 1;
    }
    if expected != total {
        return Err(CoreError::InvalidRange(format!(
            "ranges cover {expected} bytes, expected {total}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> EngineConfig {
        EngineConfig {
            connections: 4,
            max_connections: 8,
            min_segment_size: 100,
            ..EngineConfig::default()
        }
    }

    #[test]
    fn initial_assignment_covers_without_overlap() {
        let mut ledger = SegmentLedger::new(10_000, config()).unwrap();
        let assigned = ledger.initial_assignments();
        assert_eq!(assigned.len(), 4);
        assert_eq!(ledger.active_count(), 4);
        for pair in assigned.windows(2) {
            assert_eq!(pair[0].range.end + 1, pair[1].range.start);
        }
        assert_eq!(assigned[0].range.start, 0);
        assert_eq!(assigned.last().unwrap().range.end, 9_999);
    }

    #[test]
    fn split_truncates_victim_and_assigns_disjoint_half() {
        let mut ledger = SegmentLedger::new(10_000, config()).unwrap();
        let assigned = ledger.initial_assignments();
        ledger.report_progress(assigned[0].worker, 600).unwrap();
        let split = ledger
            .try_split(assigned[0].worker, WorkerId(99))
            .unwrap()
            .expect("splittable");
        assert_eq!(split.stop_at, 600 + (2_500 - 600) / 2);
        assert_eq!(split.assignment.range.start, split.stop_at);
        assert_eq!(split.assignment.range.end, assigned[0].range.end);
        let victim = ledger.segment(assigned[0].segment).unwrap();
        assert_eq!(victim.range.end, split.stop_at - 1);
        assert!(victim.written <= split.stop_at);
        let fresh = ledger.segment(split.assignment.segment).unwrap();
        assert_eq!(fresh.range.start, split.stop_at);
        assert_eq!(fresh.state, SegmentState::Active);
    }

    #[test]
    fn split_respects_minimum_segment_size() {
        let mut cfg = config();
        cfg.min_segment_size = 1_000;
        let mut ledger = SegmentLedger::new(10_000, cfg).unwrap();
        let assigned = ledger.initial_assignments();
        ledger.report_progress(assigned[0].worker, 2_400).unwrap();
        // 100 bytes remain of this 2,500-byte segment: too small to split.
        assert!(
            ledger
                .try_split(assigned[0].worker, WorkerId(99))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn finish_and_release_require_active_worker() {
        let mut ledger = SegmentLedger::new(1_000, config()).unwrap();
        let assigned = ledger.initial_assignments();
        let worker = assigned[0].worker;
        ledger.finish(worker).unwrap();
        assert!(ledger.finish(worker).is_err());
        let other = assigned[1].worker;
        ledger
            .report_progress(other, assigned[1].range.start + 10)
            .unwrap();
        ledger.release(other).unwrap();
        let segment = ledger.segment(assigned[1].segment).unwrap();
        assert_eq!(segment.state, SegmentState::Pending);
        assert_eq!(segment.written, assigned[1].range.start + 10);
        assert!(segment.remaining() > 0);
    }

    #[test]
    fn snapshot_round_trip_resumes_active_as_pending() {
        let mut ledger = SegmentLedger::new(10_000, config()).unwrap();
        let assigned = ledger.initial_assignments();
        ledger.report_progress(assigned[0].worker, 1234).unwrap();
        let snap = ledger.snapshot();
        let resumed = SegmentLedger::from_snapshot(snap, config()).unwrap();
        assert_eq!(resumed.total_size(), 10_000);
        assert_eq!(resumed.active_count(), 0);
        assert_eq!(resumed.downloaded_bytes(), 1234);
        assert_eq!(resumed.segments().len(), 4);
    }
}
