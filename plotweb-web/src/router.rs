//! App-level routing. `store.current_route` (a signal) is the single source of
//! truth on every platform; [`navigate`] sets it. On web we additionally mirror
//! the route into the browser History (URL bar + back/forward); those calls are
//! `#[cfg(target_arch = "wasm32")]` and become no-ops on native.

use crate::store::{AppStore, Route};
use rinch_core::use_store;

/// Push the route's path onto the browser History (web URL sync). No-op on native.
#[cfg(target_arch = "wasm32")]
pub fn push_state(route: &Route) {
    if let Some(window) = web_sys::window() {
        if let Ok(history) = window.history() {
            let path = route.to_path();
            let _ = history.push_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&path));
        }
    }
}

/// No-op on native (no browser History).
#[cfg(not(target_arch = "wasm32"))]
pub fn push_state(_route: &Route) {}

/// Replace the current browser History entry with the route's path. No-op on native.
#[cfg(target_arch = "wasm32")]
pub fn replace_state(route: &Route) {
    if let Some(window) = web_sys::window() {
        if let Ok(history) = window.history() {
            let path = route.to_path();
            let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&path));
        }
    }
}

/// No-op on native (no browser History).
#[cfg(not(target_arch = "wasm32"))]
pub fn replace_state(_route: &Route) {}

/// Navigate to `route`: mirror it into browser History (web) and set the route
/// signal (all platforms — this is what actually re-renders the app).
pub fn navigate(route: Route) {
    let store = use_store::<AppStore>();
    push_state(&route);
    store.current_route.set(route);
}
