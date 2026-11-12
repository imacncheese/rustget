# IDM black-box behavioural observations

**Date:** 2026-11-12
**Analyst:** RustGet clean-room team
**Environment:** Windows 11 26200, x86-64, Chrome/Edge available (not automated
against IDM), RustGet testkit on loopback.

## 0. Method and clean-room boundary

This phase collects *observable interface behaviour*, not source code. We use:

1. Public vendor documentation and help pages.
2. Static inspection of the publicly shipped browser-extension package
   (manifest, permissions, JavaScript, locale keys). This is code shipped to
   every user, not a hidden server component.
3. A local instrumented HTTP origin (`rustget-testkit`) and capture tooling for
   protocol experiments.
4. UI walkthrough/screenshot cataloguing if a licensed IDM installation is
   available.

We did **not** execute the downloaded installer, bypass licensing/registration,
unpack protected binaries, or copy IDM code, images, strings or protocol
implementations into RustGet. The extension package and installer were retained
only as analysis evidence under the untracked `.tmp/research/` directory and are
not redistributed.

### Artifacts observed

| Artifact | SHA-256 | Handling |
|---|---|---|
| IDM installer `idman643build18.exe` (12,164,608 bytes) | `7346128FAB42957B4254EABDB012BFDBD751E9069361E98A6916F26BB03D62EE` | Downloaded; **not executed**; not parsed beyond archive identification |
| Chrome extension CRX (IDM Integration Module 6.43.1) | `331A7B98AED7583B4B897205CD0393462A668576F0C5D32339EF1EA897DEB946` | Stripped CRX header, unzipped, statically read |
| Extension `manifest.json` | `6509E1BC80832FB02219D8197E2350876334B85FB76B55B22684F564CA585869` | Read |
| Extension `background.js` | `3590408894C701937FFA51048C14C3E6D1B534E9616B76D99E4AF523925294D7` | Read; no code copied |
| Extension `content.js` | `C72693E29946E7C816639214E7E7E1E7FE2BF83BC7E53020D01CB9F8564187D8` | Read; no code copied |

The installer could not be opened by a generic 7-Zip reader, consistent with a
custom installer container. We did not pursue unpacking because it would add no
protocol evidence beyond what the extension and public docs already expose.

## 1. Public documentation: confirmed behaviour

These are vendor statements, not reverse-engineered internals. They are the
requirements RustGet implements independently.

### 1.1 Dynamic segmentation

Source: <https://www.internetdownloadmanager.com/support/segmentation.html>
(accessed 2026-11-12).

- IDM divides a file dynamically **while downloading**, not once at start.
- When a connection becomes available, IDM finds the **largest segment and
  divides it in half**; the new connection downloads the second half.
- Connections are **reused** after finishing a segment, avoiding another
  connect/login stage.
- If a connection finishes and the next connection has not started its segment,
  the finished connection is reassigned. If the next connection has started,
  the finished connection “helps” slower connections by splitting the largest
  remaining segment.
- A segment is not split when it is “too small for this connection type”,
  implying a minimum-size/hysteresis rule.
- Positions are saved “several times per minute”; after a pause or power loss,
  IDM resumes and assembles the file correctly.
- On resume, the progress bar shows **new split boundaries** (pink lines),
  confirming that the segment table is rewritten and not required to match the
  pre-crash partition.

### 1.2 Product feature claims

Source: <https://www.internetdownloadmanager.com/features2.html>.

- Up to 8× acceleration, dynamic segmentation, connection reuse.
- HTTP, FTP, HTTPS and MMS.
- Browser integration for Chrome, Edge, Firefox, Opera, IE-family and others.
- Scheduler: start at a time, disconnect, shut down when done, periodic sync.
- Site spider/grabber with filters; scheduled and periodic projects.
- Categories with associated extensions and default directories.
- Download limits/quotas (MB/hour, MB per window).
- Automatic antivirus checking with configurable scanner and `[File]`
  placeholder.
- Proxy support including ISA and FTP proxies; authentication protocols Basic,
  Negotiate, NTLM, Kerberos.
- Drag and drop, “download all links”, multilingual UI.

### 1.3 Options and per-host tuning

Source: <https://www.internetdownloadmanager.com/support/options.html>.

- Per-server connection-count exceptions are supported. This is direct evidence
  that a **per-host cap table** is part of the product, though the storage format
  is not public.
- Download quotas are time-window based and resume automatically later.
- Temporary directory is configurable.
- User-Agent defaults to the browser that initiated a download; manually added
  downloads have a separate configurable UA.
- “Ignore file modification time changes when resuming” exists as an escape hatch
  for servers that rewrite `Last-Modified` on every request.
- Site logins are stored per URL/directory.
- Proxy settings support system copy, PAC, manual per-protocol, and
  “use proxy from browser request”.
- Silent mode, start-download-while-info-dialog, queue selection during batch.

### 1.4 Queue/scheduler

Source: <https://www.internetdownloadmanager.com/support/schedulerD.html>.

- Added URLs enter a queue automatically; completed files leave it.
- The `Q` column shows queue membership; order can be changed.
- User selects “number of files to download at the same time”; vendor suggests
  4–5.
- Scheduler can start and stop the queue and shut down the machine when done.
- Queue state is polled: “Every 30 seconds IDM checks the state of queued files.”
  Failed files are moved to the end of the queue.
- Scheduler-started downloads suppress completion/error dialogs.

This 30-second polling statement is important: it suggests IDM's queue state
machine is not purely event-driven. RustGet uses event-driven completion plus a
10-second scheduler window tick; behaviour is compatible, implementation is not
copied.

### 1.5 Command line

Source: <https://www.internetdownloadmanager.com/support/command_line.html>.

Documented switches: `/d URL`, `/s` (start queue), `/p path`, `/f filename`,
`/q` (exit after success), `/h` (hang up), `/n` (silent), `/a` (queue without
starting). Parameters are only valid with `/d`.

RustGet deliberately uses a POSIX-style CLI (`rustget add`, `--output`, etc.)
rather than cloning IDM's switch grammar. Compatibility with IDM command lines
is not a goal and is not claimed.

## 2. Browser extension: static observations

The shipped Chrome package is MV3, version 6.43.1, with a service-worker
background and a content script injected into **all frames at
`document_start`**.

### 2.1 Manifest surface

| Field | Observed value | Engineering implication |
|---|---|---|
| `manifest_version` | 3 | MV3 service worker lifecycle matters |
| `background` | `service_worker: background.js` | No persistent background page |
| `content_scripts` | all URLs, all frames, `document_start` | Deep frame/media interception |
| `permissions` | scripting, tabs, cookies, contextMenus, webNavigation, webRequest, declarativeNetRequest, downloads, downloads.shelf, downloads.ui, management, storage, proxy, nativeMessaging | Very broad; RustGet asks for less |
| optional | notifications, system.display | |
| `host_permissions` | `<all_urls>` | |
| `web_accessible_resources` | `document.js`, `debug.js`, `captured.js`, `captured.html` | Injected page-context helpers |
| `externally_connectable` | `*.internetdownloadmanager.com`, `*.tonec.com` | Vendor pages can message the extension |
| `minimum_chrome_version` | 109 | |

RustGet's extension intentionally requests a narrower set:
`contextMenus`, `nativeMessaging`, `storage`, `cookies`, `activeTab`,
`<all_urls>`. It does not request `proxy`, `management`, `webRequest`,
`declarativeNetRequest`, or `downloads` in the current build. This is a
deliberate least-privilege deviation.

### 2.2 Bridge transports

Static strings in `background.js` show two transports:

1. **Native messaging** via `browser.runtime.connectNative("com.tonec.idm")`.
2. A fallback **WebSocket** to a vendor-controlled host:
   `ws://<host>/?cid=<client>&rnd=<random>` with subprotocol
   `plugin.v3.internetdownloadmanager.com`.

The extension also uses custom schemes such as `idmreg:`,
`idmdwnlmfv9://`, and page-context `idmrs://` resource markers. RustGet does not
copy these names or schemes. The public, standard interface we implement is the
Chrome/Firefox native-messaging framing and a loopback API.

### 2.3 Message shape

The minified background uses compact array opcodes (for example `case 38`,
`case 44`, message arrays). We did not attempt to reconstruct the entire
proprietary protocol. That would increase coupling and legal risk without
improving user-visible compatibility. RustGet defines its own documented JSON
protocol (see [browser bridge](05-browser-bridge.md)).

### 2.4 Confidence

| Hypothesis | Confidence | Basis |
|---|---|---|
| IDM uses `com.tonec.idm` as the native host | **High** | Literal in shipped JS |
| A remote WebSocket fallback exists | **High** | Literal URL construction in shipped JS |
| The extension observes request headers and response headers | **High** | `webRequest` permission and header manipulation code paths |
| The extension injects page-context scripts | **High** | `web_accessible_resources` + injection strings |
| Native protocol is an undocumented binary/JSON hybrid | **Medium** | Array opcodes visible; no full decode attempted |
| IDM silently intercepts downloads by default | **High** (product docs) | “Automatic mode” documented; not measured here |
| IDM stores resume metadata as a custom binary file | **Medium** | Vendor describes saved positions, no public format; not observed |

## 3. Protocol test lab: design

The lab is a local origin with deterministic content and adversarial modes. It
logs method, path, `Range`, `If-Range`, User-Agent, peer address and response
status for every request.

Modes implemented in `rustget-testkit`:

| Mode | Behaviour |
|---|---|
| honest | 206 + correct `Content-Range`, ETag, Last-Modified |
| ignore-range | 200 full body despite `Range`; no `Accept-Ranges` |
| lie-range | advertises `Accept-Ranges: bytes`, then returns 200 |
| flaky | writes N bytes then resets the connection |
| throttled | sleeps proportionally to bytes |
| redirects | 307 chain to the file endpoint |
| basic-auth | 401 challenge until correct Basic credentials |
| changing-etag | ETag changes on each request |
| omit-length | chunked response with no `Content-Length` |
| slow-first-range | delays only the range starting at byte 0, forcing a work-steal |
| content-disposition | RFC 5987 filename |
| HEAD delay | exercises probe timeout |

Controls: `GET /__admin/requests`, `GET /__admin/stats`,
`POST /__admin/reset`.

## 4. Protocol runs against RustGet (not IDM)

These runs validate our own engine and the lab. They are **not** claims about
IDM's internal requests.

### Run R1 — honest multi-range

- 2 MiB deterministic file, 4 configured connections, 64 KiB minimum.
- Observed: at least four `206` responses, each with distinct
  `Content-Range` starts; one HEAD probe; output SHA-256 matched.
- Conclusion: coordinator splits and writes correct offsets; no merger pass.

### Run R2 — slow first range

- 8 MiB file, 4 initial ranges, first range delayed 700 ms, 64 MiB/s throttle.
- Observed: more than four distinct range starts; output byte-exact.
- Conclusion: an idle worker steals from the largest active range; the
  cooperative stop boundary does not create overlap.

### Run R3 — no range support

- 700 KiB file with `Range` ignored.
- Observed: probe falls back to `GET Range: 0-0`, gets `200`; transfer restarts
  as one sequential stream; output byte-exact.
- Conclusion: fallback is safe and does not leave a partial multi-segment file.

### Run R4 — flaky origin

- 900 KiB file, two injected resets at 24 KiB, 50 MB/s.
- Observed: worker retries from the last written offset; final output exact.
- Conclusion: short reads are transient and do not corrupt the part file.

### Run R5 — changed validator

- ETag changes on every request.
- Observed: ranged request with stale `If-Range` gets `200`; engine reports
  `RemoteChanged` rather than appending a second representation.
- Conclusion: validator drift is detected; optional restart policy exists.

### Run R6 — kill process

- 8 MiB file, 1 MiB/s limit, journal checkpoint visible at ≥1 MiB, child killed
  with no destructor.
- Observed: journal last record recoverable; resume completes; SHA-256 matches;
  `.part` and journal removed.
- Conclusion: crash window is bounded by checkpoint interval plus one network
  chunk; output is never corrupted.

## 5. Hypotheses about IDM (to be confirmed with a licensed install)

These experiments are ready but were not run because IDM was not installed and
we chose not to execute the proprietary installer on the analysis host.

| ID | Hypothesis | Planned experiment | Success signal |
|---|---|---|---|
| H1 | Probe uses HEAD and falls back to a 1-byte GET | Log all requests for one URL; vary HEAD support | HEAD then `Range: bytes=0-0` |
| H2 | Initial connections equal configured limit (often 8) | Count initial concurrent GETs at t=0 | N simultaneous ranges |
| H3 | Split rule is midpoint of the largest active remaining range | Delay one range; record new boundaries | New start ≈ midpoint of victim remainder |
| H4 | Minimum split threshold is size-based | Vary file size and connection count | No split below threshold |
| H5 | Connections use HTTP keep-alive | Correlate peer port/`tcp.stream` with requests | Same TCP connection reused |
| H6 | Resume sends `If-Range` with ETag or Last-Modified | Change ETag between pause/resume | 200 restart or 206 continuation |
| H7 | Resume metadata is written at a fixed cadence | Kill at controlled times; diff temp files | Bounded position loss |
| H8 | 429/503 reduce or delay connections | Inject 429 for some ranges | Backoff/connection drop |
| H9 | Redirect handling preserves Range | 302/307/308 chain | Range header on final request |
| H10 | Extension talks native messaging first, WebSocket second | Remove native host; watch extension | Fallback WebSocket attempt |
| H11 | Per-host connection exceptions are stored persistently | Configure exception; restart app | Rule survives restart |
| H12 | Scheduler polls queue every 30 s | Complete file during window; watch next start | Up to 30 s delay |

## 6. Conclusions for design

1. **Split-on-idle, largest-first is correct and sufficient.** RustGet implements
   it with a ledger rather than copying segment state.
2. **Resume boundaries are allowed to change.** Our journal stores a complete
   range table, not a fixed initial partition.
3. **Cooperative stop boundaries are safer than aborting a socket.** A worker
   truncates its write at the split byte, so a newcomer can start immediately
   without overlap.
4. **A minimum segment size is mandatory.** Without it, small files thrash.
5. **Broad extension permissions are not necessary for a useful bridge.**
   Context menu + media panel + native host covers the primary workflows with a
   much smaller attack surface.
6. **Protocol compatibility with IDM is not a product requirement.** RustGet
   exposes a documented JSON bridge and a standard native-messaging frame.

## 7. Reproduction

```sh
cargo run -p rustget-testkit -- --size 8388608 --slow-first-range 700
cargo test -p rustget-net --test work_stealing -- --nocapture
cargo test -p rustget-cli --test crash_safety -- --nocapture
```

The lab is intentionally dependency-light and runs on loopback only.
