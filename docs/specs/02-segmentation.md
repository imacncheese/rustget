# Segmentation and work-stealing specification

**Status:** implemented and property-tested
**Code:** `crates/rustget-core/src/ledger.rs`, `crates/rustget-net/src/engine.rs`
**Related:** [resume journal](03-resume-journal.md), [ADR-0008](../adr/0008-work-stealing-boundaries.md)

## 1. Model

A download of `N` bytes is represented as a partition of the half-open byte
interval `[0, N)` into non-empty, contiguous, disjoint ranges. Ranges are
represented internally as inclusive `ByteRange { start, end }` to match HTTP
`Range` semantics.

For every segment `i`:

- `start_i` is the first byte to fetch.
- `end_i` is the last byte that belongs to the segment.
- `written_i` is the absolute offset of the next byte to write.
- Invariant: `start_i <= written_i <= end_i + 1`.
- `remaining_i = max(0, end_i + 1 - written_i)`.

Partition invariants:

```text
start_0 = 0
start_{i+1} = end_i + 1
end_k = N - 1
sum(len_i) = N
```

The ledger is a single-owner state machine. Workers do not mutate the ledger;
they publish an `AtomicU64` write cursor. The async coordinator is the only
mutator. This makes every scheduling decision deterministic and unit-testable.

## 2. Probe

1. Send `HEAD` with `Accept-Encoding: identity`.
2. If the response is successful and advertises `Accept-Ranges: bytes` and a
   length, use it as a provisional plan.
3. Otherwise send `GET` with `Range: bytes=0-0`.
4. If the response is `206`, parse `Content-Range: bytes a-b/total`.
5. If it is `200`, the server ignored Range: fall back to one sequential stream.
6. A `416` with `Content-Range: bytes */N` means the resource may be empty or the
   probe range is unsatisfiable; handle zero length and retry policy separately.
7. Reject non-identity `Content-Encoding`. A byte-exact ranged download cannot
   be built on a transformed representation.

The engine still treats the first real ranged response as authoritative. A
server that advertises ranges but returns `200` mid-plan causes a clean reset
and single-stream fallback.

## 3. Initial partition

Given `N`, configured `connections C`, maximum `M`, and minimum segment `S`:

```text
by_size = ceil(N / S)
count   = clamp(min(C, by_size, N), 1, M)
base    = floor(N / count)
extra   = N mod count
range_i = [offset, offset + base + (i < extra ? 1 : 0) - 1]
```

All ranges differ in size by at most one byte. A file smaller than `S` gets one
range and therefore one connection.

## 4. Work stealing

### 4.1 Victim selection

Among active segments with `remaining > 0`, choose the one with the largest
`remaining`. Ties choose the lowest segment id, making tests and journals
deterministic.

### 4.2 Split rule

Given victim progress `w` and victim end `e`:

```text
remaining = e + 1 - w
if remaining < 2 * S: do not split
split = w + floor(remaining / 2)
if split <= w or (e - split + 1) < S: do not split
victim.end = split - 1
new segment = [split, e]
```

The newcomer receives the second half. The victim keeps its already-written
prefix and the first half. Both halves are at least `S`.

### 4.3 Cooperative stop boundary

The coordinator sets an `AtomicU64 stop_at = split` for the victim before the
newcomer starts.

The victim checks the boundary before every write:

```text
boundary = stop_at == MAX ? end + 1 : max(stop_at, cursor)
allowed  = min(chunk.len, end + 1 - cursor, boundary - cursor)
if allowed == 0: stop as Stolen
write only chunk[..allowed]
```

This is deliberately **not** a socket abort. Aborting a response can leave an
in-flight chunk ambiguous; a cooperative boundary gives an exact, observable
write limit. The cost is that the victim may read and discard up to one network
chunk beyond the split. Correctness wins over a few KB of retransmission.

The newcomer may start immediately; it writes `[split, e]`, which is disjoint
from every byte the victim is permitted to write.

### 4.4 Connection growth

The coordinator samples aggregate bytes every `adaptive_interval_ms` (default
2 s). It grows by one only when:

- `adaptive_connections` is enabled,
- `active_count < max_connections`,
- no pending ranges exist,
- the sample throughput is greater than `1.02 ×` the previous sample
  (or this is the first sample),
- a legal split exists.

Because a new split is assigned to a *new* worker id, active connections can grow
from `C` toward `M`. Without growth, a finished worker can still steal and keep
the configured count busy.

There is currently no connection-count *decrease*. A transfer that saturates and
then degrades keeps its workers until completion. This is recorded as a gap in
the feature matrix (A03).

## 5. Coordinator algorithm

```text
ledger = fresh or recovered
spawn initial_assignments()
loop:
  if ledger.complete: break
  if active == 0: claim pending, or fail
  select:
    cancellation -> sync progress, checkpoint, abort workers -> Paused
    tick -> sync progress, publish UI snapshot
    worker done ->
      sync cursor
      if Completed/Stolen: ledger.finish
      if Failed: ledger.release, checkpoint, fail
      if complete: break
      dispatch idle:
         try steal largest active
         else claim pending
    checkpoint tick -> optionally fsync data, append journal snapshot
    adaptive tick -> maybe split largest active into a new worker
```

A worker only reports `Completed` when its cursor reaches `end + 1`, or `Stolen`
when it reaches its stop boundary. The coordinator always re-reads the atomic
cursor before `finish`, so a last-moment chunk cannot be lost from accounting.

## 6. Failure handling

- **Short read**: cursor is between `start` and `end`; retry the same range from
  the cursor.
- **Transient HTTP/network error**: retry with exponential backoff and jitter.
- **`200` after a ranged request**:
  - with an `If-Range` validator, classify as `RemoteChanged`;
  - without one, classify as `RangeUnsupported` and restart single-stream.
- **`416`**: range changed (file shrank or was replaced).
- **Disk error**: fatal; do not retry into a full disk.
- **Split during retry**: the stop boundary remains authoritative; a worker can
  be stopped between attempts.

## 7. Complexity and memory

- Ledger operations are `O(number of segments)` because segments are kept in a
  sorted `Vec` and lookups are linear. With ≤32 active segments and a small
  number of split remnants, this is negligible.
- Memory per transfer is bounded by the number of segments (bounded by splits,
  typically `O(connections + completions)`) plus one network chunk per worker.
- No file-sized buffers are allocated.
- Positional writes mean no merge pass and no second copy of the file.

## 8. Tests

- `partition_covers_every_byte`: proptest over total, connection count, minimum
  size.
- `split_ranges_are_contiguous`: proptest midpoint split.
- `random_scheduling_preserves_partition`: proptest random progress/finish/
  release/steal/claim operations, asserting a contiguous partition and valid
  cursors after every operation.
- `steals_work_from_a_slow_segment_and_still_writes_exact_bytes`: live origin
  with a delayed first range; asserts > initial range count and SHA-256 equality.
- `downloads_multipart_file_byte_exact`: four connections, 2 MiB.
- `cancels_and_resumes_from_journal`: cancellation and restart.

## 9. Deliberate deviations from IDM

| Area | IDM (public behaviour) | RustGet |
|---|---|---|
| Split trigger | connection becomes available | same, plus throughput-based growth |
| Stop mechanism | not publicly specified | cooperative atomic boundary |
| Initial count | user setting, often 8 | configurable 1–32, default 8 |
| Minimum segment | “too small for this connection type” | fixed configurable byte threshold |
| Resume boundaries | new split positions after resume | full range table persisted |
| Connection pool | vendor-controlled | `reqwest` pool; own pool planned |

These deviations are implementation choices, not compatibility claims.
