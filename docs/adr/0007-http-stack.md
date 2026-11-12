# ADR-0007: HTTP stack — reqwest + rustls, own pool later

- **Status:** accepted
- **Date:** 2026-11-12

## Context

The brief suggests `hyper` + `rustls` and owning the connection pool so segment
connections can be reused deliberately. Building that from scratch before the
engine works would delay correctness. `reqwest` provides redirects, proxies,
rustls, HTTP/2, streaming bodies and a connection pool.

## Decision

Use `reqwest 0.12` with `rustls-tls`, `stream`, `http2`, disabled gzip/brotli,
and `Accept-Encoding: identity` for downloads.

- Each download has its own `Client`, and therefore its own pool, with
  `pool_max_idle_per_host = max_connections`.
- Range requests are issued per worker; completed ranges release the connection
  back to the pool.
- The engine does not yet pin a connection to a specific worker or introspect
  socket reuse.

## Consequences

- Faster path to a correct engine and TLS without OpenSSL.
- Less control over exact socket reuse, HPACK, GOAWAY behavior and per-host
  limits.
- HTTP/2 is enabled but not tested against a real h2 origin.
- HTTP/3 cannot be added through `reqwest` 0.12; it requires `quinn`/`h3` or a
  newer stack.
- If benchmarks show pool behavior is the bottleneck, replace the transport
  behind `NetClient` with a `hyper` pool; the coordinator API stays stable.
