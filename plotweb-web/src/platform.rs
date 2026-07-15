//! Platform seam for browser-only APIs.
//!
//! `web_sys::window()` does **not** return `None` off-wasm — it panics ("cannot
//! access imported statics on non-wasm targets"). Since most call sites already
//! treat the window/document as optional (`if let Some(w)`, `.and_then(..)`),
//! routing them through these accessors makes the whole web-only branch collapse
//! into a graceful no-op on desktop instead of a crash.
//!
//! Use `platform::window()` / `platform::document()` everywhere instead of
//! `web_sys::window()`. Anything nested inside the resulting `Some(..)` branch
//! (timers, `Closure`s, DOM queries) is then simply never reached on native.
//!
//! This is a *safety* seam, not a feature port: web-only affordances (DOM
//! measurement, `set_inner_html`, document listeners) stay inert on desktop. The
//! follow-up is to rebuild those declaratively in `rsx!` (rinch Rule 0) so they
//! work on both targets.

/// The browser window, or `None` on native.
#[cfg(target_arch = "wasm32")]
pub fn window() -> Option<web_sys::Window> {
    web_sys::window()
}

/// Always `None` on native — there is no browser window.
#[cfg(not(target_arch = "wasm32"))]
pub fn window() -> Option<web_sys::Window> {
    None
}

/// The browser document, or `None` on native.
#[cfg(target_arch = "wasm32")]
pub fn document() -> Option<web_sys::Document> {
    web_sys::window().and_then(|w| w.document())
}

/// Always `None` on native — there is no browser document.
#[cfg(not(target_arch = "wasm32"))]
pub fn document() -> Option<web_sys::Document> {
    None
}
