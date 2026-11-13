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
- Native Windows toast notifications on completion.
- Desktop tray with close-to-tray, Open and Quit.
- HTTP/2 TLS ALPN verified with a dedicated h2 fixture.
- GitHub release auto-update check/install with SHA-256 verification.
- WiX MSI installer for x86-64 Windows.
- Portable Windows ZIP and per-user install script.
- Parallel HLS/DASH segment fetch with ordered writes.
- HTTP 511 captive-portal classification.
- Local-file drag/drop via multipart import.
- SOCKS5 proxy support with loopback fixture.
- Per-error-class retry overrides via config and `--retry-override`.
- Searchable settings dialog with collapsible Advanced section.
- Daemon single-instance status documented as complete.
- Process-wide shared speed limiter with per-download overrides.
- Free-space preflight with configurable reserve.
- Pooled connection reuse regression test.
- Safe opt-in ZIP extraction after download.
- `rustget:` URL-scheme handler and Windows/Linux registration helpers.
- File-association helpers and CLI.
- Committed Criterion primitive benchmark results.
- 10,000-record queue load regression test.
- Windows case-collision path test.
- Persisted Basic auth with CLI/UI and end-to-end tests.
- Named command-based resolver plugins (`--plugin`, `--resolver`).
- Persistent SQLite cookie jar with per-download opt-in.
- Post-download action runner with `{file}` and timeout.
- Cross-host redirect auth-stripping regression test.
- yt-dlp resolver adapter with configurable command and stub tests.
- HTTP Digest auth (MD5/SHA-256, qop=auth) with challenge fixture.
- Mutual-TLS client certificates via per-download PEM path/CLI/UI.
- Optional antivirus scan hook with timeout and failure marking.
- Optional Windows tray shell (`rustget-tray`) with Open/Quit.
- English/German UI catalog with persisted language selector and `?lang=`.
- Two-step first-run wizard that tests the API token before saving it.
- Explicit per-download HTTP proxy (CLI/API/UI) with a forward-proxy test.
- In-app log viewer backed by a bounded INFO+ tracing buffer.
- Download tags: persisted, editable via API/CLI/UI, included in search.
- Priority editing via API and the details pane (Move up/down).
- Drag an http(s) link onto the window to prefill the add dialog.
- `rustget autostart enable|disable|status` for Windows, Linux and macOS
  (Windows registry write is blocked in the current test environment).
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
- HLS/DASH quality catalog and add-dialog variant picker.
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
