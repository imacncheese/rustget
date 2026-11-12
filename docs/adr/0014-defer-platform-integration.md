# ADR-0014: Defer tray, OS autostart and power actions

- **Status:** accepted
- **Date:** 2026-11-12

## Context

The brief lists tray integration, autostart, single-instance, notifications and
power actions. These are OS-specific, hard to test in CI, and can cause data loss
(shutdown) or persistent system changes (autostart). The current UI is a browser
client, so there is no native event loop to host a tray.

## Decision

- Do not ship tray/autostart/power execution in 0.1.0.
- Keep `rustget-platform` as an empty, isolated crate for future OS integration.
- Scheduler records `PowerAction` and exposes it, but never executes it.
- Browser notifications via the Web Notification API are allowed; they are
  user-permissioned and require no native code.
- When implementing the native shell, use a single-instance lock and an explicit
  opt-in for autostart.
- Power actions require `allow_power_actions=true`, an empty queue re-check, and
  a test that uses a fake command runner.

## Consequences

- Missing expected desktop behavior; documented in the parity scorecard.
- No risk of the daemon shutting down a developer machine during tests.
- The API and scheduler model are ready for a native shell.
- This ADR must be superseded when the Tauri/platform work lands.
