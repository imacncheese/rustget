# Contributing to RustGet

Thank you for improving an open download manager. Please read the clean-room
policy before touching protocol or compatibility code.

## Ground rules

- Be respectful. See the project code of conduct once published.
- No copied IDM code, assets, strings or protocol dumps. RustGet is an
  independent implementation.
- No telemetry or analytics.
- Every feature needs a test and a documentation update.
- Keep business logic out of the UI and API layers.
- `#![forbid(unsafe_code)]` by default. A new unsafe block requires an ADR,
  a safety comment and a focused test.

## Development setup

```sh
rustup toolchain install stable
cargo build
cargo test --workspace
```

On Windows, the MSVC toolchain is recommended. A GNU toolchain also works but
needs a MinGW runtime that provides `libgcc_eh`; CI uses MSVC.

## Before opening a PR

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo deny check
```

If you changed a protocol, journal, API or UI flow, update the corresponding
document under `docs/specs/` and add an ADR if the change is architectural.

## Commit style

Conventional Commits:

```text
feat(engine): split largest active range on idle
fix(storage): ignore torn journal tail
docs(adr): record cooperative stop boundary
test(net): cover changed validator
```

Small commits are preferred. Do not mix formatting-only changes with behavior.

## Adding a dependency

1. Check maintenance status, license and transitive size.
2. Add it to `[workspace.dependencies]` with a caret version.
3. Update `deny.toml` if a new license appears.
4. Justify it in the PR and, for a heavy dependency, in ADR-0013.

## Test requirements

| Change | Required test |
|---|---|
| Segmenter/ledger | property test or randomized invariant |
| Journal format | round-trip + torn-tail + version fixture |
| HTTP behavior | testkit integration |
| API route | auth + happy path + error path |
| UI flow | headless screenshot or Playwright (planned) |
| Scheduler | clock-injected unit test |
| Browser bridge | native framing + live loopback test |

## Reporting bugs

Include:

- RustGet version (`rustget --version`);
- OS and filesystem;
- exact command or UI steps;
- server behavior (status, `Accept-Ranges`, `Content-Length`, redirects);
- redacted logs (`RUST_LOG=debug`);
- whether a `.part`/`.rustget` file remains.

Never attach cookies, tokens or private URLs unredacted.

## Security

Do not open a public issue for vulnerabilities. See [SECURITY.md](SECURITY.md).

## Documentation style

- State what is implemented, not what is planned, in the present tense.
- Mark gaps explicitly.
- Use “verified by” links to tests where possible.
- Keep line lengths reasonable; tables are encouraged for matrices.
