//! Desktop shell assets and (future) native window integration.
//!
//! The shipping UI is a local web application served by `rustget-ipc`; it is a
//! thin client over the same authenticated API the CLI uses. The Tauri 2 shell
//! is intentionally isolated here so no business logic can leak into the UI.
#![forbid(unsafe_code)]

pub const INDEX_HTML: &str = include_str!("../assets/index.html");
pub const APP_JS: &str = include_str!("../assets/app.js");
pub const I18N_JS: &str = include_str!("../assets/i18n.js");
pub const STYLE_CSS: &str = include_str!("../assets/style.css");
pub const FAVICON_SVG: &str = include_str!("../assets/favicon.svg");

#[must_use]
pub fn asset(path: &str) -> Option<(&'static str, &'static str)> {
    match path {
        "/" | "/index.html" => Some(("text/html; charset=utf-8", INDEX_HTML)),
        "/app.js" => Some(("text/javascript; charset=utf-8", APP_JS)),
        "/i18n.js" => Some(("text/javascript; charset=utf-8", I18N_JS)),
        "/style.css" => Some(("text/css; charset=utf-8", STYLE_CSS)),
        "/favicon.svg" => Some(("image/svg+xml", FAVICON_SVG)),
        _ => None,
    }
}
