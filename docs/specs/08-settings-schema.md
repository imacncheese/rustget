# Settings schema

**Status:** `EngineConfig` implemented; persisted user settings partially
implemented. The JSON below is the target stable schema.

## 1. Storage

- SQLite `settings(key, value)` for user-facing values.
- CLI flags override the stored value for one process.
- Environment overrides for headless deployments:
  - `RUSTGET_DATA_DIR`
  - `RUSTGET_DOWNLOAD_DIR`
  - `RUSTGET_API`
  - `RUSTGET_FFMPEG`
  - `RUST_LOG`
- No secrets in logs. The API token lives in a separate `api-token` file with
  owner-only permissions on Unix.

## 2. Download engine

```json
{
  "engine": {
    "connections": 8,
    "max_connections": 32,
    "max_connections_per_host": 16,
    "antivirus_scan_command": ["clamscan", "--no-summary"],
    "min_segment_size": 262144,
    "read_buffer_size": 65536,
    "max_retries": 6,
    "retry_base_ms": 250,
    "retry_max_ms": 30000,
    "max_speed_bytes_per_sec": null,
    "idle_timeout_secs": 30,
    "probe_timeout_secs": 30,
    "request_timeout_secs": 0,
    "adaptive_connections": true,
    "adaptive_interval_ms": 2000,
    "checkpoint_interval_ms": 250,
    "fsync_checkpoints": false,
    "validate_on_resume": true,
    "restart_on_change": false,
    "max_redirects": 10,
    "user_agent": "RustGet/0.1.0"
  }
}
```

Validation:

| Field | Rule |
|---|---|
| `connections` | 1..=32 and ≤ `max_connections` |
| `max_connections` | 1..=32 |
| `max_connections_per_host` | ≥ 1; process-wide semaphore |
| `min_segment_size` | > 0 |
| `read_buffer_size` | ≥ 1024 |
| `checkpoint_interval_ms` | > 0 |
| `adaptive_interval_ms` | ≥ 100 |

Defaults favor throughput. `fsync_checkpoints=true` is the “maximum safety”
profile and costs throughput.

## 3. Paths and categories

```json
{
  "paths": {
    "download_dir": "D:\\Downloads",
    "temp_dir": null,
    "category_routing": true
  },
  "categories": [
    {"name": "Music", "directory": "D:\\Downloads\\Music",
     "extensions": ["mp3", "flac", "m4a"]},
    {"name": "Video", "directory": "D:\\Downloads\\Video",
     "extensions": ["mp4", "mkv", "webm"]}
  ]
}
```

Rules:

- explicit per-download directory wins;
- then category directory;
- then `download_dir`.
- Empty category directory means “use download_dir”.
- Extension matching is case-insensitive, without the dot.

## 4. Network

```json
{
  "network": {
    "proxy": null,
    "proxy_mode": "none",
    "pac_url": null,
    "no_proxy": ["*.local"],
    "per_host_connections": {},
    "http3": false,
    "client_certificate": null
  }
}
```

`proxy_mode`: `none`, `system`, `pac`, `fixed`.
Only `fixed` is implemented today. SOCKS/PAC/system detection are planned.

## 5. Scheduler and power

```json
{
  "scheduler": {
    "enabled": true,
    "max_concurrent": 4,
    "allow_power_actions": false,
    "tick_seconds": 10
  }
}
```

Power actions are stored per schedule but are never executed while
`allow_power_actions` is false.

## 6. UI

```json
{
  "ui": {
    "theme": "system",
    "locale": "en-US",
    "notify_on_complete": true,
    "close_to_tray": true,
    "start_minimized": false,
    "confirm_delete": true,
    "columns": ["name", "size", "progress", "speed", "status"]
  }
}
```

Implemented today: theme, completion notifications, delete confirmation is a
native `confirm`.

## 7. Browser bridge

```json
{
  "browser": {
    "enabled": true,
    "native_host_name": "com.rustget.app",
    "intercept_downloads": false,
    "media_panel": true,
    "site_allowlist": []
  }
}
```

`intercept_downloads` is reserved; the current extension does not cancel browser
downloads.

## 8. Advanced

```json
{
  "advanced": {
    "api_listen": "127.0.0.1:8765",
    "api_token_file": "api-token",
    "log_level": "warn",
    "max_concurrent_downloads": 4,
    "checksum_default": null
  }
}
```

Binding a non-loopback address is refused. A future explicit
`allow_remote_api=true` must also require TLS and a separate origin policy.

## 9. Migration

- Settings are versioned by a `schema_version` key.
- Unknown keys are preserved, not dropped.
- Removed keys are ignored with a warning.
- Destructive migrations require a backup of `rustget.db`.

## 10. Example: maximum-safety profile

```json
{
  "engine": {
    "connections": 4,
    "fsync_checkpoints": true,
    "checkpoint_interval_ms": 500,
    "validate_on_resume": true
  }
}
```

## 11. Example: constrained network

```json
{
  "engine": {
    "connections": 4,
    "max_connections": 4,
    "min_segment_size": 1048576,
    "max_speed_bytes_per_sec": 2097152,
    "adaptive_connections": false
  }
}
```

## 12. Open schema decisions

- Whether to expose per-host caps in the first UI release.
- Whether `fsync_checkpoints` should default to true on laptops.
- Whether settings should be TOML for hand-editing; SQLite is canonical, export
  to TOML is planned.
