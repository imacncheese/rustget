# ADR-0008: Cooperative stop boundaries for work stealing

- **Status:** accepted
- **Date:** 2026-11-12

## Context

When a connection frees, IDM splits the largest active segment and the freed
connection starts on the second half. The naive implementation aborts the
victim's HTTP response. Aborting a stream can leave an in-flight write/chunk
ambiguous: did the victim write the bytes or not? Starting a newcomer at the
victim's last *reported* offset can overlap writes.

## Decision

Use a cooperative stop boundary:

- The ledger truncates the victim's range to `[start, split-1]`.
- The coordinator publishes `stop_at = split` in an atomic.
- The victim truncates every accepted chunk at `stop_at` and stops.
- The newcomer starts at `split`, disjoint from every permitted victim write.
- The victim's discarded bytes beyond `split` are re-fetched by the newcomer.

The victim may return `Stolen` instead of `Completed`; both are success outcomes
for the ledger. The coordinator waits for the stop only for accounting, not for
the newcomer's start.

## Alternatives considered

1. **Abort the request and read the atomic cursor.** Fast, but exact byte
   accounting across the abort is racy; a chunk may be written but not published.
2. **Wait for the victim to finish entirely, then split.** No overlap risk, but
   loses the acceleration benefit.
3. **Per-segment write locks.** Correct but unnecessary; ranges are disjoint.

## Consequences

- Up to one network chunk per split is wasted.
- No overlap, no corruption, deterministic tests.
- The newcomer can start before the victim acknowledges the stop.
- HTTP connection reuse is reduced because the victim drops a response body
  early; the newcomer may reuse the pooled socket for a new range.
