# ADR-0001: Clean-room boundary and reverse-engineering policy

- **Status:** accepted
- **Date:** 2026-11-12
- **Decision owners:** RustGet maintainers

## Context

The product brief asks for a download manager matching Internet Download
Manager, and explicitly permits reverse engineering of IDM. A feature-compatible
product can be built from public behaviour, but copying code, assets, secrets or
protected protocol implementations would create legal and security risk. IDM is
proprietary commercial software.

## Decision

1. Feature requirements are derived from **public documentation, published UI,
   and black-box protocol observation**.
2. The publicly shipped browser extension may be statically inspected to
   understand its **public interface** (manifest, permissions, host name,
   transport framing). It is not redistributed.
3. We do **not** execute the proprietary installer, bypass registration/licensing,
   unpack protected binaries, or decode proprietary protocols beyond what is
   needed for interoperability research.
4. We do **not** copy IDM source, assets, strings, icons, protocol identifiers or
   message formats. RustGet defines its own documented JSON bridge and CLI.
5. Analysis evidence stays in an untracked directory and is not committed.
6. All production code is original Rust with an OSI license.

## Consequences

- Some IDM behaviours remain unverified (see the hypotheses table in
  `docs/specs/01-behavior-observations.md`).
- RustGet will not be wire-compatible with IDM's native host or browser
  extension. That is acceptable; users install the RustGet extension.
- We can publish the source and test lab without carrying third-party code.
- If a licensed IDM install becomes available, the documented experiments can be
  run without changing this boundary.
