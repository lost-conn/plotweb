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

// ── Clock ────────────────────────────────────────────────────────────────────

/// Current time as UTC seconds since the epoch. `updated_at` timestamps are
/// always `chrono::Utc::now()`-formatted server-side, so relative-time display
/// (dashboard "edited N minutes ago") compares against this rather than a local
/// wall clock that might be in a different timezone.
///
/// wasm has no `SystemTime::now()` (it panics — no clock syscall in the
/// sandbox), so this reaches for `js_sys::Date::now()` there instead.
#[cfg(target_arch = "wasm32")]
pub fn now_epoch_secs() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}

/// Native: the real wall clock via `SystemTime`.
#[cfg(not(target_arch = "wasm32"))]
pub fn now_epoch_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ── Bundled assets ───────────────────────────────────────────────────────────

/// Resolve a `/assets/...` reference into something the active image loader can read.
///
/// On the web these are URLs, served out of `dist/assets/` (trunk's `copy-dir`).
/// On desktop there is no origin to resolve them against, and rinch's default
/// `FileImageLoader` reads `src` straight off the **filesystem**, relative to the
/// process working directory — so a bare `/assets/logo.png` is looked up at the
/// filesystem root and fails (`No such file or directory`). Every native launch
/// logged that warning and rendered the login/dashboard logo as a broken image.
///
/// Native resolution order:
/// 1. `$XDG_DATA_HOME/plotweb/assets/<name>` (what `scripts/install-desktop.sh`
///    populates) — an installed copy, so the app does not depend on the checkout.
/// 2. The source tree the binary was built from, for `cargo run` during development.
///
/// Deliberately a **local file** rather than `{server}/assets/...`: the logo has to
/// render on a device that started with no network, which an HTTP fetch would not.
pub fn asset_src(path: &str) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        path.to_string()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let name = path.trim_start_matches('/').trim_start_matches("assets/");

        if let Some(dir) = native_asset_dir() {
            let installed = dir.join(name);
            if installed.is_file() {
                return installed.to_string_lossy().into_owned();
            }
        }
        // Fall back to the checkout this binary was built from. Absent on any other
        // machine, where the image simply fails to load exactly as it does today.
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(name)
            .to_string_lossy()
            .into_owned()
    }
}

/// The per-user directory holding installed assets, mirroring `local_store`'s
/// document dir (`$XDG_DATA_HOME/plotweb/...`, else `$HOME/.local/share/plotweb/...`).
#[cfg(not(target_arch = "wasm32"))]
fn native_asset_dir() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("plotweb").join("assets"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return Some(PathBuf::from(home).join(".local/share/plotweb/assets"));
        }
    }
    None
}

// ── Clipboard ────────────────────────────────────────────────────────────────

/// Put `text` on the system clipboard. Returns whether a copy was *attempted*
/// (the browser's `navigator.clipboard.writeText` is asynchronous and may still
/// be refused, e.g. on an insecure origin), so a caller can fall back to asking
/// the user to copy by hand when this is `false`.
#[cfg(target_arch = "wasm32")]
pub fn copy_text(text: &str) -> bool {
    use wasm_bindgen::JsCast;
    let Some(window) = window() else {
        return false;
    };
    match js_sys::Reflect::get(&window.navigator(), &"clipboard".into()) {
        Ok(clipboard) if !clipboard.is_undefined() && !clipboard.is_null() => {
            let clipboard: web_sys::Clipboard = clipboard.unchecked_into();
            let _ = clipboard.write_text(text);
            true
        }
        _ => false,
    }
}

/// Native: no clipboard wired up yet (rinch's `clipboard` feature is off for the
/// desktop build), so report that nothing was copied.
#[cfg(not(target_arch = "wasm32"))]
pub fn copy_text(_text: &str) -> bool {
    false
}
