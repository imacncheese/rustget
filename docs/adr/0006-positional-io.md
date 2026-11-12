# ADR-0006: Positional writes and sparse preallocation

- **Status:** accepted
- **Date:** 2026-11-12

## Context

Concurrent segment workers must write to one output file without a global lock
and without a final merge. Buffering whole files in memory is impossible for
multi-TB downloads. Sparse files can avoid allocating the full size up front on
most filesystems.

## Decision

- Open the `.part` file once and share it as `Arc<PartFile>`.
- Use `FileExt::write_at` (Unix) / `seek_write` (Windows) for absolute writes.
- Set the final length up front (`set_len`) in sparse mode.
- Optionally zero every block in “full” mode to surface `ENOSPC` early.
- Never use a `BufWriter` across threads. Each worker writes its own chunks;
  the OS page cache coalesces.
- A final `sync_all` precedes the rename.

## Consequences

- No merger pass, no lock contention, no whole-file memory.
- Out-of-order writes are natural and tested.
- Sparse behavior is filesystem-dependent; exFAT may allocate eagerly.
- Full mode is slow and currently not exposed in the CLI.
- `pwrite` is a syscall per chunk; at 100 MB/s with 64 KiB chunks this is
  ~1,600 syscalls/s, which is acceptable. A future `io_uring` backend can batch.
- Windows `seek_write` is thread-safe for the shared handle because it uses
  explicit offsets, not a shared file pointer.
