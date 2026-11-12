# Browser bridge specification

**Status:** Chromium/Firefox MV3 source, native host, loopback API and opt-in
download takeover implemented
**Code:** `extensions/chromium`, `extensions/firefox`,
`crates/rustget-native-host`, `crates/rustget-ipc`
**Related:** [ADR-0014](../adr/0014-browser-bridge.md)

## 1. Goals and non-goals

**Goals**

- One-click “Download with RustGet” from a context menu.
- Detect directly playable media and manifest URLs and offer them in a small
  floating panel.
- Pass page URL, Referer and cookies to the local daemon.
- Work without a remote service, without analytics, and without broad
  `webRequest`/`proxy`/`management` permissions.

**Non-goals**

- Reproducing IDM's proprietary array-opcode protocol or custom `idmreg:`/
  `idmdwnlmfv9://` schemes.
- Silently cancelling every browser download. Takeover is **default off** and
  only occurs after RustGet accepts the request.
- DRM circumvention or media decryption.

## 2. Components

```text
web page
  |  content script (media detection, floating panel)
  v
extension service worker
  |  chrome.runtime.sendNativeMessage("com.rustget.app", JSON)
  v
rustget-native-host  (stdio, 4-byte length framing)
  |  HTTP + Bearer token, loopback only
  v
rustget serve  (/api/v1)
  |
  v
download engine
```

The native host is a separate process launched by the browser. It has no UI and
no network listener. It reads the daemon token from the RustGet profile
directory.

## 3. Native messaging framing

Chrome and Firefox use the same framing:

```text
uint32 length (native endian) | length bytes of UTF-8 JSON
```

Limits:

- host accepts 1 byte ≤ length ≤ 4 MiB;
- zero-length frames are rejected;
- malformed JSON returns an error response, not a crash;
- stdout is reserved for frames; all logs go to stderr.

Request envelope:

```json
{ "type": "add", "url": "https://example.com/a.zip", "referrer": "https://example.com/", "cookie": "sid=..." }
```

Response envelope:

```json
{ "ok": true, "data": { "...": "..." } }
{ "ok": false, "error": "human readable reason" }
```

### 3.1 Message types

| Type | Fields | Result |
|---|---|---|
| `ping` | — | version and API base |
| `add` | `url`, optional `referrer`, `cookie`, `filename` | created download record |
| `list` | — | downloads array |
| `status` | `id` | one download plus live progress |
| `pause` | `id` | updated record |
| `resume` | `id` | updated record |

Only `http` and `https` URLs are accepted by the host and by the manager. `file:`,
`javascript:` and custom schemes are rejected.

## 4. Extension permissions

Chromium manifest:

```json
{
  "permissions": ["contextMenus", "nativeMessaging", "storage", "cookies", "activeTab", "downloads"],
  "host_permissions": ["<all_urls>"],
  "background": { "service_worker": "background.js" },
  "options_ui": { "page": "options.html", "open_in_tab": false },
  "content_scripts": [{ "matches": ["http://*/*", "https://*/*"], "js": ["content.js"] }]
}
```

Firefox uses the same sources with `background.scripts` and a
`browser_specific_settings.gecko.id`.

`downloads` is used only for the opt-in takeover path. Without the option enabled,
the extension never cancels or erases a browser download.

No remote code is loaded. The content script is plain JavaScript shipped in the
package. There is no eval, no `fetch` to a vendor, and no telemetry.

## 5. Installation

### 5.1 Build the native host

```sh
cargo build --release -p rustget-native-host
```

The binary is `target/release/rustget-native-host` (`.exe` on Windows).

### 5.2 Native host manifest

Edit `extensions/native-host/com.rustget.app.json`:

```json
{
  "name": "com.rustget.app",
  "description": "RustGet native messaging bridge",
  "path": "/absolute/path/to/rustget-native-host",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://<extension-id>/"]
}
```

Placement:

| OS | Chrome/Chromium/Edge |
|---|---|
| Windows | `%APPDATA%\Google\Chrome\NativeMessagingHosts\com.rustget.app.json` (and the Edge/Chromium equivalents) |
| macOS | `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/com.rustget.app.json` |
| Linux | `~/.config/google-chrome/NativeMessagingHosts/com.rustget.app.json` |

Firefox uses its own directory (`~/.mozilla/native-messaging-hosts/` or
`%APPDATA%\Mozilla\NativeMessagingHosts\`). The manifest must list Firefox's
extension origin.

The daemon must have created `api-token` in its profile directory. The host
looks in `RUSTGET_DATA_DIR` first, then the platform data directory + `RustGet`.

### 5.3 Load the extension

- Chromium: `chrome://extensions` → Developer mode → Load unpacked →
  `extensions/chromium`.
- Firefox: `about:debugging#/runtime/this-firefox` → Load Temporary Add-on →
  `extensions/firefox/manifest.json`.

Store packaging is intentionally not performed.

## 6. UI behaviour

- Context menu on links, video and audio: “Download with RustGet”.
- Context menu on the page: “Download page with RustGet”.
- Toolbar action downloads the first detected media, or the current page.
- Options page:
  - “Take over browser downloads” (default **off**): on `downloads.onCreated`,
    send the URL to RustGet, wait for acceptance, then cancel the browser item.
    `blob:`/`data:` and extension-originated downloads are ignored. On any native
    host error the browser download is left untouched.
  - “Show media panel” (default on): content script checks the stored setting
    before rendering and reacts to changes.
  - “Test daemon connection”: sends a `ping` through the native host.
- Content script:
  - scans `video`/`audio`/`source` and links ending in common media/manifest
    extensions;
  - displays a floating panel with up to five items;
  - sends only a URL and page referrer, never page contents;
  - observes DOM mutations with a debounce and a 5 s fallback scan.
- Badge shows the number of detected media and briefly shows “↓” after a
  successful add or “!” on error.

## 7. Security

- Native host only accepts loopback API base by default; `RUSTGET_API` is a
  development override.
- Token comparison is length-checked and constant-time over the token bytes.
- Browser-provided Referer/Cookie are sent as headers to the download request.
  This is necessary for authenticated media but means the extension must be
  trusted; it is not a sandbox.
- Cookies are read only for the requested URL via `chrome.cookies.getAll`.
- The API itself requires `Authorization: Bearer <token>`; the extension does not
  receive the token. The native host holds it.
- The extension's `<all_urls>` host permission is broad by necessity for media
  detection; content scripts never exfiltrate DOM data.

## 8. Failure modes

| Failure | Visible result |
|---|---|
| Daemon not running | badge “!”; tooltip explains `rustget serve` |
| Native host missing | browser reports host not found; badge error |
| Token missing | host exits with a clear stderr message |
| URL unsupported | host returns `ok:false` |
| API 401 | host returns error; token may be stale |
| Duplicate add | manager creates a separate row with a unique filename |

## 9. Tests

- Native host framing round-trip and oversized-frame rejection.
- Live test: spawn host with `RUSTGET_API` and `RUSTGET_DATA_DIR`, send `ping`
  then `add`; daemon creates a record.
- Extension JS syntax checked with `node --check`; not yet run in a real browser
  automation harness.

## 10. Future work

- Browser-driven tests for the takeover flow (Playwright/WebDriver).
- `webRequest` sniffing for HLS/DASH manifests, gated behind an optional
  permission.
- Signed store packages and native host installers.
- TypeScript sources and a bundler.
- Per-site allow/deny lists.
