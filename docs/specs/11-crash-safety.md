# Crash-safety evidence

**Status:** process-kill proof passing
**Test:** `crates/rustget-cli/tests/crash_safety.rs`

## 1. Claim

If the RustGet process is terminated without warning at an arbitrary moment
during a ranged transfer, then:

1. the final destination has not been renamed into place;
2. the `.part` file is not structurally corrupted (positional writes, fixed
   length);
3. the journal contains a valid previous snapshot or a torn tail that is ignored;
4. re-running the same download resumes and produces a SHA-256-identical file.

The bound on lost progress is approximately one checkpoint interval plus one
network chunk per active worker, when fsync is enabled. With the default
`fsync_checkpoints=false` the guarantee covers process death, not power loss.

## 2. Test procedure

1. Start the instrumented origin with an 8 MiB deterministic file throttled to
   32 MiB/s.
2. Spawn the real `rustget` binary with four connections and a 1 MiB/s limit.
3. Poll `crash.bin.rustget` until the sum of segment progress is ≥ 1 MiB.
4. `child.kill()` (TerminateProcess; no destructor, no graceful checkpoint).
5. Read the journal to confirm at least 1 MiB was durable.
6. Re-run `rustget add` with the same output path.
7. Read the final file and compare SHA-256 with `expected_content`.
8. Assert `.part` and `.rustget` are gone.

## 3. Observed result (2026-11-12)

```text
running 1 test
test kill_9_then_resume_produces_byte_exact_file ... ok

test result: ok. 1 passed; 0 failed
```

The resumed run reported `complete` and the SHA-256 equality assertion passed.

## 4. Why the output cannot be corrupted

- The part file has a fixed length `N`; every write is range-checked against it.
- Segments are disjoint by construction (property-tested).
- A worker's cursor never moves backwards.
- On resume, `written` is clamped to `[start, end+1]`; a stale cursor causes
  re-fetching, not skipping.
- The final rename happens only after all ranges report complete and the file is
  synced.
- A checksum, when configured, runs before rename.

## 5. Why a stale journal is safe

Journal records are full snapshots. Recovery picks the newest parseable record.
If the process died after writing file bytes but before the next journal record,
the journal is behind. The resume starts at the older cursor and overwrites bytes
that are already correct. Disk bytes are never treated as authoritative without a
matching journal record.

## 6. Power-loss caveat

Default `fsync_checkpoints=false` means:

- the OS page cache survives process death;
- it may not survive an abrupt power cut;
- a journal record could survive while its data does not (if the filesystem
  reorders writes).

The safe mode (`fsync_checkpoints=true`) orders `sync_data` before journal
append+sync. It is implemented but not power-cut tested on real hardware. Stating
“power-loss proof” without that test would be dishonest.

## 7. Remaining proof work

1. Power-cut test with a controlled VM/disk (QEMU with cache=none or a hardware
   test rig), repeated 100 times.
2. Journal torn-write injection at every byte offset around a record boundary.
3. Filesystem matrix: NTFS, ext4, APFS, exFAT (no sparse support), network shares.
4. Disk-full injection during a write and during rename.
5. Rename failure (destination locked by another process).
6. Worker panic injection; ensure the coordinator checkpoints and fails.
7. Windows `TerminateProcess` vs POSIX `SIGKILL` behavior comparison.

## 8. Reproduce

```sh
cargo test -p rustget-cli --test crash_safety -- --nocapture
```

The test writes only under the system temp directory and a loopback origin.
