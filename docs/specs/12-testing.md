# Testing and QA strategy

**Status:** implemented layers; CI not yet exercised on a hosted repository

## 1. Test pyramid

```text
             /\
            /  \      end-to-end CLI + daemon + browser
           /____\
          /      \    integration: engine + testkit + API
         /________\
        /          \  property/unit: ledger, journal, parsers, paths
       /____________\
```

The pure ledger and journal are the highest-value unit targets because a bug
there corrupts output. The testkit makes protocol behaviour reproducible without
the public internet.

## 2. Inventory

| Crate | Tests | What they prove |
|---|---:|---|
| `rustget-core` | 14 unit + 3 proptest | partition, split, scheduling, rate, retry |
| `rustget-storage` | 15 | journal recovery, preallocation, positional writes, paths |
| `rustget-testkit` | 5 | range parsing, 206, no-range, flaky, redirect |
| `rustget-net` | 7 unit + 8 HTTP + 2 work/adaptive + 2 mirrors + 2 HLS + 2 encrypted HLS + 3 DASH + 1 host cap + 1 FTP | probe/checksum, host semaphore, download, fallback, redirects, retries, validator, resume, work stealing, adaptive growth, mirror failover, host caps, HLS quality/AES-128, DASH timeline, FTP |
| `rustget-queue` | 16 | DB migration/persistence, queue order, interruption, duplicate URL, tags, manager completion, scheduler, power hook, category/HLS/FTP routing, mirror probe |
| `rustget-ipc` | 6 | bearer auth, UI serving, add/list/delete, probe/config bundles, logs, origin and Host rejection |
| `rustget-cli` | 4 + 1 | pattern expansion, kill/resume |
| `rustget-media` | 6 | HLS master/media/encryption, DASH duration/timeline, FFmpeg discovery |
| `rustget-crawler` | 2 | robots parsing, origin/extension filters |
| `rustget-native-host` | 2 | framing, oversize rejection |
| `rustget-platform` | 3 | single-instance lock, power/autostart command construction |
| **Total** | **105** | |

## 3. Commands

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p rustget-net --test work_stealing -- --nocapture
cargo test -p rustget-cli --test crash_safety -- --nocapture
```

## 4. Adversarial origin

`rustget-testkit` can run standalone:

```sh
cargo run -p rustget-testkit -- --size 1073741824 --name big.bin
```

Modes: honest ranges, ignore ranges, lie about ranges, flaky resets, throttle,
redirect chains, basic auth, changing ETag, missing length, slow first range,
content disposition, HEAD delay.

Its request transcript is available at `/__admin/requests`; tests assert on
status, range starts and request counts.

## 5. Property tests

- `partition_covers_every_byte` over total (1..10^12), connections (1..32), min
  segment (1..10^7).
- `split_ranges_are_contiguous`.
- `random_scheduling_preserves_partition` performs up to 300 random
  progress/finish/release/steal/claim operations and asserts the partition after
  every step.

These caught nothing in the current code but are the guardrail for future
work-stealing changes.

## 6. Fault injection

Current:

- mid-stream reset (testkit);
- changed ETag;
- no-range server;
- cancellation;
- process kill (CLI test).

Planned:

- disk full (small RAM disk or filesystem quota);
- network switch/IP change (virtual adapter reset);
- DNS failure and captive portal;
- slow-loris;
- server closes TLS mid-body;
- HTTP/2 GOAWAY;
- 429/503 storms;
- path-too-long and reserved names;
- 10k queue load;
- worker panic.

## 7. Fuzzing

Planned targets under `fuzz/`:

| Target | Input |
|---|---|
| `hls_playlist` | arbitrary UTF-8 |
| `dash_mpd` | arbitrary XML |
| `content_range` | arbitrary header strings |
| `native_frame` | arbitrary bytes |
| `pattern_expand` | arbitrary bracket strings |

Fuzz runs are not part of the current CI budget. A nightly job should run each
target for 10 minutes and upload crashes.

## 8. Benchmarks

Planned/partially implemented:

- `criterion` ledger split/partition microbenchmark.
- End-to-end localhost throughput with the testkit.
- Comparison with `aria2c -x16 -s16` and `curl` on the same origin.
- 1 GiB and 8 GiB files, warm/cold cache.
- Memory ceiling measured with a 64 GiB sparse file.

No benchmark result is claimed as final until the script and environment are
committed and reproducible.

## 9. CI

`.github/workflows/ci.yml`:

- matrix: `windows-latest`, `ubuntu-latest`, `macos-latest`;
- `cargo fmt --check`;
- `cargo clippy --all-targets -- -D warnings`;
- `cargo test --workspace`;
- `cargo deny check`;
- `cargo audit` (best effort; advisory DB network).
- caching via `Swatinem/rust-cache`.

Not yet executed because the repository is not hosted. The local Windows run is
the evidence for this report.

## 10. Release gates

A release is blocked by:

1. all tests green on three OSes;
2. clippy with `-D warnings`;
3. no high/critical advisories without a documented exception;
4. crash-safety test green;
5. installer smoke test;
6. checksum/signature of artifacts published.

## 11. Known test weaknesses

- The engine tests use `reqwest` against a loopback axum server; they do not
  exercise real TLS or HTTP/2.
- The UI is syntax-checked and screenshotted, not driven by a browser test
  harness.
- The scheduler daemon loop lacks a clock-injection test.
- The CLI crash test uses `TerminateProcess`; POSIX `SIGKILL` is not run here.
- No load test at 10k rows/queued downloads.
- No fuzzing yet.

These weaknesses are listed rather than hidden behind “all tests pass”.
