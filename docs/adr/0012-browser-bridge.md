# ADR-0012: Browser bridge — standard native messaging, own JSON protocol

- **Status:** accepted
- **Date:** 2026-11-12

## Context

IDM's extension uses a proprietary array-opcode protocol over a native host and
a vendor WebSocket fallback. Reproducing it would be brittle, undocumented and
legally risky, and would not let users run RustGet without IDM's server.

## Decision

- Use the standard Chrome/Firefox native-messaging framing: 4-byte native-endian
  length + UTF-8 JSON.
- Define a small, documented request/response schema (`ping`, `add`, `list`,
  `status`, `pause`, `resume`).
- The native host is the only component holding the API token; it POSTs to the
  loopback API.
- Extension permissions are minimized: no `webRequest`, `proxy`, `management`,
  `declarativeNetRequest` or `downloads` in 0.1.0.
- Do not implement proprietary schemes (`idmreg:`, `idmdwnlmfv9://`).
- Do not auto-cancel browser downloads; provide explicit context-menu/media
  actions. Auto-interception is a future opt-in.

## Consequences

- RustGet does not work with IDM's extension and vice versa.
- Much smaller attack surface and a protocol we can test.
- The extension cannot observe every network request, so some media is missed.
- Users install the RustGet extension and native-host manifest.
- The bridge is testable with a Node/Python harness and a loopback daemon.
