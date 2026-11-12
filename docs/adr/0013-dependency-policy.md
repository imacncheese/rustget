# ADR-0013: Dependency and supply-chain policy

- **Status:** accepted
- **Date:** 2026-11-12

## Context

A download manager parses untrusted headers, filenames, XML and JSON, and links
TLS and SQLite. Dependency mistakes are security and maintenance risks. The brief
asks for well-maintained crates, pinned versions and justification for heavy
dependencies.

## Decision

- Commit `Cargo.lock`; applications ship locked.
- Use caret requirements in `Cargo.toml` for libraries, exact resolved versions
  in the lock file.
- No wildcard requirements (`deny.toml`).
- Allow only OSI licenses in the allowlist; `cargo-deny` in CI.
- `cargo-audit` (best effort) and `cargo-deny advisories` in CI.
- First-party crates use `#![forbid(unsafe_code)]`.
- Avoid crates that execute code at build time beyond widely used build scripts
  (`cc`, `bindgen` is not used).
- No telemetry/analytics dependencies.
- Prefer rustls over system OpenSSL for reproducibility and cross-platform
  builds.

## Justified heavy dependencies

| Crate | Why | Cost |
|---|---|---|
| `tokio` | multi-threaded async runtime, timers, process, channels | large but standard |
| `reqwest` | TLS, redirects, streaming, HTTP/2, proxies | pulls hyper/rustls; saves months |
| `axum` | local API + WS with tower ecosystem | moderate |
| `rusqlite` (bundled) | transactional catalogue without a DB server | compiles SQLite C |
| `rustls` | memory-safe TLS | crypto stack |
| `serde`/`serde_json` | wire/journal formats | compile time |
| `roxmltree` | safe DASH XML parsing | small |
| `proptest` | invariant generation | dev only |
| `criterion` | benchmarks | dev only |

## Consequences

- `cargo build` compiles SQLite and a TLS stack; total build time is tens of
  seconds on a modern machine.
- The license allowlist must be updated for any new dependency.
- A future MSRV bump must be recorded and tested.
- `cargo-deny` is the gate; a manual exception requires an ADR note.
