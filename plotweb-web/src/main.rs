pub mod api;
pub mod store;
pub mod router;
pub mod pages;
pub mod components;
pub mod fonts;
pub mod ws;
pub mod platform;
pub mod rinch_backend;

use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;

use rinch::prelude::*;
use rinch_core::element::ThemeProviderProps;

use store::{AppStore, Route};

// ── App component ───────────────────────────────────────────────────────────

#[component]
fn app() -> NodeHandle {
    let store = AppStore::new();
    rinch_core::create_store(store);

    // Parse the current URL to determine the initial route. On native there is no
    // browser location (and `web_sys::window()` panics off-wasm), so start at the
    // session check with a neutral route.
    #[cfg(target_arch = "wasm32")]
    let initial_route = web_sys::window()
        .and_then(|w| w.location().pathname().ok())
        .map(|p| Route::from_path(&p))
        .unwrap_or(Route::Dashboard);
    #[cfg(not(target_arch = "wasm32"))]
    let initial_route = Route::Dashboard;

    if matches!(
        initial_route,
        Route::Reader(_) | Route::ForgotPassword | Route::ResetPassword(_)
    ) {
        // Public, no-auth pages — set directly without the session check that
        // would otherwise redirect a logged-out visitor to Login.
        store.current_route.set(initial_route.clone());
        router::replace_state(&initial_route);
        store.loading.set(false);
    } else if matches!(initial_route, Route::ThemePreview | Route::EditorSpike | Route::OpfsSpike | Route::SyncSpike) {
        // Dev preview routes — public, no session check.
        store.current_route.set(initial_route.clone());
        router::replace_state(&initial_route);
        store.loading.set(false);
    } else {
        let requested = initial_route;
        // Check session on start
        api::get::<plotweb_common::User>("/api/auth/me", move |result| {
            match result {
                Ok(user) => {
                    store.current_user.set(Some(user));
                    let route = match &requested {
                        Route::Login | Route::Register => Route::Dashboard,
                        other => other.clone(),
                    };
                    router::replace_state(&route);
                    store.current_route.set(route);
                }
                Err(_) => {
                    let route = match &requested {
                        Route::Register => Route::Register,
                        _ => Route::Login,
                    };
                    router::replace_state(&route);
                    store.current_route.set(route);
                }
            }
            store.loading.set(false);
        });
    }

    // Listen for browser back/forward navigation (web only — no History natively).
    #[cfg(target_arch = "wasm32")]
    {
        let popstate_closure = Closure::wrap(Box::new(move |_event: web_sys::Event| {
            if let Some(window) = web_sys::window() {
                if let Ok(pathname) = window.location().pathname() {
                    let route = Route::from_path(&pathname);
                    store.current_route.set(route);
                }
            }
        }) as Box<dyn FnMut(_)>);
        web_sys::window()
            .unwrap()
            .add_event_listener_with_callback(
                "popstate",
                popstate_closure.as_ref().unchecked_ref(),
            )
            .unwrap();
        popstate_closure.forget();
    }

    rsx! {
        ThemeProvider {
            primary_color_fn: Rc::new(|| "teal"),
            default_radius: "xs",
            dark_mode_fn: Rc::new(move || store.dark_mode.get()),

            {components::app_shell::app_shell(__scope)}
        }
    }
}

/// The app theme, shared across the web (`rinch_web::mount`) and desktop
/// (`rinch::run_with_theme`) entry points.
fn theme_props() -> ThemeProviderProps {
    ThemeProviderProps {
        primary_color: Some("teal".into()),
        default_radius: Some("xs".into()),
        font_family: Some("'Playwrite DE Grund', Georgia, 'Times New Roman', serif".into()),
        dark_mode: true,
        ..Default::default()
    }
}

// ── Entry points ────────────────────────────────────────────────────────────

/// Web entry: mount into the browser DOM via rinch-web.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Info).ok();
    rinch_web::mount(theme_props(), app);
    log::info!("PlotWeb mounted");
}

/// Desktop entry: run a native window via rinch's shell (winit/wgpu).
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    rinch::run_with_theme("PlotWeb", 1200, 800, app, theme_props());
}

#[cfg(target_arch = "wasm32")]
fn main() {}
