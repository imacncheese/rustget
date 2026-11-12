# ADR-0009: Local API security model

- **Status:** accepted
- **Date:** 2026-11-12

## Context

The daemon exposes download control and file deletion to a local HTTP server.
Any web page in the user's browser can attempt requests to loopback. A
download-manager API is a high-value target: it can fetch arbitrary URLs, write
files and open folders.

## Decision

1. Bind loopback only; refuse non-loopback in `rustget serve`.
2. Generate a 256-bit bearer token per profile, stored in `api-token`.
3. Require `Authorization: Bearer` on every API route except health.
4. WebSocket uses a query token because browsers cannot set handshake headers;
   accept this only on the WS route.
5. Reject mutations when `Origin` is present and does not match `Host` or an
   explicit allowlist.
6. Emit no CORS headers.
7. Cap request bodies at 1 MiB.
8. Never accept a filesystem path to read; only download destinations and
   known sidecars can be opened/deleted.
9. Static UI is public but tokenless; token is read from the URL fragment or
   localStorage.
10. Constant-time token comparison.

## Alternatives considered

- **No auth on loopback.** Rejected: any web page could POST to the API.
- **Cookie session.** Requires CSRF handling and a login flow; bearer is simpler
  for a local tool.
- **Unix socket/named pipe.** Stronger on Unix, unavailable uniformly on Windows;
  a future option.
- **CORS allowlist.** Rejected: no browser origin needs cross-origin access.

## Consequences

- Users must handle a token; the CLI prints a ready-to-open URL.
- A local process running as the same user can read the token file. That is the
  same-user trust boundary; OS ACLs are the mitigation.
- API routes require a loopback `Host` header (`127.0.0.1`, `localhost`, `::1`),
  closing the DNS-rebinding gap. A missing Host is rejected.
- The token still appears in the WS query; local log exposure is documented.
- The token appears in the URL fragment (not sent to the server) and in the WS
  query (may appear in local logs). Acceptable for loopback, documented.
