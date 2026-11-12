# Feature-parity scorecard

**Date:** 2026-11-12
**Version:** 0.1.0 (M0–M3 complete, M4/M5 partial)
**Matrix:** [00-feature-matrix.md](00-feature-matrix.md)

## 1. Headline

RustGet is a working download manager with a verified multi-segment engine,
crash-safe resume, queue persistence, local API and UI. It is **not** yet at
Internet Download Manager feature parity, and it does not claim to be.

| Dimension | IDM | RustGet today | Assessment |
|---|---|---|---|
| Single-file HTTP(S) acceleration | mature, up to 32 connections | multi-range, work stealing, adaptive growth | **core parity** |
| Resume/crash safety | mature, custom format | journal + kill-process proof | **core parity**, power-loss proof missing |
| Browser integration | deep automatic interception | context menu + media panel + native host | **partial** |
| Media/HLS/DASH | mature grabber/muxer | parsers only | **early** |
| Queue/scheduler | mature | queue + weekly windows | **partial** |
| UI | native Windows, dense | modern web UI | **differentiation**, native shell missing |
| Protocols | HTTP/FTP/MMS + auth/proxy breadth | HTTP(S) + plain FTP | **large gap** (FTPS/MMS/auth breadth) |
| Platform integration | tray, startup, shell | daemon + web | **large gap** |
| Installers/updates | signed installers | source build | **not started** |

## 2. Matrix roll-up

| State | Count | Share |
|---|---:|---:|
| ✅ implemented + verified | 78 | 53.1% |
| ◐ partial | 41 | 27.9% |
| ○ specified only | 1 | 0.7% |
| — not started | 27 | 18.4% |
| **Total** | **147** | 100% |

“Implemented or partial” = 119/147 = **81.0%** of the tracked surface. That
number is deliberately broad: a parser counts as partial even when it is not
wired to the UI.

### 2.1 By area

| Area | Complete | Partial | Not started |
|---|---:|---:|---:|
| Download engine (46) | 22 | 18 | 6 |
| Capture/integration (22) | 12 | 8 | 2 |
| Management (30) | 21 | 5 | 4 |
| UI/UX (12) | 9 | 2 | 1 |
| Quality/security (15) | 9 | 3 | 2 |
| Platform/packaging (10) | 2 | 2 | 6 |
| Protocols/extensibility (12) | 3 | 3 | 6 |

## 3. What matches or beats IDM today

These are defensible claims, backed by code and tests:

1. **Dynamic work stealing with a formal invariant.** IDM documents the
   behaviour; RustGet proves no gaps/overlaps under randomized operations and
   verifies it end-to-end with a forced-slow origin.
2. **Crash resume under process kill.** A real child process is killed and the
   resumed file is SHA-256 identical.
3. **Open, documented local API.** Token-authenticated REST + WebSocket, with a
   machine-readable OpenAPI surface. IDM's equivalent is private.
4. **Thin, cross-platform UI.** A modern, keyboard-navigable virtualized UI
   served from the daemon, not tied to Windows.
5. **Open-source auditability and no telemetry.**
6. **Checksum verification (MD5/SHA-1/SHA-256/BLAKE3) before rename.**
7. **Adversarial protocol lab** available to contributors, not just internal QA.
8. **Local throughput** beat `aria2c -x16 -s16` on a 128 MiB loopback run
   (167.5 vs 119.9 MiB/s wall); not evidence for WAN links.

## 4. What IDM does materially better

1. **Protocol/auth breadth.** FTPS, MMS, Digest/NTLM/Kerberos, client certs,
   PAC/system proxy and learned per-host limits are not implemented. Plain FTP,
   fixed per-host caps and mirrors are implemented.
2. **Browser interception.** IDM's extension injects at `document_start` in all
   frames, observes requests and can take over downloads. RustGet requires a
   context-menu/media-panel action and does not intercept silently.
3. **Media pipeline.** IDM recognises and muxes many stream types. RustGet has
   parsers and an FFmpeg boundary, but no end-to-end HLS/DASH download/mux.
4. **Site grabber.** Not started.
5. **Windows shell integration.** Tray, startup, file association, IE-family
   integration, native dialogs.
6. **Mature scheduler/power actions.** Windows-only gaps, but IDM's power
   actions and queue semantics are more complete.
7. **Installer/update infrastructure.** None here.
8. **Per-host connection learning.** IDM lets users set server exceptions; we
   only have a global cap.
9. **Language breadth.** IDM ships ~50 locales; RustGet is English-only.

## 5. Milestone status

| Milestone | Exit criteria | State |
|---|---|---|
| M0 | feature matrix, observations, specs, ADRs, workspace, testkit | **complete** |
| M1 | multi-segment HTTP + resume + CLI + benchmarks | **complete except benchmark** |
| M2 | work stealing, adaptive concurrency, rate limit, retries, crash proof | **complete**; per-host caps and mirrors missing |
| M3 | queue/categories/scheduler, SQLite, API, event bus | **mostly complete**; queue UI/power actions missing |
| M4 | desktop UI, tray, notifications, clipboard, batch/pattern | **partial**; web UI/batch done, native shell/tray/clipboard watcher missing |
| M5 | extensions, native host, video sniffing, HLS/DASH, FFmpeg | **partial**; bridge and parsers done, interception/mux orchestration missing |
| M6 | FTP/FTPS, proxies/PAC, NTLM/Digest, cookies, grabber | **partial**: plain FTP done; FTPS/PAC/NTLM/grabber UI missing |
| M7 | HTTP/3, SFTP, BitTorrent, plugins, i18n, installers | **not started** |

## 6. Highest-risk gaps

| Risk | Why it matters | Mitigation |
|---|---|---|
| No automatic browser interception | Main IDM “it just works” behaviour | Add opt-in `webRequest`/downloads interception with per-site allowlist |
| No FTPS/auth breadth | Many enterprise/media use cases | FTPS via rustls + Digest/NTLM |
| No end-to-end media | “Download this video” is a headline IDM feature | Wire HLS/DASH scheduler + FFmpeg; test with local manifests |
| No tray/native shell | Desktop expectation | Tauri 2 or Slint shell over the same API |
| Power-loss claim | fsync off by default | Add maximum-safety profile and VM power-cut test |
| Browser extension untested in real browser | Could fail at install/run time | Add Playwright/WebDriver smoke test |
| UI accessibility unaudited | Must be usable by screen readers | NVDA/VoiceOver audit + axe-core CI |
| No installers | Distribution blocker | MSI/NSIS, AppImage/deb/rpm, dmg + signing |

## 7. Benchmark scorecard

| Metric | Target | Measured | Status |
|---|---|---|---|
| 1 Gbps saturation | 940 Mbps useful | not measured on 1 Gbps link | unproven |
| Beat `aria2c -x16 -s16` | ≥ 1.0× | 167.5 vs 119.9 MiB/s wall, 128 MiB loopback | **proven locally**, WAN unproven |
| Memory with 64 GiB file | bounded < 100 MiB | 64 KiB chunks + segment table | design-compliant, unmeasured |
| 10k rows UI | no jank | virtualized list built | unmeasured |
| Crash loss bound | ≤ checkpoint + chunk | test observed ≥1 MiB durable, resumed exact | proven for process kill |

The local comparison is reproducible with `benchmarks/run-local.ps1`; see
[14-benchmark.md](14-benchmark.md). It is not a WAN claim.

## 8. Release recommendation

RustGet 0.1.0 is suitable for:

- developers and power users who want a scriptable, auditable HTTP(S) downloader;
- contributors who want a tested segmentation/resume core to build on;
- Linux/macOS users willing to build from source.

It is **not** yet suitable as a drop-in IDM replacement for:

- users who need automatic browser takeover;
- FTPS and authenticated/media-site workflows (plain FTP works);
- Windows tray/installer expectations;
- non-English users;
- mission-critical power-loss guarantees.

## 9. Next three increments

1. **M4.1 native shell**: Tauri 2 wrapper over the existing API, tray, single
   instance, autostart, clipboard monitor.
2. **M5.1 media pipeline**: HLS/DASH download orchestration, AES-128 key fetch,
   FFmpeg mux, local manifest integration tests.
3. **M6.1 protocol breadth**: FTPS, Digest, SOCKS5, learned per-host limits.

Each increment must update this scorecard with evidence, not adjectives.
