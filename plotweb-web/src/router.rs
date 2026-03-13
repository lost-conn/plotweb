use wasm_bindgen::JsValue;

use crate::store::{AppStore, Route};
use rinch_core::use_store;

pub fn push_state(route: &Route) {
    if let Some(window) = web_sys::window() {
        if let Ok(history) = window.history() {
            let path = route.to_path();
            let _ = history.push_state_with_url(&JsValue::NULL, "", Some(&path));
        }
    }
}

pub fn replace_state(route: &Route) {
    if let Some(window) = web_sys::window() {
        if let Ok(history) = window.history() {
            let path = route.to_path();
            let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&path));
        }
    }
}

pub fn navigate(route: Route) {
    let store = use_store::<AppStore>();
    push_state(&route);
    store.current_route.set(route);
}
