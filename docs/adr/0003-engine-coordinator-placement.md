# ADR-0003: Engine coordinator placement

- **Status:** accepted
- **Date:** 2026-11-12

## Context

The brief lists `core-engine` as the home of segmentation, scheduling and
work-stealing. In practice the coordinator must drive HTTP bodies, positional
writes, cancellation and journal checkpoints. Putting all of that in the pure
core would pull `reqwest`, `tokio` and storage into the crate whose value is
being pure and property-testable. A separate `rustget-engine` crate would create
a fourth layer between net/storage and queue with little benefit.

## Decision

- Keep the **algorithm and state machine** in `rustget-core` (`SegmentLedger`).
- Keep the **async coordinator** in `rustget-net/src/engine.rs`, where it can own
  workers, HTTP responses and file writes.
- Do not create a separate engine crate yet.

The public boundary is `Downloader::download`, `DownloadRequest` and
`DownloadProgress`; callers do not see worker tasks.

## Consequences

- `rustget-core` remains free of I/O and fast to test.
- `rustget-net` is larger and contains both protocol and orchestration.
- If a second transport (FTP, BitTorrent) is added, the coordinator should be
  extracted behind a trait; that extraction is deferred until there is a second
  implementation, not done speculatively.
- The dependency graph remains acyclic.
