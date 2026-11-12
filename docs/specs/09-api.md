# Local API specification

**Status:** implemented
**Base URL:** `http://127.0.0.1:8765`
**OpenAPI:** `GET /api/v1/openapi.json`

## 1. Authentication

Every `/api/v1/*` route except `/api/v1/health` requires:

```http
Authorization: Bearer <token>
```

The token is created by `rustget serve` in `<data-dir>/api-token`. It is 64 hex
characters and is written with owner-only permissions on Unix.

WebSocket authentication uses `?token=<token>` because browsers cannot set an
Authorization header on a WebSocket handshake. The token is compared in constant
time over its full length.

Mutations also validate `Origin` when present:

- accepted: same host as `Host` (`http://` or `https://`);
- accepted: explicit `allowed_origins` from configuration;
- rejected: any other origin (`403 forbidden_origin`);
- CLI requests with no Origin are allowed.

The listener refuses non-loopback addresses unless a future explicit setting is
added.

## 2. Content types and limits

- JSON request/response (`application/json`), except `204 No Content`.
- Request body limit: 1 MiB.
- No CORS headers are emitted.
- No cookies are used for API auth.
- Errors:

```json
{"error": "human readable", "code": "unauthorized"}
```

## 3. Endpoints

### `GET /api/v1/health`

Public liveness probe.

```json
{"status":"ok","version":"0.1.0"}
```

### `GET /api/v1/downloads?status=downloading`

```json
{
  "downloads": [
    {
      "id": "afd1f60c-...",
      "url": "https://example.test/file.bin",
      "final_url": "https://cdn.example.test/file.bin",
      "destination": "D:\\Downloads\\file.bin",
      "filename": "file.bin",
      "category": "General",
      "status": "downloading",
      "total_size": 1048576,
      "downloaded": 524288,
      "speed_bps": 943718.4,
      "connections": 8,
      "speed_limit": null,
      "headers": [],
      "checksum_algorithm": null,
      "checksum_value": null,
      "etag": "\"abc\"",
      "last_modified": "Wed, 21 Oct 2015 07:28:00 GMT",
      "error": null,
      "priority": 0,
      "created_at_ms": 1794500343979,
      "updated_at_ms": 1794500345000,
      "completed_at_ms": null
    }
  ],
  "count": 1
}
```

`status` accepts `queued`, `probing`, `downloading`, `finalizing`, `paused`,
`completed`, `failed`.

### `POST /api/v1/downloads`

Request:

```json
{
  "url": "https://example.test/file.zip",
  "mirrors": ["https://mirror.example/file.zip"],
  "destination": null,
  "directory": "D:\\Downloads",
  "category": "Archives",
  "connections": 8,
  "speed_limit": null,
  "headers": [["Referer", "https://example.test/"]],
  "checksum_algorithm": "sha256",
  "checksum_value": "…",
  "priority": 0,
  "start": true
}
```

- HTTP(S) is probed before insertion, so `filename`, `destination`, size and
  validators are resolved. Plain `ftp:` is inserted without an HTTP probe; FTPS
  is rejected.
- `mirrors` is an optional array of alternate HTTP(S) URLs.
- `201 Created` with the record.
- `400` for an invalid URL or state.
- `500` for an unexpected probe/DB failure (the HTTP status mapping will be
  refined).

### `GET /api/v1/downloads/{id}`

```json
{"download": { ... }, "progress": { ... } | null}
```

### `PATCH /api/v1/downloads/{id}`

Updates queue priority. Higher values are promoted first.

```json
{"priority": 7}
```

Returns the updated record.

### `POST /api/v1/downloads/pause-all`

Pauses every active/queued row. Returns `{"paused": N}`.

### `POST /api/v1/downloads/resume-all`

Resumes every paused/failed row. Returns `{"resumed": N}`.

### `POST /api/v1/downloads/{id}/pause`

Cancels the task; the journal/part are retained. Returns the updated record with
`status: "paused"`.

### `POST /api/v1/downloads/{id}/resume`

Queues a paused/failed record. Returns the record; the manager promotes it when a
slot is free.

### `POST /api/v1/downloads/{id}/retry`

Only for `failed`. Same body as resume.

### `DELETE /api/v1/downloads/{id}?delete_files=true`

Removes the row. With `delete_files=true`, deletes the final file, `.part` and
journal. Returns `204`.

### `POST /api/v1/downloads/{id}/open`

Opens the containing folder using the OS default handler. Returns `204` or
`500`.

### `GET /api/v1/export`

Returns a portable bundle:

```json
{"version":1,"downloads":[{"url":"...","mirrors":[],"destination":"...","start":false}]}
```

### `POST /api/v1/import`

```json
{"downloads":[{"url":"https://example.test/a.zip","start":false}]}
```

Each entry is probed and queued; `start` is forced to false so imports do not
unexpectedly saturate the network. Returns `{"imported":N,"failed":[...]}`.

### `GET /api/v1/stats`

```json
{"total":12,"active":2,"completed":9,"failed":1,"total_speed_bps":18874368.0}
```

### `GET /api/v1/categories`

Returns the category table:

```json
{"categories":[{"name":"Music","directory":"","extensions":["mp3","flac"]}]}
```

### Schedules

`GET /api/v1/schedules`

```json
{"schedules":[{"id":"...","name":"night","enabled":true,"days":31,
  "start_min":60,"stop_min":360,"action":"none"}]}
```

`POST /api/v1/schedules`

```json
{"name":"night","start":"01:00","stop":"06:00","days":"mon,tue,wed",
 "action":"none"}
```

`DELETE /api/v1/schedules/{id}` → `204`.

### `GET /api/v1/ws?token=...`

WebSocket. On connect:

```json
{"type":"hello","downloads":[...]}
```

Then one JSON object per manager event:

```json
{"type":"updated","id":"...","status":"downloading", "...": "..."}
{"type":"progress","id":"...","progress":{"downloaded":123,"segments":[...]}}
{"type":"removed","id":"..."}
{"type":"finished","id":"...","status":"completed"}
```

A `progress` event is emitted at most about every 250 ms per active download.
Progress is not persisted; a reconnecting client should fetch downloads and
resume from new events.

## 4. Static UI

| Path | Content |
|---|---|
| `/` `/index.html` | SPA |
| `/app.js` | client script |
| `/style.css` | styles |
| `/favicon.svg` | icon |
| any other path | SPA fallback |

Static assets are public but contain no token. The SPA reads the token from the
URL fragment (`#token=...`) or `localStorage` and sends it as a Bearer header.

## 5. Error codes

| HTTP | Code | Meaning |
|---|---|---|
| 400 | `bad_request` | invalid id/body/URL |
| 401 | `unauthorized` | missing/bad token |
| 403 | `forbidden_origin` | cross-origin mutation or non-loopback peer |
| 404 | `not_found` | unknown download |
| 500 | `internal` | DB/network/OS failure |

## 6. Examples

```sh
TOKEN=$(cat ~/.local/share/RustGet/api-token)
curl -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8765/api/v1/downloads
curl -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"url":"https://example.com/a.zip","start":true}' \
  http://127.0.0.1:8765/api/v1/downloads
```

## 7. Compatibility

`/api/v1` is the stable prefix. Additive fields are allowed. Breaking changes
require `/api/v2` or a documented migration window. The OpenAPI document is
generated by hand today and must be updated with every route change; an automated
schema check is planned.
