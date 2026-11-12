# FTP specification

**Status:** plain FTP download and `REST` resume implemented; FTPS and segmented
FTP are not
**Code:** `crates/rustget-net/src/engine.rs` (`download_ftp`),
`crates/rustget-testkit/src/ftp.rs`

## 1. Supported URL form

```text
ftp://[user[:password]@]host[:port]/absolute/path
```

- Default port: 21.
- Credentials come from URL userinfo first, then `RequestOptions.basic_auth`.
- With neither, the client tries `anonymous` with an empty password.
- The path is percent-decoded before being sent to the server.
- `ftps://` is rejected with a typed error. No implicit TLS or AUTH TLS.

## 2. Transfer

1. Connect and log in.
2. Set binary transfer type.
3. Query `SIZE`. An unsupported `SIZE` is tolerated; the size is then unknown.
4. If a `.part` file exists and its length is ≤ remote size, use that length as
   the resume offset and issue `REST <offset>`.
5. `RETR` the file, streaming 64 KiB chunks into the positional part file.
6. On EOF, verify the remote size when known, call `sync_all`, optionally verify
   a checksum, then rename to the final path.

Progress is sampled from an atomic byte counter by the async coordinator every
200 ms. Cancellation is checked before every chunk read.

## 3. Resume semantics

- Resume is based on the actual `.part` length, not a journal.
- A part longer than the remote size is discarded and restarted at 0.
- A server that rejects `REST` currently fails the resume; it does not silently
  append from 0. This is deliberate to avoid duplicate bytes.
- There is no journal and no segmented FTP; a crash can lose at most one 64 KiB
  read plus the unwritten page cache.
- A malformed or changed server file with the same size is not detected beyond
  the optional checksum.

## 4. Security

- FTP is plaintext; credentials and data are visible to the network.
- Credentials embedded in the URL are persisted in the SQLite `url` column. This
  is a known risk and is called out in the security spec.
- Passive mode only; no active mode (`PORT`) is issued.
- The parser rejects non-FTP schemes before connecting.
- The test fixture checks USER/PASS and only serves one configured file.

## 5. Errors

| Condition | Mapping |
|---|---|
| 530/login | `NetError::Ftp` classified `Auth` |
| Connection/reset | transient |
| `SIZE` unsupported | continue with unknown size |
| `REST` rejected | fail; user may delete the part to restart |
| size mismatch | `ShortRead` |
| cancellation | `Cancelled` / paused |

## 6. Tests

- `downloads_ftp_file_and_resumes_with_rest`: 1 MiB via a local FTP fixture,
  byte-exact; then a 384 KiB prefix with `REST`, still byte-exact.
- `manager_downloads_ftp_without_http_probe`: manager path skips HTTP probing,
  persists the row and completes.
- The fixture is in `rustget-testkit` and supports only the commands the client
  needs.

## 7. Gaps

- FTPS (explicit/implicit TLS).
- Active mode and proxy traversal.
- Segmented FTP and a dedicated journal.
- Directory listings/site-grabber integration.
- Credential storage in the OS keyring.
- IPv6 passive-mode address rewriting (EPSV).
