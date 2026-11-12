# ADR-0002: Workspace layering

- **Status:** accepted
- **Date:** 2026-11-12

## Context

A download manager mixes pure scheduling logic, protocol I/O, filesystem
durability, persistence, UI and OS integration. Putting these in one crate makes
testing slow and invites UI logic into the engine. The brief suggests a strict
layering with `#![forbid(unsafe_code)]`.

## Decision

Use a Cargo workspace with these dependency directions:

```text
core (pure)  <-- storage <-- net <-- queue <-- ipc <-- cli
     ^                         ^
     +----- media (pure)       +----- testkit (dev)
app (assets only) <-- ipc
native-host (independent)
platform/crawler (independent, future)
```

Rules:

- `rustget-core` has no I/O, no HTTP, no filesystem, no async runtime except the
  rate limiter's timer.
- `rustget-storage` owns files/journal but not networking.
- `rustget-net` combines core + storage into a transfer coordinator.
- `rustget-queue` owns SQLite and lifecycle, not HTTP details.
- `rustget-ipc` is a transport adapter; it never contains download logic.
- UI assets are static data; the UI uses the same API as the CLI.
- Every crate starts with `#![forbid(unsafe_code)]`. A future OS-specific crate
  must be isolated, audited and documented before adding `unsafe`.

## Consequences

- The ledger and journal are testable without sockets or a runtime.
- The manager and API can be tested with a fake/testkit origin.
- Some types cross layers (e.g. `DownloadProgress`), which is acceptable because
  they are plain data.
- More crates and manifests than a single binary; compile time is higher.
- The coordinator lives in `rustget-net` rather than a separate `engine` crate
  (see ADR-0003).
