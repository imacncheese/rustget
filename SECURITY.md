# Security policy

## Supported versions

| Version | Supported |
|---|---|
| 0.1.x | yes, current development |
| < 0.1 | no |

RustGet is pre-1.0; security fixes may require a minor version bump.

## Reporting a vulnerability

Please **do not** open a public issue.

Use a private GitHub Security Advisory once the repository is hosted. Until then,
contact the maintainers through the private channel listed in the repository
profile. Include:

- affected version and platform;
- a minimal reproduction (URL pattern, server response, API request);
- impact (file write, token disclosure, code execution, SSRF, etc.);
- whether the issue is already public.

We aim to acknowledge within 72 hours and to provide a remediation plan within
7 days for high-severity issues. There is no bug bounty.

## Scope

In scope:

- the local API (auth bypass, origin bypass, token disclosure, arbitrary file
  read/write);
- path traversal via filenames, redirects or manifests;
- native-messaging framing/memory issues;
- journal/part-file corruption that can write outside the destination;
- browser-extension token leakage or unexpected data collection;
- dependency vulnerabilities with a reachable path.

Out of scope:

- denial of service by downloading a very large file (the product's purpose);
- social engineering;
- vulnerabilities in IDM or third-party sites;
- missing hardening features that are already documented as gaps;
- issues requiring a hostile local administrator.

## Safe harbour

Good-faith research on your own machine, without accessing other users' data or
disrupting services, is welcome. Do not test against third-party sites without
permission.

## Security design

See [docs/specs/10-security.md](docs/specs/10-security.md) and
[ADR-0009](docs/adr/0009-local-api-security.md).

## Known limitations

- API token is stored in a file readable by the same OS user.
- Cookies may be persisted in SQLite for downloads that require them.
- No signed releases yet.
- No independent audit.
- The local API does not yet enforce a Host-header allowlist (DNS rebinding
  hardening).
