# ADR-0011: No telemetry, opt-in updates only

- **Status:** accepted
- **Date:** 2026-11-12

## Context

Download managers handle URLs, filenames and sometimes cookies. Telemetry is a
privacy risk and a trust liability. The product brief requires no telemetry by
default.

## Decision

- Ship no analytics, crash reporting, advertising or “phone home” code.
- Do not add dependencies whose default behavior reports usage.
- Update checks, when implemented, are disabled by default and require an
  explicit user action.
- Logs stay local. `RUST_LOG` controls verbosity; logs go to stderr.
- The UI makes no third-party requests; fonts are system fonts.

## Consequences

- No usage data for prioritization; rely on issues and the parity scorecard.
- Harder to detect crashes in the field; users must report them.
- Simple security review and a credible privacy claim.
- Any future telemetry would require a new ADR and explicit consent.
