# Error taxonomy and recovery policy

**Status:** implemented in `rustget-core::retry` and `rustget-net::error`

## 1. Classes

| Class | Meaning | Retry? | User action |
|---|---|---|---|
| `Transient` | Network reset, DNS, timeout, 5xx, short read | Yes, exponential backoff | Wait; check network |
| `RateLimited` | 429, 503 backpressure | Yes, 4× base delay | Wait; lower connections |
| `Auth` | 401, 403, 407 | No | Supply/refresh credentials |
| `NotFound` | 404, 410 | No | Check URL |
| `RangeChanged` | 416, validator drift | Policy | Restart or ignore mtime |
| `RangeUnsupported` | Server ignores Range after probe | No | Automatic single-stream fallback |
| `Fatal` | Disk full, permission, invalid path, checksum, canceled | No | Fix local condition |

`NetError::class()` is the single mapping point. The engine must not invent
retry decisions in worker code.

## 2. HTTP mapping

| Status | Class | Notes |
|---|---|---|
| 200 to a Range request | `RangeUnsupported` or `RangeChanged` | `RangeChanged` if `If-Range` was sent |
| 206 | success | `Content-Range` start must equal cursor |
| 301/302/303/307/308 | handled by client | `reqwest` follows; method/header rules apply |
| 400 | fatal | malformed request/custom header |
| 401 | auth | no retry |
| 403 | auth | often expired signed URL; no infinite retry |
| 404/410 | not found | no retry |
| 408/425 | transient | retry |
| 416 | range changed | restart policy |
| 429 | rate limited | longer backoff |
| 500/502/504 | transient | retry |
| 503 | rate limited | retry |
| other 5xx | transient | retry |
| other 4xx | fatal | |

## 3. Backoff

```text
delay(attempt) = min(max_delay, base * 2^(attempt-1)) * jitter
jitter ∈ [1 - 0.25, 1 + 0.25]
rate_limited base = 4 * base
```

Defaults: `base = 250 ms`, `max = 30 s`, `max_retries = 6`. Attempt numbers start
at 1. A retry counter is per worker/range, not per whole file, so a long transfer
with many segments does not exhaust the budget globally.

## 4. Non-retryable local errors

- `StorageFull` / `PermissionDenied` / invalid path → `Fatal`.
- Checksum mismatch → `ChecksumMismatch`; the part file is retained for
  inspection/retry.
- Destination exists and overwrite was not requested → `DestinationExists`.
- Cancellation is not an error to retry; it becomes `Paused`.

## 5. Classification of reqwest errors

All `reqwest::Error` values currently classify as `Transient`. This is
conservative but can cause useless retries for malformed URLs or TLS certificate
errors. The next iteration should inspect:

- `is_builder()` → fatal;
- `is_redirect()` → fatal after policy exhaustion;
- certificate errors → fatal with a specific message;
- `is_timeout()`/`is_connect()` → transient.

## 6. User-facing messages

Internal errors are logged with context. The UI shows a short sentence plus the
technical message in the details error row:

| Internal | UI copy |
|---|---|
| `RemoteChanged` | “The file changed on the server. Retry to start again.” |
| `RangeUnsupported` | not shown; automatic fallback |
| `Auth` | “The server requires sign-in or rejected the credentials.” |
| `DestinationExists` | “A file with that name already exists.” |
| `ChecksumMismatch` | “Checksum mismatch. The partial file was kept.” |
| `StorageFull` | “Not enough disk space.” |
| `Cancelled` | “Paused.” |

## 7. Error invariants

1. A failed write must never be followed by a journal checkpoint that claims
   those bytes are durable.
2. A `RangeUnsupported` fallback must delete the old part/journal before starting
   a sequential transfer.
3. A checksum failure must not rename the part file.
4. A pause must checkpoint before returning.
5. A worker panic must fail the whole download, not silently leave a gap.

## 8. Known gaps

- No per-host retry budget or circuit breaker.
- No distinction between DNS NXDOMAIN (fatal-ish) and temporary DNS failure.
- No captive-portal detection.
- No retry-after header parsing; 429/503 use fixed exponential backoff.

## 9. Tests

- `status_classes`
- `backoff_is_capped_and_jittered`
- `auth_is_not_retried`
- `retries_mid_stream_resets`
- `detects_changed_validator`
- `falls_back_to_single_stream_when_ranges_are_ignored`
