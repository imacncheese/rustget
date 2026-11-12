# Progress

**Updated:** 2026-11-12
**Milestone:** M0 complete; M1–M3 substantially complete; M4/M5 partial

## Done

### M0 — research, specs, scaffold

- [x] Workspace with strict layering and `#![forbid(unsafe_code)]`.
- [x] 147-row feature matrix with priorities, milestones and honest status.
- [x] Black-box observation report with artifact hashes, extension manifest
      facts, 12 hypotheses and 6 reproducible protocol runs.
- [x] Implementation-neutral specs: segmentation, journal, scheduler, browser
      bridge, UI/UX, error taxonomy, settings, API, security, crash safety,
      testing, parity.
- [x] 14 ADRs.
- [x] Instrumented adversarial testkit (no-range, flaky, redirects, auth,
      changing ETag, throttle, slow-first-range).
- [x] UI screenshot captured in a real headless Chromium.

### M1 — engine

- [x] HEAD/`Range: bytes=0-0` probe, 206 validation, identity encoding.
- [x] Dynamic initial partition, positional writes, sparse part file.
- [x] Append-only journal, torn-tail recovery, compaction.
- [x] Resume from journal with ETag/Last-Modified/size validation.
- [x] Single-stream fallback on non-range servers.
- [x] Redirects, mid-stream reset retry, checksum verification.
- [x] CLI `add/list/info/config/batch/expand`.
- [x] Byte-exact integration tests.

### M2 — acceleration and safety

- [x] Work-stealing split with cooperative stop boundary.
- [x] Adaptive connection growth while throughput improves.
- [x] Token-bucket speed limit.
- [x] Exponential backoff with jitter and error classes.
- [x] Process-kill/resume SHA-256 proof.
- [x] Proptest gap/overlap/random-scheduling invariants.

### M3 — queue, API, UI

- [x] SQLite WAL catalogue, queue, priorities, concurrency.
- [x] Categories with extension routing.
- [x] Weekly scheduler windows + scheduler-owned pause marker.
- [x] REST + WebSocket API with token/origin checks.
- [x] Web UI: virtualized list, details, segments, add/settings, themes.
- [x] Queue priority Move up/down via PATCH API.
- [x] Live speed sparkline, URL probe preview, duplicate URL detection and daemon single-instance lock.
- [x] Opt-in scheduler power actions with platform command planning.
- [x] Versioned JSON export/import for downloads, settings, categories and schedules.
- [x] Drag-and-drop http(s) links into the add dialog.
- [x] HLS/DASH quality catalog and variant picker.
- [x] Persisted tags with API/CLI/UI editing and tag search.
- [x] In-app log viewer over a bounded tracing buffer.
- [x] Two-step first-run wizard with token validation.
- [~] English/German UI catalog and language selector (partial, non-Fluent).
- [~] Windows tray shell (`rustget-tray`); startup verified, menu not automated.
- [x] Explicit per-download HTTP proxy with a live proxy fixture.
- [~] Autostart CLI/platform entries; Windows registry writes blocked here.
- [x] Scheduler weekly timeline and add/delete form in the web UI.
- [x] Chromium/Firefox extension + native-messaging host (live tested).
- [x] Opt-in browser-download takeover with options UI (default off).
- [x] HLS/DASH parsers, FFmpeg sidecar boundary.
- [x] HLS VOD download path (master/media, init map, ordered segments).
- [x] DASH VOD download path (duration SegmentTemplate, DRM rejection).
- [x] Mirror/alternate-URL probe fallback and worker failover.
- [x] Process-wide per-host connection semaphore.
- [x] Plain FTP download with REST resume and local fixture.
- [x] Queue priority editing and download-list export/import.
- [x] Criterion ledger benchmarks and 128 MiB aria2 comparison.
- [x] Adaptive concurrency growth test.
- [x] Same-origin crawler with robots.txt and CLI `grab`.
- [x] Category sidebar in the web UI.

## In progress / next

- [ ] Fuzz targets actually run.
- [ ] Tauri/native shell, tray, autostart (single-instance is implemented).
- [ ] HLS AES-128/audio groups, DASH SegmentTimeline/live, parallel media segments.
- [ ] FTPS, Digest, SOCKS5, learned per-host limits.
- [ ] Load test at 10k rows.
- [ ] Accessibility audit.

## Blocked

- **IDM black-box protocol runs**: no licensed IDM installation; we chose not to
  execute the proprietary installer. The experiments are documented and ready.
- **Power-loss proof**: requires a VM/disk with controlled cache flushing; not
  available in this environment.
- **1 Gbps benchmark**: no 1 Gbps link available; the loopback result (152.5
  MiB/s median over 9 runs) is not a WAN saturation claim.
- **Hosted CI evidence**: repository is local-only; workflow exists but has not
  run on GitHub.

## Verification snapshot

```text
rustget-core     14 unit + 3 proptest   PASS
rustget-storage  15                      PASS
rustget-testkit   5                      PASS
rustget-net       7 unit + 8 HTTP + 2 work/adaptive + 2 mirrors + 2 HLS + 2 AES-HLS + 3 DASH + 1 host cap + 1 FTP + 1 proxy  PASS
rustget-queue     16                     PASS
rustget-ipc       6                      PASS
rustget-cli       4 + 1 crash safety     PASS
rustget-media     6                      PASS
rustget-crawler   2                      PASS
rustget-native-host 2                    PASS
rustget-platform  3                     PASS
clippy -D warnings                       PASS
cargo fmt --check                        PASS
total                                  106 tests
```

Commands:

```sh
cargo test --workspace
cargo test -p rustget-net --test work_stealing -- --nocapture
cargo test -p rustget-cli --test crash_safety -- --nocapture
```

## Known cleanup

- `rustget-platform` is a placeholder.
- Fuzz targets compile only with `cargo fuzz`; not yet run in CI.
- The benchmark is loopback-only; no 1 Gbps measurement.
- `cargo-deny`/`cargo-audit` are configured but not installed in this analysis
  environment; CI runs them once hosted.

## Decision log

See `docs/adr/`. Recent decisions:

- ADR-0004: ship a web UI first; Tauri deferred.
- ADR-0005: SQLite catalogue + JSONL journal.
- ADR-0008: cooperative stop boundaries.
- ADR-0010: scheduler-owned pause marker.
- ADR-0014: defer tray/autostart/power execution.
