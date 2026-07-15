//! Platform seam for browser-only APIs.
//!
//! `web_sys::window()` does **not** return `None` off-wasm — it panics ("cannot
//! access imported statics on non-wasm targets"). Since most call sites already
//! treat the window/document as optional (`if let Some(w)`, `.and_then(..)`),
//! routing them through these accessors makes the whole web-only branch collapse
//! into a graceful no-op on desktop instead of a crash.
//!
//! Use `platform::window()` / `platform::document()` everywhere instead of
//! `web_sys::window()`. Anything nested *inside* the resulting `Some(..)` branch
//! (timers, DOM queries) is then simply never reached on native.
//!
//! # `Closure` is the exception — a runtime guard cannot save you
//!
//! Constructing a [`wasm_bindgen::closure::Closure`] is **itself** a wasm-bindgen
//! call that panics off-wasm ("function not implemented on non-wasm32 targets"),
//! and it is a `#[track_caller]` abort — not an unwind, so nothing catches it.
//! A `window()`/`document()` check placed *after* the construction is dead weight:
//!
//! ```ignore
//! let cb = Closure::wrap(..);                  // ← aborts on native, right here
//! if let Some(doc) = platform::document() {    // ← never gets a chance to help
//!     doc.add_event_listener_with_callback("keydown", ..).ok();
//! }
//! ```
//!
//! So a `Closure` must be *compiled out*, not guarded. Wrap the whole block in
//! [`web_only!`], which is a no-op statement off-wasm. Only put a `Closure`
//! outside `web_only!` when it is already lexically inside a
//! `platform::window()`/`document()` `Some(..)` branch (that branch is genuinely
//! unreachable on native) — when in doubt, use `web_only!`.
//!
//! This is a *safety* seam, not a feature port: web-only affordances (DOM
//! measurement, `set_inner_html`, document listeners) stay inert on desktop. The
//! follow-up is to rebuild those declaratively in `rsx!` (rinch Rule 0) so they
//! work on both targets.

/// Runs a block **only on wasm32**; expands to nothing at all on native.
///
/// Unlike a `platform::window()` / `platform::document()` check — which is a
/// *runtime* guard, and therefore useless against code that panics as it is
/// *constructed* — this removes the block from the native build entirely. That
/// makes it the only safe home for `Closure::wrap` / `Closure::once` /
/// `Closure::new`, plus anything else that only exists in a browser
/// (`setTimeout`, `requestAnimationFrame`, document listeners, `<input
/// type=file>`).
///
/// The block is a statement and must evaluate to `()`, since on native there is
/// nothing left to evaluate. If the surrounding code needs a value out of it,
/// compute a sane native default *outside* the macro first — do not let the
/// native path fall through with a bogus one.
///
/// ```ignore
/// web_only! {
///     let cb = Closure::wrap(Box::new(move |e: web_sys::Event| { .. }) as Box<dyn FnMut(_)>);
///     if let Some(doc) = crate::platform::document() {
///         doc.add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref()).ok();
///     }
///     cb.forget();
/// }
/// ```
#[macro_export]
macro_rules! web_only {
    ($($body:tt)*) => {
        #[cfg(target_arch = "wasm32")]
        {
            $($body)*
        }
    };
}

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
