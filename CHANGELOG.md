# Changelog

All notable changes to this project are documented here. The format follows
Keep a Changelog and the project uses Semantic Versioning.

## [0.1.0] - 2026-11-12

### Added

- Multi-segment HTTP(S) download engine with dynamic work stealing.
- Cooperative stop boundaries for safe range splits.
- Adaptive connection growth and pause-all/resume-all.
- Crash-safe `.part` + append-only JSONL resume journal.
- Resume validation via ETag, Last-Modified, size and If-Range.
- Single-stream fallback for servers that ignore ranges.
- Mirror/alternate-URL probe fallback and per-worker failover.
- Process-wide per-host connection semaphore.
- Retry with exponential backoff and jitter; error classification.
- Token-bucket speed limiting.
- MD5/SHA-1/SHA-256/BLAKE3 checksum verification.
- SQLite WAL catalogue, queues, priorities and concurrency limits.
- Add-dialog URL probe preview (filename, size, range support).
- Duplicate-URL handling: completed URLs return 409, interrupted duplicates
  resume the existing row, `--allow-duplicate` overrides.
- Live speed sparkline in the details pane.
- Opt-in scheduler power actions (`--allow-power-actions`) with tested command
  planning and a fake-hook test.
- Cross-process single-instance lock for `rustget serve`.
- Priority editing via API and the details pane (Move up/down).
- Drag an http(s) link onto the window to prefill the add dialog.
- Portable JSON export/import for downloads, settings, categories and schedules,
  exposed through the API, CLI and Settings dialog.
- Weekly scheduler windows with scheduler-owned pauses.
- Category auto-routing by file extension.
- Authenticated loopback REST + WebSocket API.
- Web UI with virtualized list, live segment view, add dialog and themes.
- Chromium and Firefox MV3 extension sources with options page.
- Opt-in browser-download takeover (default off, accept-before-cancel).
- Native-messaging host with live daemon test.
- Plain FTP downloads with `REST` resume and a local fixture.
- HLS VOD downloads (master/media playlists, init maps, ordered segments).
- HLS AES-128 CBC decryption with key fetch and explicit/sequence IVs.
- DASH VOD downloads (duration or SegmentTimeline `$Time$`), DRM rejection.
- HLS/DASH segment progress units and bounded pseudo-segment UI events.
- FFmpeg sidecar boundary.
- Same-origin site crawler with robots.txt and extension filters.
- Instrumented adversarial HTTP testkit.
- Property tests for segment invariants.
- Process-kill/resume crash-safety test.
- ADRs, specs, feature matrix and parity scorecard.

### Security

- Loopback-only API, bearer token, loopback Host allowlist, origin checks, no
  CORS, body limit.
- Filename traversal and Windows reserved-name protection.
- No telemetry.

### Known limitations

- No FTPS/SFTP, HTTP/3, BitTorrent or MMS (plain FTP works).
- Browser interception is opt-in and source-tested, not browser-automated.
- HLS/DASH VOD work; DASH live/audio groups, HLS key rotation and FFmpeg mux
  wiring are still missing.
- No tray, autostart, single-instance or installers.
- English-only.
