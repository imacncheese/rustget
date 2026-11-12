# RustGet

**A fast, open-source, cross-platform download manager written in Rust.**

RustGet is an independent, clean-room implementation of the download-manager
feature set popularised by Internet Download Manager: dynamic multi-connection
segmentation, crash-safe resume, queues, a local API, browser integration and a
modern UI. It is not affiliated with or endorsed by Tonec FZE.

![RustGet German UI](docs/assets/ui-de-main.png)
![RustGet first-run wizard](docs/assets/ui-wizard.png)
![RustGet main window](docs/assets/ui-main.png)

![RustGet live speed chart](docs/assets/ui-speed.png)
![RustGet scheduler](docs/assets/ui-scheduler.png)

## Status at a glance

| Area | State | Evidence |
|---|---|---|
| Multi-connection HTTP(S) engine | Working | 8 integration tests, 1 work-stealing test |
| Crash-safe resume | Working | kill-process test resumes byte-exact |
| Dynamic work stealing | Working | instrumented slow-range test observes re-splits |
| Rate limiting, retries, checksums | Working | unit + integration tests |
| SQLite queue and daemon | Working | manager/DB tests, CLI end-to-end |
| Authenticated REST + WebSocket API | Working | API integration tests |
| Web UI | Working | headless-browser screenshot above |
| Native-messaging browser bridge | Working | framing tests + live daemon test |
| HLS / DASH downloads | VOD working | HLS AES-128, DASH template; DRM/audio groups not supported |
| FTP | Plain FTP + resume | local FTP fixture; FTPS not supported |
| Scheduler windows | Working | pure tests, daemon loop, web timeline |
| FTPS, HTTP/3, BitTorrent, tray | Not implemented | see the parity scorecard |

The authoritative, test-backed status is
[`docs/specs/13-parity-scorecard.md`](docs/specs/13-parity-scorecard.md). The
running log is [`docs/PROGRESS.md`](docs/PROGRESS.md).

## Quick start

### Build

```sh
cargo build --release
```

Rust 1.85+ is required. On Linux, install a C toolchain and OpenSSL-free
`rustls` is used for TLS. No system libcurl is needed.

### One-shot download

```sh
rustget add https://example.com/archive.zip --connections 16
rustget add https://example.com/archive.zip --output ./archive.zip --limit 8
rustget add https://primary.example/file.bin --mirror https://mirror.example/file.bin
rustget add https://mtls.example/file.bin --client-cert ./client.pem
rustget add ftp://user:pass@ftp.example/file.bin
rustget export queue.json
rustget-tray   # optional Windows tray; RUSTGET_UI_URL overrides the URL
rustget serve --scanner "C:\Program Files\ClamAV\clamscan.exe" --scanner-arg --no-summary
rustget import queue.json
```

The CLI waits for completion and prints the final path. If a daemon is running,
the same command submits the download to it instead.

### Daemon + UI

```sh
rustget serve --open
```

The daemon prints a loopback URL containing a one-time bearer token. It binds
`127.0.0.1:8765` by default and refuses non-loopback addresses.

```text
API:    http://127.0.0.1:8765
token:  <generated>
open:   http://127.0.0.1:8765/#token=<token>
```

The UI is a thin client over `/api/v1`. The CLI uses the same API when a daemon
is available.

### Queue and scheduling

```sh
rustget add URL --no-start
rustget list
rustget schedule add --name night --start 01:00 --stop 06:00 --days mon,tue,wed,thu,fri
rustget schedule list
```

### Pattern batch

```sh
rustget add 'https://example.com/img_[001-100].jpg' --connections 4
rustget batch urls.txt --connections 4
```

### Browser integration

1. Build `rustget-native-host`.
2. Install `extensions/native-host/com.rustget.app.json` in the browser's native
   messaging host directory, replacing the binary path and extension ID.
3. Load `extensions/chromium` unpacked (or `extensions/firefox` via
   `about:debugging`).
4. Start `rustget serve`.

See [the browser bridge spec](docs/specs/05-browser-bridge.md).

## What works

- **Dynamic segmentation.** Starts with up to 32 connections. When a worker
  finishes, the largest active range is split and the freed connection takes the
  second half. A cooperative stop boundary prevents overlapping writes.
- **Adaptive concurrency.** Aggregate throughput is sampled; connections grow
  only while adding one still improves throughput.
- **Crash safety.** Every segment writes at its own absolute offset into a
  `.part` file. An append-only JSONL journal records full segment snapshots.
  Recovery ignores torn tails and refetches anything beyond the last durable
  checkpoint. A test kills the process and verifies SHA-256 equality after
  resume.
- **Protocol behavior.** HEAD probe with `Range: bytes=0-0` fallback, 206
  validation, `If-Range`, `ETag`/`Last-Modified`/size drift detection, redirect
  handling, 200 fallback to a single stream, retry/backoff with jitter,
  mirror/alternate-URL failover (`--mirror`), and a process-wide per-host
  connection semaphore.
- **Integrity.** MD5, SHA-1, SHA-256 and BLAKE3 verification before the final
  rename.
- **Storage.** Sparse preallocation, positional writes, collision-safe filenames,
  Windows reserved-name handling, RFC 5987 content-disposition parsing.
- **Local API.** Token-authenticated REST, WebSocket events, origin checks, body
  limits, no CORS.
- **UI.** Virtualized list (10k+ rows), search/filter/sort, per-segment detail
  view, keyboard shortcuts, light/dark/system themes, add dialog, settings.
- **Browser bridge.** Standard 4-byte length-prefixed native messaging, context
  menu, floating media panel, and an opt-in browser-download takeover that only
  cancels after RustGet accepts the request.
- **Media.** HLS master/media VOD and duration-template DASH VOD downloads,
  ordered segment concatenation, encryption/DRM rejection, FFmpeg sidecar
  boundary; `rustget add https://site/master.m3u8`.
- **Discovery.** Same-origin, depth-bounded site crawler with robots.txt,
  extension filters and `rustget grab`.

## What is deliberately not claimed yet

RustGet is an M0–M3 implementation with parts of M4/M5. It does **not** yet have:

- FTPS/SFTP, HTTP/3/QUIC, BitTorrent/magnet or MMS (plain FTP is supported).
- Digest/NTLM/Kerberos authentication, PAC, client certificates.
- Learned/adaptive per-host limits (a fixed semaphore is implemented).
- HLS key rotation/audio groups, DASH live/multi-period manifests, separate
  audio/video muxing, or parallel segment download/resume for media.
- Tray integration, OS autostart, single-instance desktop shell, native
  notifications, or a Tauri binary.
- i18n, import/export, post-download actions, antivirus hooks, shutdown/sleep
  power actions.
- Auto-interception of every browser download (the extension uses explicit
  context-menu/media actions; it does not cancel browser downloads silently).

These gaps are listed row-by-row in the parity scorecard, not hidden.

## Architecture

```text
crates/
  rustget-core      pure segment ledger, partition, rate limiter, retry policy
  rustget-storage   positional part file, JSONL/crash journal, path safety
  rustget-net       probe, range workers, coordinator, checksums
  rustget-queue     SQLite persistence, manager, scheduler, categories
  rustget-ipc       authenticated REST/WS, static UI host
  rustget-app       web UI assets
  rustget-cli       `rustget` binary
  rustget-native-host  browser native-messaging binary
  rustget-tray      optional Windows tray shell (open UI/quit)
  rustget-media     HLS/DASH parsers, FFmpeg sidecar boundary
  rustget-testkit   instrumented adversarial origin
  rustget-crawler   same-origin site crawler
  rustget-platform  OS integration hooks (partial)
extensions/         Chromium + Firefox MV3 sources
docs/adr/           architecture decisions
docs/specs/         implementation-neutral specifications
```

The engine is layered so the segment algebra can be property-tested without
sockets, and the coordinator can be tested without a UI. See
[ADR-0002](docs/adr/0002-workspace-layering.md) and
[ADR-0003](docs/adr/0003-engine-coordinator-placement.md).

### Actual requests seen by the test origin

A 2 MiB download with four configured connections produced at least four
`206 Partial Content` requests. A forced-slow first range caused additional
split ranges; the final file still matched the source byte-for-byte.

## Tests and verification

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Current verified totals (run locally; see `docs/specs/12-testing.md` for the
commands and output summary):

- `rustget-core`: 17 tests (14 unit, 3 property), including randomized
  gap/overlap invariants.
- `rustget-storage`: 15 tests, including torn-journal recovery and out-of-order
  positional writes.
- `rustget-net`: 7 unit + 27 integration tests (engine, work stealing, adaptive
  growth, mirrors, host caps, HLS quality/AES, DASH timeline, FTP, HTTP proxy,
  mutual TLS, Digest, cross-host redirect auth stripping).
- `rustget-queue`: 16 DB migration/manager/duplicate/tags/scheduler/power-hook/HLS/FTP/mirror tests.
- `rustget-ipc`: 6 API tests (auth, origin/Host rejection, add/list/delete, probe/config bundles, logs).
- `rustget-cli`: 4 pattern + 1 kill/resume + 2 scanner + 2 yt-dlp + 1 post + 1 cookie + 2 plugin + 2 basic-auth tests.
- `rustget-testkit`: 5 protocol tests.
- `rustget-media`: 6 parser/discovery tests.
- `rustget-crawler`: 2 discovery/robots tests.
- `rustget-native-host`: 2 framing tests.
- `rustget-platform`: 3 single-instance/power/autostart + 2 scheme/extension tests.

Local benchmark (128 MiB loopback, 16 connections): RustGet 159.1 MiB/s latest
median (152.5 MiB/s over 9 runs) vs aria2 119.9 MiB/s earlier; both byte-exact. See
[`docs/specs/14-benchmark.md`](docs/specs/14-benchmark.md); this is not a WAN/1
Gbps claim.

## Documentation

- [Feature matrix](docs/specs/00-feature-matrix.md)
- [Black-box observations](docs/specs/01-behavior-observations.md)
- [Segmentation spec](docs/specs/02-segmentation.md)
- [Resume journal](docs/specs/03-resume-journal.md)
- [Scheduler semantics](docs/specs/04-scheduler.md)
- [Browser bridge](docs/specs/05-browser-bridge.md)
- [UI/UX + wireframes](docs/specs/06-ui-ux.md)
- [Error taxonomy](docs/specs/07-error-taxonomy.md)
- [Settings schema](docs/specs/08-settings-schema.md)
- [Local API](docs/specs/09-api.md)
- [Security model](docs/specs/10-security.md)
- [Crash-safety proof](docs/specs/11-crash-safety.md)
- [Testing strategy](docs/specs/12-testing.md)
- [Parity scorecard](docs/specs/13-parity-scorecard.md)
- [Benchmark report](docs/specs/14-benchmark.md)
- [Crawler spec](docs/specs/15-crawler.md)
- [Media spec](docs/specs/16-media.md)
- [FTP spec](docs/specs/17-ftp.md)
- [ADRs](docs/adr/)

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md). The
project has no telemetry and no network calls other than user-requested
downloads, update checks that are opt-in, and the loopback API.

## License

MIT OR Apache-2.0. RustGet contains no IDM code, assets or binaries. IDM is a
trademark of Tonec FZE; this project is independent.
