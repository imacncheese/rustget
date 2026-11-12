# UI/UX specification

**Status:** main window, add dialog, details, settings and scheduler timeline
implemented; video picker and first-run wizard are design-only
**Code:** `crates/rustget-app/assets`
**Reference screenshots:** [main](../assets/ui-main.png),
[first-run wizard](../assets/ui-wizard.png),
[German UI](../assets/ui-de-main.png),
[live speed](../assets/ui-speed.png), [scheduler](../assets/ui-scheduler.png)

## 1. Design principles

1. **Two-second comprehension.** A non-technical user should be able to start a
   download from the first screen without opening a manual.
2. **Expert depth on demand.** Connection counts, per-segment progress, headers
   and speed limits exist, but are folded into the detail pane or “advanced”.
3. **Never lie.** No fake speed graphs, no invented ETAs when size is unknown,
   no “complete” until the final rename succeeds.
4. **Keyboard and screen reader first-class.** Every action has a focus target
   and a label.
5. **The engine is remote.** The UI never owns a download. Closing the window does
   not cancel the daemon; it is a view over the API.

## 2. Main window

```text
┌───────────────────────────┬──────────────────────────────────────────────┬────────────────────────────┐
│ RustGet                   │  ⌕ Search downloads          [ + Add ]       │                            │
│                           ├──────────────┬──────────────┬──────────────┬──┤  Select a download to      │
│ ● All           12        │ Active  2    │ Speed 18 MB/s│ Done 8       │  │  see its connections,      │
│ ○ Downloading    2        ├──────────────┴──────────────┴──────────────┴──┤  segments and live         │
│ ○ Queued         1        │ NAME             SIZE   PROGRESS  SPEED  ST.│  progress.                 │
│ ○ Paused         0        ├──────────────────────────────────────────────┤                            │
│ ○ Completed      8        │ ▣ ubuntu.iso    2.1 GB ███████░░ 74% 9 MB/s │                            │
│ ○ Failed         1        │ ♪ album.zip    480 MB  ████░░░░ 40% 4 MB/s  │                            │
│                           │ ≡ report.pdf   8.2 MB  ████████ 100% done  │                            │
│                           │                                              │                            │
│                           │                                              │                            │
│ ◐ Theme      ⚙ Settings   │                                              │                            │
└───────────────────────────┴──────────────────────────────────────────────┴────────────────────────────┘
```

### 2.1 Layout

- 248 px sidebar, fluid table, 360 px details panel.
- At < 1080 px the details panel becomes a drawer.
- At < 760 px the sidebar becomes a top filter bar and low-priority columns hide.
- The table is virtualized: only visible rows + 4-row overscan are in the DOM.

### 2.2 Table columns

| Column | Content | Sort |
|---|---|---|
| Name | icon, filename, category · age | lexicographic |
| Size | total size, or downloaded if unknown | numeric |
| Progress | segmented bar + percent | ratio |
| Speed | live speed for active rows | numeric |
| Status | badge: queued/downloading/paused/completed/failed | lexicographic |

The default order is newest first. Multi-select is not yet implemented.

### 2.3 Row states

- Hover: subtle surface lift.
- Selected: accent inset bar and tinted background.
- Failed: red progress track and a persistent error row in details.
- Completed: green track, 100%.
- Unknown size: progress bar indeterminate; percent hidden; ETA “—”.

## 3. Detail pane

```text
┌────────────────────────────────────────────┐
│ ▣  ubuntu-24.04.iso             [downloading] │
│                                             │
│ ███████████████████████░░░░░░░  74%         │
│                                             │
│ Downloaded  1.6 GB     Speed   9.1 MB/s      │
│ ETA         1m 12s     Added   just now      │
│                                             │
│ CONNECTIONS                          8 / 8   │
│ ██████ ██████ ██████ ██████ ██████ ████ ███  │
│ ████   ██     ████   ██     ██    ██   ██    │
│                                             │
│ SAVED TO  D:\Downloads\ubuntu-24.04.iso      │
│ SOURCE    https://.../ubuntu-24.04.iso       │
│                                             │
│ [Pause] [Resume] [Open folder] [Remove]     │
└────────────────────────────────────────────┘
```

Segment bars use live `Progress` events. Each segment's width is proportional to
its byte length; its inner fill is `written / length`. A complete segment is
green; active is teal; pending is dimmed.

The detail pane is updated over WebSocket, so it remains live when the table is
sorted or filtered differently.

## 4. Add download dialog

```text
┌──────────────────────────────────────────────┐
│ Add download                              ✕  │
│                                              │
│ URL    [ https://example.com/file.zip  ] [Paste] │
│                                              │
│ Save folder [ D:\Downloads               ]   │
│ Connections [ 8 ]                            │
│ Speed limit [ 0.0 MB/s ]                     │
│ Category    [ General ▾ ]                    │
│ [x] Start immediately                        │
│                                              │
│ RustGet probes the server before adding.     │
│                                              │
│                        [Cancel] [Add download]│
└──────────────────────────────────────────────┘
```

Current behaviour: the probe happens on submit; the dialog closes when the row
is created. Planned improvement: debounce a probe after a valid URL and show
filename/size/range support before submit.

### 4.1 Validation

- URL must parse and use `http`/`https`.
- Connections clamp 1–32.
- Speed limit accepts fractional MB/s; zero means unlimited.
- Folder is created if missing; collisions get ` (1)` automatically.
- The submit button is disabled while the request is in flight.

## 5. Settings

Current small dialog: API token, theme (system/dark/light), completion
notifications. Planned searchable settings tree:

```text
General       startup, language, updates, silent mode
Downloads     default folder, connections, segment minimum, checksums
Network       proxy/PAC, per-host caps, timeouts, HTTP/3
Scheduler     weekday timeline, power actions
Categories    name, folder, extensions
Browser       extension status, native host path, site allowlist
Advanced      fsync policy, journal interval, log level, API port
```

Settings must be simple by default and advanced collapsed. Every setting needs a
stable key, a default, a validation rule and a migration note (see
[settings schema](08-settings-schema.md)).

## 6. Scheduler timeline (design)

```text
           00 02 04 06 08 10 12 14 16 18 20 22 24
Monday     ░░░████████░░░░░░░░░░░░░░░████████░░
Tuesday    ░░░████████░░░░░░░░░░░░░░░████████░░
Wednesday  ░░░████████░░░░░░░░░░░░░░░████████░░
...
```

- A seven-row timeline renders API-backed windows.
- Add/delete through the same `/api/v1/schedules` contract as the CLI.
- Overnight windows split across midnight.
- Each row is a day; blocks are schedule windows.

Implemented: rendering, add/delete, validation, overnight split. Not implemented:
drag-to-create, 15-minute snapping, overlap hatching, a “now” line, and an
editable block dialog. The screenshot is
[ui-scheduler.png](../assets/ui-scheduler.png), and the live chart in
[ui-speed.png](../assets/ui-speed.png).

## 7. Interaction model

### 7.1 Keyboard

| Key | Action |
|---|---|
| `Ctrl/Cmd+N` | Add download |
| `Ctrl/Cmd+F` | Focus search |
| `Space` | Pause/resume selected |
| `Delete` | Remove selected (with confirmation) |
| `↑`/`↓` | Move selection, scroll into view |
| `Enter` | Open containing folder |
| `Esc` | Clear selection / close dialog |
| `Tab` | Native focus traversal |

The details pane draws a live speed sparkline from the last 60 WebSocket
progress samples, scaled to the 95th percentile so a single burst does not
flatten the chart. It includes the peak label and fills the area under the line.

The sidebar **Logs** dialog polls `/api/v1/logs` every 2 seconds and shows the
last 300 INFO+ daemon events with level colouring. Stderr remains the full log
stream.

Settings has **Export config** and **Import config** actions. The export is the
same JSON bundle as the API (downloads, settings, categories, schedules); import
reloads the queue after applying the bundle.

`rustget handle-url "rustget://add?url=..."` parses the custom scheme and submits
the download to the running local API. `rustget scheme install|status|remove`
and `rustget associate install|status|remove <ext>` register the scheme or a
file extension; registration writes are unverified in the sandbox.

On Windows, `rustget-tray` provides a native tray icon with **Open RustGet**
(opens `http://127.0.0.1:8765`, or `RUSTGET_UI_URL`) and **Quit**. It is a
separate optional binary; menu interaction is not automated yet.

Settings includes a live search filter and a collapsible **Advanced** section;
the language selector switches the core UI between English and
German using `assets/i18n.js` and `data-i18n` attributes; the choice is persisted
in `localStorage` and `?lang=de` is supported for testing. Dynamic strings and
Fluent message formatting are not complete yet.

With no token (and no valid saved token), the first-run wizard opens instead of
the settings form: a welcome step, then a token field whose value is tested
against `/api/v1/stats` before being saved. Closing the wizard still leaves the
Settings token field available.

Dropping an `http(s)` URI list anywhere on the window opens the add dialog with
the first link prefilled and a drop overlay. Local files are rejected with an
explanation; RustGet has no local-file import path yet.

For `.m3u8`/`.mpd` URLs the **Check** action also loads the quality catalog and
reveals a **Media quality** select. The chosen index is persisted as
`media_variant`; Auto keeps the previous highest-bandwidth behavior.

The add dialog has a **Check** action next to the URL. It calls
`POST /api/v1/probe` and shows the filename, size and whether the origin supports
ranges before the user commits the download.

The details pane shows tag chips with a remove action and an **Add tag** input;
the search box also matches tags. The add dialog accepts comma-separated tags, an optional HTTP/SOCKS5 proxy URL, Basic
and Digest username/password, an optional PEM client-certificate path for mutual TLS, and a
**Resolve with yt-dlp** checkbox, **Use persistent cookie jar**, and **Extract
ZIP after download**.

The details actions include **Move up/Move down**, which PATCH the queue priority.
There is no drag reorder yet.

### 7.2 Screen reader

- Sidebar is a `nav` with an accessible label.
- Table uses `role="row"` and named action buttons.
- Progress has a text equivalent (`74%`), not color alone.
- Status badges include text.
- Live region announces completion and errors.
- Focus returns to the add button after a dialog closes.

### 7.3 Motion

Respects `prefers-reduced-motion`; no animated progress beyond a linear width
transition.

## 8. Visual system

| Token | Dark | Light |
|---|---|---|
| background | `#0b0d12` | `#f4f6fa` |
| panel | `#11141c` | `#ffffff` |
| text | `#eef1f7` | `#161b26` |
| muted | `#8c95a8` | `#667085` |
| accent | `#ff7a45` | `#e85d26` |
| success | `#43d17a` | `#1f9d55` |
| danger | `#ff5f6d` | `#d92d3f` |

System font stack, 14 px base, 52 px rows, 14 px panel radius, 10 px controls.
No raster assets except the favicon; all icons are text glyphs today, with an SVG
icon set planned.

## 9. Performance

- Virtualized list renders ~20 rows regardless of 10,000 records.
- WebSocket coalesces UI updates to ~4/s from the manager; React-style full
  rerender is not used.
- Search is debounced at 90 ms.
- No layout read in a write loop; scroll rendering is `requestAnimationFrame`
  throttled.

## 10. Current gaps

1. Video quality picker, first-run wizard, scheduler drag editing.
2. Multi-select and batch actions.
3. Queue reordering UI.
4. Speed graph.
5. Screen-reader audit with NVDA and VoiceOver.
6. Tauri/native window shell (see [ADR-0004](../adr/0004-ui-shell.md)).
7. Mobile layout for the details drawer.
8. Empty-state copy when a filter has no matches (“No downloads yet” is shown
   instead of “No matching downloads”).
