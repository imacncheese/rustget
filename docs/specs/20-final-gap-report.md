# Final gap report — RustGet 0.1.0

**Date:** 2026-11-13
**Matrix:** 147 rows, 1 skipped (BitTorrent, by user request), active 146.
**Score:** **113 implemented / 27 partial / 1 specified / 5 not started** — **140/146 = 95.9%**.

## Verified evidence

- Windows: **136 tests** pass; `cargo fmt --check` and
  `cargo clippy --workspace --all-targets -- -D warnings` clean.
- Linux: full workspace test suite passes under Ubuntu 26.04 amd64.
- Packaged `.deb` binary verified as `rustget 0.1.0`.
- AppImage verified as a valid x86-64 ELF AppImage.
- RPM built; metadata and file list verified.
- MSI payload verified with WiX `dark.exe`; Start Menu shortcut targets
  `rustget-desktop.exe`.
- Desktop smoke tests: native window, tray, notifications, clipboard monitoring.
- Live GitHub auto-update check.
- axe-core audit: zero violations.
- Public release: <https://github.com/imacncheese/rustget/releases/tag/v0.1.0>.

## Not started (5)

| ID | Gap | Why it remains |
|---|---|---|
| A21 | HTTP/3 / QUIC | No QUIC stack or h3 test origin wired; HTTP/2 is verified |
| A24 | SFTP | No SFTP client/server fixture; FTP/FTPS partial separately |
| E09 | Reproducible/signed installers | No trusted code-signing certificate or reproducible-build comparison |
| G11 | NTLM / Negotiate / Kerberos | Digest works; no NTLM/Negotiate/Kerberos exchange |
| G12 | HTTP/3 | Same as A21 |

## Partial (27)

- **A05** resume across crash/power loss/IP change: crash resume tested; power-loss/IP-change not.
- **A18** per-host caps: shared caps implemented; learned server limits absent.
- **A23** FTP/FTPS: plain FTP + resume verified; FTPS not.
- **A25/A26** HLS/DASH: VOD, AES-128 and parallel segments verified; audio groups, live and key rotation gaps remain.
- **A31** auth: Digest MD5/SHA-256 verified; NTLM/Negotiate/Kerberos absent.
- **A35/A36** proxies: HTTP/SOCKS5 and per-download proxy available; PAC and full system-proxy discovery absent.
- **B01–B05** browser: MV3/Firefox sources and native bridge exist; automated browser-host tests and full sniffing absent.
- **B14/B15/G08** site/video grabber: crawler and FFmpeg mux exist; integrated grabber workflow absent.
- **B22** custom schemes: parser and Windows/Linux registration helpers; live registration unverified in this sandbox.
- **C07** queue UI: filters/priority/move; no drag reorder or per-queue lanes.
- **C19** autostart: preview/registry helpers; live registration unverified.
- **C23** i18n: English/German JSON catalog; not Fluent and not exhaustive.
- **E08** lint/audit: fmt + strict clippy clean; `cargo-deny`/`cargo-audit` not run.
- **E13** post-action sandbox: allowlisted process launch; no OS sandbox.
- **F03** macOS: build/DMG script only; no macOS host to compile or verify.
- **F06** DMG: `.app` + `hdiutil` script unverified.
- **F07** file association: registration helpers; live association unverified.
- **G04** HLS/DASH segment scheduler: parallel segment fetch; no shared segment scheduler.
- **G10** PAC/system proxy: HTTP/SOCKS5 proxies; PAC absent.

## Recommended next work

1. Run the macOS build and DMG script on a macOS host, then close F03/F06.
2. Add NTLM via a maintained client library and a validating test origin (A31/G11).
3. Add an SFTP backend and local SFTP fixture (A24).
4. Add HTTP/3 through `quinn`/`h3` plus a QUIC test origin (A21/G12).
5. Obtain a trusted signing certificate, sign the MSI/DMG, and add reproducible-build checks (E09).
