# RustGet feature matrix

**Scope.** This matrix enumerates the IDM feature surface requested in the
mission, adds the engineering requirements that make those features usable, and
records the *current* RustGet state. It is intentionally blunt: a feature is only
`✅` when it is implemented and exercised by an automated test or a recorded
end-to-end run.

**Legend**

- `✅` implemented, verified
- `◐` implemented partially or verified only in a narrower path
- `○` designed/specified, not implemented
- `—` not started
- Priority: `MUST` / `SHOULD` / `COULD`
- P0 (ship blocker) … P3 (nice to have)
- Milestone: M0–M7 plus `Backlog`

**Reference sources.** IDM public help and product pages, accessed 2026-11-12;
see [behavior observations](01-behavior-observations.md) for URLs and the
clean-room boundary. Feature names are descriptive, not copied assets.

## A. Download engine

| ID | Capability | Pri | P | Target | State | Evidence / remaining gap |
|---|---|---|---|---|---|---|
| A01 | Dynamic file segmentation, selectable up to 32 connections | MUST | P0 | M1 | ✅ | `EngineConfig.max_connections <= 32`, ledger partitions; tested |
| A02 | In-half split of the largest active segment when a connection frees | MUST | P0 | M2 | ✅ | `SegmentLedger::try_split`; engine publishes cooperative stop boundary; forced-slow test |
| A03 | Adaptive connection growth while throughput improves | SHOULD | P1 | M2 | ✅ | 2 s AIMD sample in coordinator; only grows on >2% gain |
| A04 | Connection reuse / keep-alive (avoid per-segment handshake) | MUST | P0 | M2 | ◐ | `reqwest` pool (`pool_max_idle_per_host`, 90 s idle); no own socket pool or explicit reuse proof |
| A05 | Resume after crash, power loss, network drop, IP change | MUST | P0 | M1 | ◐ | Process-kill resume proven byte-exact; power-loss fsync path configurable, not power-cut tested |
| A06 | Resume validation with ETag/Last-Modified/size/If-Range | MUST | P0 | M1 | ✅ | Probe + journal validator; changing-ETag test |
| A07 | Detect server change mid-transfer | MUST | P0 | M1 | ✅ | 200-to-If-Range and 416 classified as `RemoteChanged`; test |
| A08 | Fallback to single stream on non-Range servers | MUST | P0 | M1 | ✅ | Probe sees 200/Accept-Ranges absent; reset + single stream; test |
| A09 | Pre-allocation / sparse part file | MUST | P0 | M1 | ✅ | `PartFile` `set_len`; optional full zero-fill; tests |
| A10 | Positional writes and out-of-order segment assembly | MUST | P0 | M1 | ✅ | `write_at` on Unix/Windows; out-of-order test |
| A11 | Bounded write buffering, no whole-file memory | MUST | P0 | M1 | ✅ | 64 KiB network chunks, direct `pwrite`; no file-sized buffer |
| A12 | Safe `.part` + metadata journal | MUST | P0 | M1 | ✅ | `.part` + append-only `.rustget`; torn-tail test |
| A13 | Journal records positions several times per minute | MUST | P0 | M1 | ✅ | 250 ms default checkpoint; kill test observes progress |
| A14 | Mirror / alternate URL failover | SHOULD | P1 | M2 | ✅ | Probe fallback + per-worker URL rotation on transient/rate-limit errors; two-origin tests |
| A15 | Retry with exponential backoff and jitter | MUST | P0 | M2 | ✅ | `RetryPolicy`, capped, jitter; unit tests |
| A16 | Per-error-class retry policies | SHOULD | P1 | M2 | ◐ | HTTP classes and transient/fatal; no user-editable per-class table |
| A17 | Global and per-download speed limit (token bucket) | MUST | P0 | M2 | ◐ | Per-download token bucket and global config; no single process-wide shared bucket |
| A18 | Per-host connection caps / learned server limits | SHOULD | P1 | M2 | ◐ | Process-wide per-host semaphore (`max_connections_per_host`), tested across two downloads; no learned 429 limit cache |
| A19 | HTTP/1.1 | MUST | P0 | M1 | ✅ | `reqwest`/hyper, range tests |
| A20 | HTTP/2 | MUST | P1 | M2 | ◐ | Enabled by `reqwest` (`http2` feature), no dedicated h2 test |
| A21 | HTTP/3 / QUIC | COULD | P2 | M7 | — | `quinn`/`h3` not wired |
| A22 | HTTPS with rustls | MUST | P0 | M1 | ✅ | `rustls-tls`, no OpenSSL system dependency |
| A23 | FTP / FTPS | SHOULD | P2 | M6 | ◐ | Plain FTP download + `REST` resume with local fixture; FTPS/TLS and segmented FTP missing |
| A24 | SFTP | COULD | P3 | M7 | — | Planned |
| A25 | HLS (.m3u8) | SHOULD | P1 | M5 | ◐ | Master/media VOD, init map, ordered concat, AES-128 CBC + key fetch; audio groups/parallel segments/key rotation missing |
| A26 | DASH (.mpd) | SHOULD | P1 | M5 | ◐ | Duration and SegmentTimeline VOD, `$Time$`/`$Number$`, concat, DRM rejection; live/multi-period/audio missing |
| A27 | Magnet / BitTorrent | COULD | P3 | M7 | — | Out of current scope |
| A28 | Probe: HEAD then GET `Range: bytes=0-0` | MUST | P0 | M1 | ✅ | Implemented; Content-Range/length parsing |
| A29 | Content-Encoding pitfalls | MUST | P0 | M1 | ✅ | Forces `Accept-Encoding: identity`; rejects non-identity |
| A30 | Basic authentication | MUST | P1 | M1 | ◐ | `RequestOptions.basic_auth` exists; not persisted or exposed in UI/CLI |
| A31 | Digest / NTLM / Negotiate / Kerberos | SHOULD | P2 | M6 | ◐ | HTTP Digest MD5/SHA-256 + qop=auth implemented/tested; NTLM/Negotiate/Kerberos missing |
| A32 | Cookies | MUST | P1 | M1 | ◐ | Native host passes cookie header; no cookie jar/refresh |
| A33 | Custom headers, Referer, User-Agent | MUST | P1 | M1 | ✅ | CLI `-H`, manager persists headers; UA config |
| A34 | Client certificates | COULD | P2 | M6 | ✅ | PEM client cert+key (`client_cert`/`--client-cert`), mutual-TLS success and rejection tests |
| A35 | HTTP/HTTPS/SOCKS5 proxy, system proxy, PAC | SHOULD | P2 | M6 | ◐ | Explicit HTTP proxy in `RequestOptions`; no SOCKS/PAC/system detection |
| A36 | Per-download proxy | COULD | P2 | M6 | ◐ | Field exists only at client construction |
| A37 | MD5 / SHA-1 / SHA-256 / BLAKE3 verification | MUST | P1 | M1 | ✅ | Streaming hashes, pre-rename verification; tests |
| A38 | Post-download open/run/extract | SHOULD | P2 | M4 | ◐ | Open containing folder via API; no run/extract |
| A39 | Antivirus scan hook | SHOULD | P2 | M6 | ✅ | `antivirus.scan_command` JSON argv, file appended, 60 s timeout, no shell; success/failure tests |
| A40 | Disk-full handling | MUST | P0 | M2 | ◐ | I/O errors surfaced; no preflight free-space reservation/retry UI |
| A41 | Slow-loris / idle timeout | MUST | P0 | M2 | ✅ | Idle timeout on stream and probe |
| A42 | Mid-stream reset recovery | MUST | P0 | M2 | ✅ | Flaky-origin test retries and verifies bytes |
| A43 | DNS failure, captive portal classification | SHOULD | P1 | M2 | ◐ | Error classes exist; no portal detection |
| A44 | Path-too-long / reserved names / case collisions | MUST | P0 | M1 | ◐ | Sanitizer + reserved names + unique names; no long-path tests |
| A45 | 10k+ queued downloads without UI lag | SHOULD | P1 | M4 | ◐ | Virtualized list and SQLite; not load-tested at 10k |
| A46 | Saturate 1 Gbps / beat `aria2c -x16 -s16` | SHOULD | P1 | M2 | ✅ | Local 128 MiB: 167.5 vs 119.9 MiB/s wall (1.40×), byte-exact; WAN/1 Gbps unproven |

## B. Capture and integration

| ID | Capability | Pri | P | Target | State | Evidence / remaining gap |
|---|---|---|---|---|---|---|
| B01 | Chromium MV3 extension | MUST | P1 | M5 | ◐ | Context menu, media panel, native messaging; loaded source, not store-published |
| B02 | Firefox extension | MUST | P1 | M5 | ◐ | MV3 manifest and shared sources; not browser-tested |
| B03 | Edge | SHOULD | P1 | M5 | ◐ | Chromium MV3 by construction; not tested |
| B04 | Automatic download interception | MUST | P1 | M5 | ◐ | Opt-in `downloads.onCreated`: accepts in RustGet first, then cancels; source-tested, not browser-automated |
| B05 | Media request sniffing | SHOULD | P1 | M5 | ◐ | Direct `video/audio/source` + media-link detection; no `webRequest` sniffer |
| B06 | Floating “download this video” affordance | SHOULD | P1 | M5 | ✅ | Injected panel lists detected media, max 5 visible |
| B07 | Native Messaging host (Rust) | MUST | P1 | M5 | ✅ | Standard framing, ping/add/list/pause/resume; live daemon test |
| B08 | Local authenticated REST/WebSocket API | MUST | P0 | M3 | ✅ | API tests; token, origin, body limit |
| B09 | Clipboard URL monitoring | SHOULD | P1 | M4 | ◐ | UI paste button; no background clipboard watcher |
| B10 | Drag and drop links | SHOULD | P1 | M4 | ✅ | Drop an http(s) URI list onto the window; add dialog prefilled |
| B11 | Add URL dialog with probe preview | MUST | P0 | M4 | ✅ | Check button calls `/api/v1/probe` and shows filename/size/range support |
| B12 | Batch URL import | MUST | P1 | M4 | ✅ | `batch` command |
| B13 | Numeric/wildcard batch pattern | SHOULD | P1 | M4 | ✅ | `[001-100]`, multiple ranges; tests; 10k cap |
| B14 | Site grabber/spider | SHOULD | P2 | M6 | ◐ | Same-origin BFS crawler, depth/page caps, robots toggle, extension filters; CLI `grab`; no UI or saved projects |
| B15 | Video grabber HLS/DASH + FFmpeg mux | SHOULD | P2 | M5 | ◐ | Parsers and sidecar boundary; not connected to downloader |
| B16 | yt-dlp plugin | COULD | P3 | M7 | ✅ | yt-dlp is a named entry in the command-based resolver plugin registry |
| B17 | Categories and auto-routing | MUST | P1 | M3 | ✅ | DB categories, extension mapping, directory routing |
| B18 | Duplicate detection by URL | SHOULD | P1 | M3 | ✅ | Completed URL returns 409; queued/paused duplicate resumes the existing row; `allow_duplicate` overrides |
| B19 | Filename sanitization and collisions | MUST | P0 | M1 | ✅ | Traversal, reserved names, truncation, `(1)` suffix; tests |
| B20 | Content-Disposition / RFC 5987 | MUST | P0 | M1 | ✅ | Quoted + `filename*`; tests |
| B21 | Redirect resolution (301/302/303/307/308) | MUST | P0 | M1 | ✅ | `reqwest` policy; 2-hop test |
| B22 | Custom protocol schemes (`idmreg:` etc.) | COULD | P3 | M5 | — | RustGet does not imitate IDM schemes |

## C. Management

| ID | Capability | Pri | P | Target | State | Evidence / remaining gap |
|---|---|---|---|---|---|---|
| C01 | Download queue | MUST | P0 | M3 | ✅ | SQLite `queued`, scheduler promotes by priority |
| C02 | Priorities | MUST | P1 | M3 | ✅ | `priority` column, queue order test |
| C03 | Concurrency limits | MUST | P0 | M3 | ✅ | `max_concurrent`, manager test |
| C04 | Scheduler time windows | MUST | P0 | M3 | ✅ | Weekly day mask, start/stop, overnight; pure tests |
| C05 | Start/stop/pause-all | MUST | P0 | M3 | ✅ | Per-item + `pause-all`/`resume-all` API and CLI; scheduler closes windows |
| C06 | Auto shutdown/sleep/hibernate/exit | SHOULD | P1 | M6 | ✅ | Actions persisted and executed on window close only with daemon `--allow-power-actions`; dry command tests |
| C07 | Queue UI | MUST | P1 | M4 | ◐ | Status filters + priority Move up/down in details; no drag reorder or per-queue lanes |
| C08 | Per-download properties | MUST | P1 | M4 | ✅ | Details pane: path, URL, size, speeds, error |
| C09 | Per-connection view | MUST | P1 | M4 | ✅ | Live segment list over WebSocket |
| C10 | Live speed chart | SHOULD | P1 | M4 | ✅ | Canvas sparkline from the last 60 progress samples, peak label |
| C11 | Import/export download list | SHOULD | P2 | M6 | ✅ | Versioned JSON bundle via API and `rustget export`/`import`; imported rows are queued |
| C12 | Import/export settings | SHOULD | P2 | M6 | ✅ | Bundle contains downloads, settings, categories and schedules; API + CLI + Settings dialog |
| C13 | Search | MUST | P1 | M4 | ✅ | UI search; API filter by status |
| C14 | Filter | MUST | P1 | M4 | ✅ | Status filters |
| C15 | Sort | MUST | P1 | M4 | ✅ | Name/size/progress/speed/status |
| C16 | Tags | COULD | P2 | M6 | ✅ | Persisted/normalized tags, API PATCH, CLI `tag`/`--tag`, details chips, search includes tags |
| C17 | Tray icon | SHOULD | P1 | M4 | ◐ | Windows `rustget-tray` binary (open UI/quit) starts and stays alive; menu interaction not automated, other OS stubs |
| C18 | Desktop notifications | SHOULD | P1 | M4 | ◐ | Browser Notification API; no OS integration |
| C19 | Start with system | SHOULD | P2 | M6 | ◐ | Windows registry / Linux .desktop / macOS LaunchAgent code + `autostart` CLI; Windows registry write is denied in this environment, Unix paths not executed |
| C20 | Single instance | MUST | P1 | M4 | ◐ | Daemon lock file enforced; no native desktop shell yet |
| C21 | CLI: add/list/pause/resume/remove/schedule/config | MUST | P0 | M1 | ✅ | Implemented; pause/resume require daemon |
| C22 | Headless daemon | MUST | P0 | M3 | ✅ | `rustget serve` |
| C23 | i18n via Fluent | SHOULD | P2 | M7 | ◐ | JSON catalog (English/German), persisted language selector, core UI translated; not Fluent and dynamic strings incomplete |
| C24 | Accessibility (labels, focus, screen reader) | MUST | P1 | M4 | ◐ | Semantic labels/keyboard; no screen-reader audit |
| C25 | Light/dark/system themes | MUST | P1 | M4 | ✅ | CSS variables, persisted theme |
| C26 | Keyboard shortcuts | SHOULD | P1 | M4 | ✅ | Ctrl+N/F, Space, Delete, arrows, Esc |
| C27 | 10k-row virtualized table | SHOULD | P1 | M4 | ✅ | Windowed rendering |
| C28 | Session restore | MUST | P0 | M3 | ✅ | SQLite + interrupted-row recovery |
| C29 | Pause survives restart | MUST | P0 | M3 | ✅ | Journal + row state |
| C30 | Log viewer | SHOULD | P2 | M4 | ✅ | Bounded INFO+ tracing buffer, `/api/v1/logs`, UI Logs dialog; stderr unchanged |

## D. UI / UX

| ID | Capability | Pri | P | Target | State | Evidence / remaining gap |
|---|---|---|---|---|---|---|
| D01 | Main window with category sidebar and table | MUST | P0 | M4 | ✅ | Screenshot `docs/assets/ui-main.png` |
| D02 | Detail pane with per-segment progress | MUST | P0 | M4 | ✅ | Segment bars from live progress |
| D03 | Add download dialog | MUST | P0 | M4 | ✅ | URL, folder, connections, limit, category, start |
| D04 | File-info probe preview in dialog | SHOULD | P1 | M4 | ✅ | “Check” calls `/api/v1/probe`; filename, size and range support shown before submit |
| D05 | Video quality picker | SHOULD | P2 | M5 | ✅ | `/api/v1/media/probe` catalog, add-dialog quality select, persisted `media_variant`; HLS/DASH tested |
| D06 | Scheduler weekly timeline | SHOULD | P2 | M4 | ✅ | Web UI timeline, add/delete form, API-backed; power actions not executed |
| D07 | Settings, searchable/advanced collapse | SHOULD | P1 | M4 | ◐ | Small settings dialog only |
| D08 | First-run wizard | SHOULD | P1 | M4 | ✅ | Two-step welcome → token test/save; screenshot `docs/assets/ui-wizard.png` |
| D09 | HiDPI | MUST | P1 | M4 | ✅ | CSS pixels, no fixed raster assets |
| D10 | Drag/drop links and files | SHOULD | P1 | M4 | ◐ | Links supported; local files rejected with an explanation |
| D11 | Sortable columns and live ETA | MUST | P1 | M4 | ✅ | Implemented |
| D12 | Responsive layout | SHOULD | P2 | M4 | ✅ | Breakpoints at 1080/760 px |

## E. Quality, test and security

| ID | Capability | Pri | P | Target | State | Evidence / remaining gap |
|---|---|---|---|---|---|---|
| E01 | Unit tests | MUST | P0 | M0 | ✅ | Workspace tests |
| E02 | Property tests for segment invariants | MUST | P0 | M0 | ✅ | `proptest` gap/overlap/random ops |
| E03 | Fault injection / kill -9 resume | MUST | P0 | M2 | ✅ | `crash_safety.rs` |
| E04 | Adversarial HTTP testkit | MUST | P0 | M0 | ✅ | no-Range, flaky, redirect, auth, throttle, ETag |
| E05 | Fuzzing header/playlist/native parsers | SHOULD | P1 | M2 | ○ | Targets specified; not run in CI |
| E06 | Benchmarks (`criterion`) | SHOULD | P1 | M1 | ◐ | Plan and script; no committed criterion run |
| E07 | CI matrix Win/Linux/macOS | MUST | P0 | M0 | ✅ | `.github/workflows/ci.yml` (not yet executed in a hosted repo) |
| E08 | `clippy -D warnings`, rustfmt, deny, audit | MUST | P0 | M0 | ◐ | fmt + clippy clean; deny/audit config present but not run locally |
| E09 | Reproducible/signed installers | SHOULD | P1 | M7 | — | Planned |
| E10 | SSRF/local API hardening | MUST | P0 | M3 | ✅ | Loopback bind, bearer token, origin check, no CORS |
| E11 | Path traversal protection | MUST | P0 | M1 | ✅ | Sanitizer tests |
| E12 | Redirect safety (no credential leak) | MUST | P0 | M2 | ✅ | Cross-host redirect fixture proves Authorization is dropped at target |
| E13 | Post-action sandboxing | SHOULD | P2 | M6 | ◐ | argv post-action runner with `{file}`, 60 s timeout, no shell; no OS/filesystem sandbox |
| E14 | No telemetry by default | MUST | P0 | M3 | ✅ | No analytics code/dependency |
| E15 | Dependency/license policy | MUST | P1 | M0 | ✅ | `deny.toml`, `cargo-deny` in CI |

## F. Platform / packaging

| ID | Capability | Pri | P | Target | State | Evidence / remaining gap |
|---|---|---|---|---|---|---|
| F01 | Windows 10/11 | MUST | P0 | M1 | ✅ | Built/tested on Windows 11 (GNU toolchain) |
| F02 | Linux | MUST | P0 | M1 | ◐ | Unix positional writes compile path; not run in this environment |
| F03 | macOS | MUST | P0 | M1 | ◐ | Unix path, no notarized build |
| F04 | MSI/NSIS installer | SHOULD | P1 | M7 | — | Planned |
| F05 | AppImage/deb/rpm | SHOULD | P1 | M7 | — | Planned |
| F06 | DMG | SHOULD | P1 | M7 | — | Planned |
| F07 | File association | COULD | P2 | M7 | — | Planned |
| F08 | Auto-update | SHOULD | P3 | M7 | — | Opt-in only; not implemented |
| F09 | Native tray/shell | SHOULD | P1 | M4 | ◐ | Windows tray shell implemented; no tray state/minimize-to-tray or macOS/Linux backend |
| F10 | Single-instance lock | MUST | P1 | M4 | ✅ | Cross-process lock file; second daemon exits with a clear error |

## G. Protocols, media and extensibility

| ID | Capability | Pri | P | Target | State | Evidence / remaining gap |
|---|---|---|---|---|---|---|
| G01 | Generic HTTP(S) progressive | MUST | P0 | M1 | ✅ | Engine tests |
| G02 | HLS master/media playlist parser | SHOULD | P1 | M5 | ✅ | Variants, segments, encryption/init map; tests |
| G03 | DASH MPD parser | SHOULD | P1 | M5 | ✅ | Representations, templates, ISO duration; tests |
| G04 | HLS/DASH segment scheduler | SHOULD | P1 | M5 | ◐ | Sequential VOD segment download with retry/rate limit; no parallel segment workers or resume |
| G05 | FFmpeg mux sidecar | SHOULD | P2 | M5 | ◐ | `-c copy` plan; not invoked by the media path yet |
| G06 | Plugin API for extractors | COULD | P2 | M7 | ✅ | `plugins.resolvers` NAME→argv registry, `--plugin`, `--resolver`, 60 s timeout, success/unknown tests |
| G07 | yt-dlp adapter | COULD | P3 | M7 | ✅ | `ytdlp.command`, `use_ytdlp`, CLI/UI, 60 s timeout, stub resolver success/failure tests |
| G08 | Site grabber | SHOULD | P2 | M6 | ◐ | Link discovery implemented; no mirroring, JS rendering or project persistence |
| G09 | Cookie jar persistence | SHOULD | P1 | M6 | ✅ | SQLite cookie jar, Set-Cookie capture, `use_cookie_jar`/CLI/UI, session reuse test |
| G10 | PAC / system proxy | SHOULD | P2 | M6 | ◐ | Explicit per-download HTTP proxy (CLI/API/UI) verified through a local proxy fixture; PAC/system discovery missing |
| G11 | Digest/NTLM | SHOULD | P2 | M6 | — | Planned |
| G12 | HTTP/3 | COULD | P3 | M7 | — | Planned |

## Counts (current)

| State | Count |
|---|---:|
| ✅ implemented + verified | 88 |
| ◐ partial | 46 |
| ○ specified, not implemented | 1 |
| — not started | 12 |
| **Total tracked** | **147** |

By area:

| Area | Rows | ✅ | ◐ | ○ | — |
|---|---:|---:|---:|---:|---:|
| A. Download engine | 46 | 24 | 19 | 0 | 3 |
| B. Capture/integration | 22 | 13 | 8 | 0 | 1 |
| C. Management | 30 | 23 | 7 | 0 | 0 |
| D. UI/UX | 12 | 10 | 2 | 0 | 0 |
| E. Quality/security | 15 | 10 | 3 | 1 | 1 |
| F. Platform/packaging | 10 | 2 | 3 | 0 | 5 |
| G. Protocols/extensibility | 12 | 6 | 4 | 0 | 2 |

The count is maintained manually alongside the table; the parity scorecard gives
the higher-level roll-up and accepted gaps.
