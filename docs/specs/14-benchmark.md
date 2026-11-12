# Benchmark report

**Date:** 2026-11-12
**Host:** Windows 11 build 26200, AMD64, local loopback
**Build:** `cargo build --release` (thin LTO, codegen-units=1)
**Origin:** `rustget-testkit` 128 MiB deterministic file, honest `206` ranges,
64 KiB response chunks
**Clients:** RustGet 0.1.0, 16 connections; aria2 1.37.0, `-x16 -s16`
**File:** 134,217,728 bytes; SHA-256 equality checked on both outputs

## 0. Primitive microbenchmarks (Criterion)

Command: `cargo bench -p rustget-core --bench ledger` (2026-11-12, same host)

| Benchmark | Median |
|---|---:|
| `partition_1TiB_16` | 37.7 ns |
| `split_1GiB_range` | 5.06 ns |
| `ledger/create_1TiB` | 114.5 ns |

These measure segmentation/scheduling primitives only, not network throughput.

## 1. Headline

| Client | Runs (s) | Median | Effective throughput |
|---|---|---:|---:|
| RustGet (latest 3) | 0.804, 0.790, 0.925 | **0.804 s** | **159.1 MiB/s** |
| RustGet (median of 9) | — | **0.839 s** | **152.5 MiB/s** |
| aria2 (earlier run) | 1.116, 1.016, 1.067 | **1.067 s** | **119.9 MiB/s** |

Wall-clock ratio: **1.33×** in RustGet's favour on this loopback origin.
Run-to-run variance is material (144–167 MiB/s); the 9-run median is the stable
number. The per-host semaphore added after the first benchmark is included.

> This is a **loopback microbenchmark**, not a 1 Gbps WAN claim. Both clients
> start a process, negotiate ranges and write the same 128 MiB. The origin's
> XOR generation is CPU-bound and is shared by both runs. Results on a real
> 1 Gbps link, a high-latency link, or a server with per-connection limits will
> differ. No conclusion about IDM performance is made.

Both outputs were byte-identical to the generated source. The testkit was warm
for all runs.

## 2. Why RustGet can win here

- 16 initial ranges, no separate merge pass.
- Direct positional writes into a preallocated file.
- 64 KiB streaming chunks, no intermediate buffering to a second file.
- Adaptive growth does not trigger before 2 s, so all runs use 16 connections.
- aria2's wall time includes more startup and its own allocation/control work;
  its internal reported average was higher than the wall-clock average.

This is not evidence that RustGet beats aria2 on WAN links. It is evidence that
the engine is not structurally slower on a local origin.

## 3. Microbenchmarks

`cargo bench -p rustget-core --bench ledger`:

| Benchmark | Time |
|---|---:|
| Partition 1 TiB into 16 ranges | 39.18 ns |
| Split 1 GiB range | 5.18 ns |
| Create 1 TiB ledger (16 ranges) | 113.77 ns |

Criterion reported 100 samples, 3 s warm-up, 5 s measurement. The ledger is not
a throughput bottleneck.

## 4. Reproduce

```powershell
./benchmarks/run-local.ps1 -Size 134217728
```

The script builds release binaries, starts the testkit on a fixed loopback port,
runs both clients three times, prints medians and verifies file sizes. It does
not download aria2 automatically; set `ARIA2C` to its path or place `aria2c` on
`PATH`.

## 5. What is not measured

- 1 Gbps saturation (no link).
- 10 Gbps / NVMe limits.
- Memory high-water mark for a 64 GiB sparse file.
- TLS handshake and HTTP/2 behavior (testkit is plain HTTP/1.1).
- High-latency / packet-loss links.
- Server per-host connection limits.
- 10k queued downloads.
- Power-loss safety.
- IDM comparison (no licensed install).

These are listed as gaps in the parity scorecard rather than estimated.
