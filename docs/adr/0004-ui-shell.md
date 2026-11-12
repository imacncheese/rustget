# ADR-0004: UI shell — local web app first, Tauri deferred

- **Status:** accepted, revisit at M4.1
- **Date:** 2026-11-12

## Context

The brief recommends Tauri 2 + TypeScript (SolidJS/Svelte) + Tailwind, with Slint
or iced as pure-Rust alternatives. The project must ship an intuitive UI while
keeping all business logic in Rust and decoupling UI from engine.

The build environment had no Rust toolchain at start and no Node package manager;
a portable toolchain was installed. A Tauri build adds WebView2/Windows SDK
requirements, a JS bundler, and a second distribution pipeline. It does not
change the engine/UI contract.

## Decision

For 0.1.0, ship a **dependency-free web application** served by the local daemon:

- HTML/CSS/ES modules in `crates/rustget-app/assets`;
- same authenticated `/api/v1` REST + WebSocket as the CLI;
- no business logic in JavaScript;
- virtualized table, detail pane, add dialog, settings.

The UI is deliberately a thin client. A future Tauri 2 shell will load the same
assets and API, adding tray/window/native notifications. The shell must not gain
engine logic.

## Why not Slint/iced now

- Both would require a separate rendering model and a second implementation of
  the already-working UI.
- The web UI is testable with headless Chrome and accessible to remote/headless
  users.
- Tauri can reuse the exact assets; a pure-Rust UI cannot.

## Consequences

- No tray icon, native menus, or OS notifications yet.
- The UI requires a running daemon and a browser (or a future WebView).
- Token onboarding is a settings dialog rather than a first-run wizard.
- The web UI is cross-platform by construction and screenshots are reproducible.
- A future ADR must record the Tauri decision, bundle size and security review
  before replacing this one.
