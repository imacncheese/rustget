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
- Retry with exponential backoff and jitter; error classification.
- Token-bucket speed limiting.
- MD5/SHA-1/SHA-256/BLAKE3 checksum verification.
- SQLite WAL catalogue, queues, priorities and concurrency limits.
- Weekly scheduler windows with scheduler-owned pauses.
- Category auto-routing by file extension.
- Authenticated loopback REST + WebSocket API.
- Web UI with virtualized list, live segment view, add dialog and themes.
- Chromium and Firefox MV3 extension sources with options page.
- Opt-in browser-download takeover (default off, accept-before-cancel).
- Native-messaging host with live daemon test.
- HLS VOD downloads (master/media playlists, init maps, ordered segments).
- DASH VOD downloads (duration-based SegmentTemplate), DRM rejection.
- HLS/DASH segment progress units and bounded pseudo-segment UI events.
- FFmpeg sidecar boundary.
- Same-origin site crawler with robots.txt and extension filters.
- Instrumented adversarial HTTP testkit.
- Property tests for segment invariants.
- Process-kill/resume crash-safety test.
- ADRs, specs, feature matrix and parity scorecard.

### Security

- Loopback-only API, bearer token, origin checks, no CORS, body limit.
- Filename traversal and Windows reserved-name protection.
- No telemetry.

### Known limitations

- No FTP/FTPS/SFTP, HTTP/3, BitTorrent or MMS.
- No automatic browser download interception.
- No HLS/DASH download orchestration or FFmpeg mux integration.
- No tray, autostart, single-instance or installers.
- No per-host connection caps or mirror failover.
- English-only.
