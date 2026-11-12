use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use rustget_core::{ByteRange, EngineConfig, SegmentLedger, range::partition};

fn config() -> EngineConfig {
    EngineConfig {
        connections: 16,
        max_connections: 32,
        min_segment_size: 256 * 1024,
        ..EngineConfig::default()
    }
}

fn bench_partition(c: &mut Criterion) {
    c.bench_function("partition_1TiB_16", |b| {
        b.iter(|| partition(black_box(1 << 40), 16, 256 * 1024));
    });
}

fn bench_split(c: &mut Criterion) {
    let range = ByteRange::new(0, (1 << 30) - 1).unwrap();
    c.bench_function("split_1GiB_range", |b| {
        b.iter(|| range.split().unwrap());
    });
}

fn bench_ledger_schedule(c: &mut Criterion) {
    let mut group = c.benchmark_group("ledger");
    group.bench_function("create_1TiB", |b| {
        b.iter(|| SegmentLedger::new(black_box(1 << 40), config()).unwrap());
    });
    group.finish();
}

criterion_group!(benches, bench_partition, bench_split, bench_ledger_schedule);
criterion_main!(benches);
