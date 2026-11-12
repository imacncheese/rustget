# ADR-0010: Scheduler semantics — weekly windows, scheduler-owned pauses

- **Status:** accepted
- **Date:** 2026-11-12

## Context

IDM schedules queue starts/stops and power actions. A download manager must not
silently resume a download the user paused manually, and an overnight window must
work across midnight and weekday boundaries.

## Decision

- Represent a schedule as a weekday bitmask + local start/stop minutes.
- Support overnight windows by checking “today after start” or “yesterday before
  stop”.
- Union overlapping schedules (any active = open).
- Pause active downloads when the window closes and mark them with
  `error = "paused by scheduler"`.
- On open, resume only rows carrying that marker, plus promote queued rows.
- Persist schedules in SQLite; daemon ticks every 10 s.
- Persist power actions but do not execute them until an explicit
  `allow_power_actions` setting and a tested platform implementation exist.

## Alternatives considered

- **Start/stop timestamps instead of weekly rules.** Simpler but doesn't repeat;
  IDM's scheduler is recurring.
- **A separate scheduler daemon.** More moving parts; the download daemon is
  already the lifecycle owner.
- **Resume every paused row on open.** Dangerous: overrides the user.
- **Execute shutdown immediately on queue empty.** Risk of data loss and tests
  shutting down CI; deferred.

## Consequences

- Scheduler pauses are distinguishable from user pauses.
- Clock changes are wall-clock transitions, which is what users expect for
  “01:00 local”.
- A crash loses the in-memory open/closed flag; the next tick re-evaluates.
- Power actions are a visible gap, not a hidden behavior.
