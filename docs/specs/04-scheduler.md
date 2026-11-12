# Scheduler semantics

**Status:** window enforcement implemented; power actions recorded but not
executed
**Code:** `crates/rustget-queue/src/scheduler.rs`,
`crates/rustget-queue/src/manager.rs`
**Related:** [ADR-0011](../adr/0011-scheduler-semantics.md)

## 1. Model

A schedule is a weekly local-time window plus an optional completion action.

```text
Schedule {
  id: UUID,
  name: String,
  enabled: bool,
  days: u8,            // bit 0 = Monday ... bit 6 = Sunday
  start_min: Option<u16>, // minutes after local midnight
  stop_min: Option<u16>,
  action: none | shutdown | sleep | hibernate | exit
}
```

`start_min == stop_min` is a zero-length window (never active). At least one of
start/stop is normally present; both absent means the row is inert.

### 1.1 Normal window

If `start <= stop`:

```text
active = day_selected(today) && start <= minute < stop
```

### 1.2 Overnight window

If `start > stop`:

```text
active = (day_selected(today) && minute >= start)
      || (day_selected(yesterday) && minute < stop)
```

This lets “Monday 23:00–06:00” include Tuesday 00:00–05:59 while correctly
excluding Wednesday 05:00.

### 1.3 Multiple schedules

The window is open when **any** enabled schedule is active. Overlaps are a union.
A future version may add per-queue schedules; today the window governs the whole
manager.

## 2. State machine

```text
             window opens                      window closes
   [closed] ----------------> [open] ----------------------> [closed]
       ^                         |
       |                         | downloads complete + action
       +--- daemon restart ------+
```

On the **open** transition:

1. Query paused rows whose `error == "paused by scheduler"`.
2. Clear the marker, set status `queued`, broadcast update.
3. Promote queued rows up to `max_concurrent`.

On the **close** transition:

1. Query rows in `downloading`.
2. Cancel each worker through the manager, retaining the journal and part.
3. Set status `paused`, `error = "paused by scheduler"`.
4. Broadcast updates.

Why a marker instead of a separate state? It distinguishes scheduler-owned
pauses from user pauses. A user-paused download is never silently resumed when
the window opens.

## 3. Timing

- The daemon ticks every 10 seconds.
- Decisions use local wall-clock time (`chrono::Local`).
- The boundary is inclusive at start and exclusive at stop.
- A missed tick after sleep is handled by the next tick; the transition is
  evaluated from current wall time, not from a queue of events.
- Clock changes (DST, manual jump) are treated as normal wall-clock transitions.
  There is no monotonic timeline for schedules.

IDM publicly documents a 30-second queue-state poll. RustGet's 10-second tick is
faster and independent; this is a deliberate deviation.

## 4. Persistence

SQLite table:

```sql
CREATE TABLE schedules (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  days INTEGER NOT NULL DEFAULT 127,
  start_min INTEGER,
  stop_min INTEGER,
  action TEXT NOT NULL DEFAULT 'none'
);
```

A schedule survives restart. The daemon re-evaluates the current window on its
first tick; it does not remember the previous process's open/closed state.

## 5. API and CLI

```http
GET    /api/v1/schedules
POST   /api/v1/schedules
DELETE /api/v1/schedules/{id}
```

Example create:

```json
{
  "name": "night",
  "start": "01:00",
  "stop": "06:00",
  "days": "mon,tue,wed,thu,fri",
  "action": "none"
}
```

CLI:

```sh
rustget schedule add --name night --start 01:00 --stop 06:00 --days mon,tue
rustget schedule list
rustget schedule remove <uuid>
```

## 6. Power actions

`PowerAction` is persisted and returned by the API. Execution is intentionally
**disabled** in the shipping binary because an untested shutdown path is a data
loss risk. The planned contract:

- execute only after the queue is empty;
- require explicit `allow_power_actions = true`;
- re-check that no active tasks exist immediately before invoking the OS;
- log the exact command;
- never run in tests or CI.

This is a documented gap, not an accidental omission.

## 7. Per-queue concurrency

Today `max_concurrent` is global. The `queues` table exists as a forward
compatibility placeholder. A future per-queue scheduler will:

1. select the highest-priority enabled queue;
2. enforce its concurrency independently;
3. share the global network budget;
4. preserve FIFO within a priority tier.

## 8. Tests

- `daytime_window`
- `overnight_window_uses_previous_day_selection`
- `time_parsing`
- manager integration exercises the queue promotion path via normal queueing.

Missing tests: a clock-injection integration test that advances local time across
a boundary. The pure function is tested; the daemon loop is a thin caller.

## 9. Open questions

- Should schedule windows pause only scheduler-started downloads? Current
  behaviour pauses all active downloads, which matches “stop queue” semantics but
  can surprise a user who started one manually.
- Should start/stop actions be per-queue rather than global?
- How should auto-shutdown behave when a download is paused by the user at the
  moment the queue empties?
