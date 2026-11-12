# Site grabber / crawler specification

**Status:** link discovery implemented; project persistence, mirroring and UI
planned
**Code:** `crates/rustget-crawler`, CLI `rustget grab`

## 1. Boundary

The grabber discovers links. It is **not** a browser:

- no JavaScript execution;
- no form submission;
- no login automation;
- no mirror rewriting of HTML;
- no automatic download unless `--download` is explicit.

This keeps it predictable and avoids turning the tool into a general scraper.

## 2. Algorithm

```text
parse start URL (http/https only)
if respect_robots: fetch /robots.txt and parse User-agent: * Disallow rules
queue = [(start, depth=0)]
while queue and pages < max_pages:
  url, depth = pop
  if seen or robots-disallowed: continue
  GET url
  if content-type is not HTML: continue
  for href/src in body:
    resolve against url
    reject non-http(s)
    if same_origin and origin != start_origin: reject
    if extension matches filters and not robots-disallowed:
       record file
    else if depth < max_depth:
       enqueue page
```

## 3. Defaults

| Setting | Default | CLI |
|---|---|---|
| max depth | 2 | `--depth` |
| max pages | 100 | `--max-pages` |
| same origin | true | fixed |
| robots.txt | respected | fixed |
| extension filter | all links | `--ext zip,pdf` |
| download results | false | `--download` |

## 4. Robots rules

Only the `User-agent: *` and `User-agent: rustget` groups are considered.
`Disallow` values are prefix-matched against the path + query. An empty
`Disallow:` means allow all. `Allow` directives are intentionally ignored in
0.1.0 (documented limitation), because prefix precedence is easy to get wrong.

## 5. Output

```json
{
  "start_url": "https://example.test/",
  "pages_visited": 12,
  "files": [
    {"url": "https://example.test/a.pdf", "source_page": "https://example.test/", "extension": "pdf"}
  ],
  "errors": []
}
```

CLI prints one URL per line, then optionally adds each to the manager.

## 6. Security

- Same-origin by default prevents accidentally crawling the whole web.
- Max pages and depth bound the work.
- No credentials are sent unless the caller supplies headers (not exposed in the
  current `grab` command).
- Redirects are limited to 10.
- HTML is parsed with a conservative `href|src` regex; it is not a full HTML
  parser and can miss dynamically constructed URLs. That is a functional
  limitation, not a trust boundary.
- No extracted file is executed.

## 7. Tests

- `robots_rules_are_prefix_based`
- `extensions_and_origins`
- Manual local-origin smoke test: one page, one allowed file, one
  robots-disallowed file; only the allowed file was returned.

## 8. Gaps

- Saved projects, scheduled sync, incremental change detection.
- HTML base/href edge cases and `<base>` handling.
- Per-project headers/auth.
- UI.
- Concurrency (currently sequential).
- Mirroring/offline rewriting.
