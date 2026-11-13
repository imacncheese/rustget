# Accessibility audit

**Date:** 2026-11-13
**Tool:** axe-core 4.10.2 through Chrome DevTools Protocol
**Page:** authenticated RustGet UI (`/` with a local token)
**Command:** `node scripts/accessibility-audit.mjs <cdp-port> <url> <axe.min.js>`

## Result

`[]` — zero axe violations after two fixes:

- Removed an invalid `role="row"` from the non-table list header, which also
  triggered required-parent/required-children failures.
- Added a visually hidden level-one heading (`RustGet downloads`) for page
  structure.

The UI already had labelled controls, semantic navigation, keyboard row
selection, `<dialog>` focus behaviour and `aria-live` toasts. A manual
screen-reader pass is still recommended; automated axe coverage is not a
substitute for it.
