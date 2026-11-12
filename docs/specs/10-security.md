# Security model

**Status:** controls implemented; independent audit not performed

## 1. Assets

- Downloaded file contents and destination paths.
- API bearer token.
- Cookies and Referer values passed by the browser extension.
- Download history and URLs in SQLite.
- The user's filesystem, via destination paths and post-actions.

## 2. Trust boundaries

```text
untrusted: remote server, redirects, headers, filenames, media manifests
semi-trusted: browser extension (user-installed, broad host permission)
trusted: local RustGet process and its profile directory
loopback peers: other local processes/users (same machine)
```

A local API token is not protection against a hostile local administrator; it is
protection against web pages, other browser origins and casual local processes.

## 3. Controls

### 3.1 Network

- TLS via `rustls`; system trust store through `rustls-native-certs` is planned,
  `webpki-roots` is currently used by reqwest's rustls feature.
- `Accept-Encoding: identity` for byte-exact ranges.
- Redirect limit (10 by default).
- Sensitive headers (`Authorization`, `Cookie`) must not be forwarded across
  origins by the HTTP client. `reqwest`'s redirect policy removes sensitive
  headers on cross-host redirects; this must be covered by a dedicated test.
- No automatic proxy credentials.
- No URL credentials logged.

### 3.2 SSRF and the local API

The API is a **local** service, not a proxy for arbitrary remote clients:

- binds loopback only; `rustget serve` refuses non-loopback;
- requires a 256-bit bearer token;
- WebSocket token in query (browser limitation);
- rejects cross-origin mutations;
- emits no CORS headers;
- 1 MiB body limit;
- no endpoint accepts a filesystem path for reading arbitrary files;
- `open` only opens the parent/known destination of an existing row.

The downloader itself can fetch arbitrary `http(s)` URLs by design. It is not
sandboxed from the user's network; that is the product's purpose.

### 3.3 Filenames and paths

Remote names pass through `sanitize_filename`:

- strips directory separators and control characters;
- replaces `<>:"/\|?*`;
- trims trailing spaces/dots;
- rejects `.`/`..` and Windows reserved device names;
- truncates to 180 characters preserving the extension;
- collision suffix ` (n)`.

The destination path is resolved by the caller or by the sanitized filename under
the configured directory. A malicious `Content-Disposition` cannot escape the
download directory in the default path.

### 3.4 Native host

- Standard length-prefixed framing, 4 MiB cap.
- Only `http`/`https` URLs.
- Token read from the profile; never sent back to the extension.
- No shell execution, no arbitrary commands.
- Logs to stderr; stdout is protocol-only.
- Does not start the daemon automatically.

### 3.5 Browser extension

- Minimal permissions compared with IDM's extension: no `proxy`,
  `management`, `webRequest`, `declarativeNetRequest`, or `downloads`.
- No remote code.
- Content script sends URL/referrer only, not page contents.
- Cookies are read for the requested URL only, then passed to the local host.
- The `<all_urls>` host permission is required for media detection across sites;
  it is a documented risk.

### 3.6 Post-download actions

Not implemented. The planned design:

- explicit allowlist of executables;
- no shell interpolation;
- arguments as an array, never a joined string;
- `[File]` substitution only for the completed path;
- sandbox/consent prompt before running anything;
- never auto-run an `.exe` just because it was downloaded.

### 3.7 Telemetry

None. No analytics SDK, no crash uploader, no “phone home”. Update checks, when
implemented, will be opt-in.

### 3.8 Secrets at rest

- API token file; Unix mode 0600.
- Cookies passed to a download are stored in the SQLite `headers` JSON column.
  This is a **known risk**: a session cookie can persist in the database. A
  future redesign will store sensitive headers in the OS keyring or redact them
  after the transfer.
- Basic auth fields are not persisted today (and not exposed in the UI).

## 4. Threat scenarios

| Scenario | Result |
|---|---|
| Malicious website opens `http://127.0.0.1:8765` | Can load static UI, cannot call API without token; no CORS read |
| Website issues cross-origin POST | 403 origin check |
| Local unprivileged process reads token file | Same-user trust boundary; possible. Full-disk encryption/OS ACLs required |
| Server returns `../../etc/passwd` filename | Sanitizer prevents traversal |
| Server redirects to `file://` | reqwest only supports http/https redirects |
| Server returns huge `Content-Length` | Sparse preallocation; disk-full handling; no memory allocation by size |
| Server slows to a trickle | Idle timeout |
| Server changes content mid-download | Validator drift detection |
| Browser extension compromised | Can submit arbitrary downloads and cookies; same trust as user |
| Malicious `.m3u8` | Parser is bounds-checked; wired downloader not yet enabled |
| Zip-slip on extraction | Extraction not implemented yet |

## 5. Known gaps

1. No dedicated redirect credential-leak test.
2. Cookie persistence in SQLite.
3. No OS keyring.
4. No signed binaries or update signature verification.
5. No fuzzing in CI yet.
6. No independent security audit.
7. No rate limiting on the local API beyond loopback/token.
8. Token file ACL on Windows inherits the profile directory; not explicitly
   hardened.
9. No certificate pinning (not recommended by default).
10. DNS rebinding to loopback is mitigated by requiring a token but not by a
    Host-header allowlist; add one.

## 6. Reporting

See [SECURITY.md](../../SECURITY.md). Do not open a public issue for a
vulnerability. Include the version, platform, reproduction and impact.

## 7. Verification

- API auth/origin tests (`crates/rustget-ipc/tests/api.rs`).
- Sanitizer tests (`crates/rustget-storage/src/paths.rs`).
- Native-host framing tests.
- `cargo-deny` config for licenses/advisories (CI).
- No unsafe code outside isolated platform crates; `#![forbid(unsafe_code)]` is
  on every current crate.
