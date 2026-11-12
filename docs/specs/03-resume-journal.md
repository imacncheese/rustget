# Resume journal and crash-safety specification

**Status:** implemented; process-kill proof passing, power-loss proof incomplete
**Code:** `crates/rustget-storage/src/journal.rs`,
`crates/rustget-storage/src/part.rs`,
`crates/rustget-net/src/engine.rs`
**Related:** [ADR-0005](../adr/0005-storage-and-journal.md),
[crash-safety evidence](11-crash-safety.md)

## 1. Files

For a destination `report.pdf`:

| File | Purpose |
|---|---|
| `report.pdf` | final visible output; only appears after complete verification |
| `report.pdf.part` | preallocated output, written at absolute offsets |
| `report.pdf.rustget` | append-only JSONL snapshots of the resume state |
| `report.pdf.rustget.tmp` | optional compaction temporary; recovery accepts a complete file |

A download is only renamed to its final name after:

1. all segment cursors equal their range ends + 1;
2. file data is synced (`sync_all` on finalize);
3. optional checksum verification passes;
4. a `complete: true` journal record is durable;
5. the destination does not exist (or overwrite was explicitly requested).

## 2. Journal format

One JSON object per line, UTF-8, newline terminated:

```json
{
  "version": 1,
  "saved_at_unix_ms": 1794500343979,
  "state": {
    "task_id": "67cfb9a5-f040-494b-a70b-02c494a0b903",
    "url": "https://example.test/report.pdf",
    "destination": "D:\\Downloads\\report.pdf",
    "part_path": "D:\\Downloads\\report.pdf.part",
    "total_size": 10485760,
    "etag": "\"abc123\"",
    "last_modified": "Wed, 21 Oct 2015 07:28:00 GMT",
    "range_supported": true,
    "segments": {
      "total_size": 10485760,
      "segments": [
        {"id": 0, "start": 0, "end": 2621439, "written": 1835008, "state": "active", "attempts": 1}
      ]
    },
    "complete": false
  }
}
```

Each record is a **full snapshot**, not a delta. This is intentionally
space-inefficient in exchange for simple, robust recovery. At 4 records/s and
roughly 400 bytes per record, a 10 GiB transfer writes on the order of a few
hundred MB over many hours; compaction every 512 records reduces this. A future
version can switch to a binary delta log behind the same version field.

### 2.1 State transitions

- `pending`: not assigned.
- `active`: assigned to a worker. On recovery, treated as `pending` at its
  recorded `written` offset.
- `complete`: `written` must equal `end + 1`.

Unknown fields are ignored by `serde` for forward compatibility. Unknown
`version` records are ignored by readers. The current writer only emits version
1.

## 3. Checkpoint protocol

The engine checkpoints on a timer (default 250 ms) and on pause/failure:

```text
sync worker atomic cursors into the ledger
snapshot = ledger.snapshot()
if snapshot == last_snapshot: skip
if fsync_checkpoints: part.sync_data()
journal.append(snapshot, sync = fsync_checkpoints)
```

Ordering matters:

- With `fsync_checkpoints = false` (default), progress is durable against
  **process death** because `pwrite` enters the kernel page cache. It is not
  guaranteed against sudden power loss.
- With `fsync_checkpoints = true`, the data file is synced before the journal is
  appended and synced. A recorded cursor therefore never points beyond durable
  file bytes.
- The journal is append-only. A torn final line is ignored.
- Compaction writes a complete `.tmp`, syncs it, removes the old journal, then
  renames. Recovery also accepts a complete `.tmp`, covering the crash window
  between remove and rename.

### 3.1 Why not write every chunk to the journal?

At 100 MB/s with 64 KiB chunks, per-chunk journaling would be 1,600 records/s.
The 250 ms interval is the deliberate tradeoff. The loss bound is:

```text
lost progress <= checkpoint_interval * throughput + one network chunk per worker
```

Even if the journal is stale, re-fetching bytes is safe: the part file has a
fixed length and positional writes overwrite the same offsets. A stale journal
never causes output corruption; it only causes redundant network work.

## 4. Recovery algorithm

```text
candidate = latest valid record from journal or .tmp
if none: fresh download
if candidate.total_size != probe.total_size: RemoteChanged
if candidate.etag or last_modified present and probe disagrees:
    if restart_on_change: reset part + journal, fresh
    else: fail with RemoteChanged
ledger = from_snapshot(candidate.segments)
convert active -> pending, retain written
open existing part (must have length >= total; extend sparse if short)
claim initial assignments from pending segments
resume checkpointing
```

`from_snapshot` validates:

- segment ids unique;
- ranges are contiguous, start at 0, end at `total - 1`;
- `written` within `[start, end + 1]`;
- no gaps or overlaps.

Invalid journals are rejected rather than silently trusted. A corrupt journal
therefore fails the resume with a typed error; the user can choose to restart.

## 5. Crash matrix

| Event | Data file | Journal | Recovery result |
|---|---|---|---|
| Process killed during write | bytes in page cache; fixed length | last complete line | resume from stale/valid cursor; overwrite safely |
| Process killed during journal append | unchanged | torn tail | previous line used |
| Process killed between journal and rename | part complete | `complete=true` | finalize part to destination |
| Process killed after rename before journal delete | destination exists | complete | treat as complete; delete/orphan journal |
| Power loss, fsync off | recent bytes may be lost | recent records may be lost | unsynced cursor could exceed durable data → **not guaranteed** |
| Power loss, fsync on | durable up to last sync | durable up to last sync | safe bound |
| Disk full | write returns error | last snapshot | fail with I/O error; keep journal for retry |
| File changed remotely | part old | old validator | probe mismatch → restart policy or fail |

The power-loss row is a known limitation. The default `fsync_checkpoints=false`
optimizes throughput for process crashes. A production build should expose
“maximum safety” and measure the throughput cost; this is tracked in the parity
scorecard.

## 6. Integrity and finalization

- A checksum, if configured, is streamed over the `.part` file before rename.
- A mismatch leaves the part and journal intact and returns
  `ChecksumMismatch`.
- Final rename is same-directory and same-filesystem, so it is atomic on NTFS,
  APFS and POSIX filesystems.
- Windows refuses rename onto an existing path; overwrite is an explicit remove
  before rename, not a silent clobber.

## 7. Cleanup

On success, the journal and `.rustget.tmp` are removed. On failure or pause, both
the part and journal are retained. `rustget remove --delete-files` deletes the
final file, part and journal. The UI always asks before deleting files.

## 8. Tests

- `latest_record_wins`
- `torn_tail_falls_back_to_previous_record`
- `compaction_keeps_latest_state`
- `segment_snapshot_is_preserved`
- `out_of_order_positional_writes_are_preserved`
- `write_beyond_length_is_rejected`
- `kill_9_then_resume_produces_byte_exact_file` (CLI child process killed;
  SHA-256 equality after resume)

## 9. Versioning

Journal format changes require:

1. a new `version` value,
2. a migration or an explicit “cannot resume, restart” error,
3. a fixture under `crates/rustget-storage/tests/fixtures/`,
4. a note in the ADR.

There is no silent downgrade. A newer RustGet may read older versions, but an
older RustGet must not guess at a newer layout.
