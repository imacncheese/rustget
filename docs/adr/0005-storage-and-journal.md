# ADR-0005: SQLite metadata + JSONL resume journal + `.part` output

- **Status:** accepted
- **Date:** 2026-11-12

## Context

A download manager needs three different durability scopes:

1. the user-visible output file must never be half-written;
2. per-transfer progress must survive a crash within a bounded loss;
3. the download list, settings and queue must survive restarts and support
   queries.

Using one mechanism for all three either makes progress writes too expensive
(SQLite transaction per chunk) or makes the catalogue fragile (a per-download
text file per row).

## Decision

- **Final output:** a `.part` file with a fixed length and positional writes.
- **Progress:** an append-only JSONL journal of full segment snapshots, plus
  atomic compaction.
- **Catalogue/queue/settings:** SQLite in WAL mode.

The part file is the source of bytes; the journal is the source of *accounting*.
SQLite never stores the byte cursor as authority; it stores the user-facing row.

## Consequences

- Recovery is simple: last parseable journal record wins.
- Torn tails are safe; duplicate/stale records only cost re-fetch.
- Journal space grows with time between compactions; 512-record compaction
  bounds it.
- SQLite and the journal can disagree; the journal wins for resume, SQLite wins
  for list/status.
- `fsync_checkpoints=false` is process-crash-safe but not power-loss-safe. This
  is explicit and configurable.
- A future binary/delta journal can be introduced behind a version field.
