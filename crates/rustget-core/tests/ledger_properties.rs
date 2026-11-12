use proptest::prelude::*;
use rustget_core::{
    ByteRange, EngineConfig, SegmentLedger, SegmentState, WorkerId, range::partition,
};

fn ledger_config() -> EngineConfig {
    EngineConfig {
        connections: 4,
        max_connections: 12,
        min_segment_size: 64,
        ..EngineConfig::default()
    }
}

fn assert_partition(ledger: &SegmentLedger) {
    let mut segments = ledger.segments().to_vec();
    segments.sort_by_key(|s| s.range.start);
    let mut expected = 0_u64;
    for segment in &segments {
        assert_eq!(
            segment.range.start, expected,
            "gap or overlap in segment ranges"
        );
        assert!(segment.written >= segment.range.start);
        assert!(
            segment.written <= segment.range.end + 1,
            "written past end for {segment:?}"
        );
        if segment.state == SegmentState::Complete {
            assert_eq!(segment.written, segment.range.end + 1);
        }
        expected = segment.range.end + 1;
    }
    assert_eq!(expected, ledger.total_size());
}

proptest! {
    #[test]
    fn partition_covers_every_byte(
        total in 1_u64..1_000_000_000_000,
        desired in 1_u8..32,
        min_segment in 1_u64..10_000_000,
    ) {
        let ranges = partition(total, desired, min_segment);
        prop_assert!(!ranges.is_empty());
        prop_assert_eq!(ranges[0].start, 0);
        prop_assert_eq!(ranges.last().unwrap().end, total - 1);
        for pair in ranges.windows(2) {
            prop_assert_eq!(pair[0].end + 1, pair[1].start);
        }
        let sum: u64 = ranges.iter().map(|r| r.len()).sum();
        prop_assert_eq!(sum, total);
    }

    #[test]
    fn split_ranges_are_contiguous(start in 0_u64..1_000_000, length in 2_u64..1_000_000) {
        let range = ByteRange::new(start, start + length - 1).unwrap();
        let (left, right) = range.split().unwrap();
        prop_assert_eq!(left.start, range.start);
        prop_assert_eq!(left.end + 1, right.start);
        prop_assert_eq!(right.end, range.end);
        prop_assert_eq!(left.len() + right.len(), range.len());
    }

    #[test]
    fn random_scheduling_preserves_partition(
        total in 100_u64..2_000_000,
        operations in prop::collection::vec(0_u8..10, 1..300),
    ) {
        let mut ledger = SegmentLedger::new(total, ledger_config()).unwrap();
        let _ = ledger.initial_assignments();
        let mut next_idle = 100_u64;
        for op in operations {
            let active = ledger.active_segments();
            if active.is_empty() {
                break;
            }
            match op % 5 {
                0 => {
                    let (worker, segment_id) = active[(op as usize) % active.len()];
                    let segment = ledger.segment(segment_id).unwrap();
                    let delta = u64::from(op).saturating_mul(17) % (segment.remaining() + 1);
                    ledger.report_progress(worker, segment.written + delta).unwrap();
                }
                1 => {
                    let (worker, _) = active[(op as usize) % active.len()];
                    ledger.finish(worker).unwrap();
                }
                2 => {
                    let (worker, _) = active[(op as usize) % active.len()];
                    let _ = ledger.release(worker);
                }
                3 => {
                    let idle = WorkerId(next_idle);
                    next_idle += 1;
                    if let Ok(Some(_)) = ledger.steal(idle) {
                        // Successful steal adds a worker.
                    }
                }
                _ => {
                    let idle = WorkerId(next_idle);
                    next_idle += 1;
                    let _ = ledger.claim(idle);
                }
            }
            assert_partition(&ledger);
        }
    }
}
