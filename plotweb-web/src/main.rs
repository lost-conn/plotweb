// The book page's modal/pane render functions each take several `impl Fn`
// closures through nested rsx! expansions; the default recursion limit trips
// on the resulting trait-resolution depth (see plotweb-web/src/pages/book/).
#![recursion_limit = "512"]

pub mod api;
pub mod find;
pub mod local_book;
pub mod local_dictionary;
pub mod local_store;
pub mod local_user;
pub mod spell;
pub mod store;
pub mod sync;
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

    // Did this device keep work the server replaced? Asked once at startup, before
    // anything else: a rescue is only useful if the author is told it exists, and until
    // now nothing in the UI ever was.
    local_store::spawn(async move {
        match local_store::rescued_copies().await {
            Ok(found) if !found.is_empty() => {
                log::warn!("local-first: {} rescued copy/copies on this device", found.len());
                store.rescued.set(found);
            }
            Ok(_) => {}
            Err(e) => log::warn!("local-first: could not list rescued copies: {e}"),
        }
    });

    // The account's custom spellcheck words, brought up the moment there *is* an
    // account — on the session check at startup, and again after a login or a
    // register. Deliberately not on the dashboard: opening `/book/{id}` directly (a
    // bookmark, a reload while writing) never renders the dashboard, and an
    // author's own words have to survive that. `enter_user` is idempotent, so
    // re-running it on any `current_user` change is free.
    __scope.create_effect(move || {
        if let Some(user) = store.current_user.get() {
            local_dictionary::enter_user(user.id, store);
        }
    });

    // Does this device want its writing spellchecked? Read before any editor
    // exists, so the first chapter opened is already right (the default is on, so
    // a slow read only ever turns it off a moment later — never on).
    spell::settings::hydrate(store);

    // Which books are cut over, as far as this device was last told. Read before the
    // session check, because it decides whether writing on this device can reach the
    // server at all: a cut-over book takes body edits through sync only, and a device
    // that started offline cannot wait for a fetch to find that out.
    sync::hydrate_cutover(store);

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
    } else if matches!(initial_route, Route::ThemePreview | Route::EditorSpike | Route::OpfsSpike) {
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

/// The desktop application id.
///
/// Must match the basename of the installed desktop entry
/// (`~/.local/share/applications/dev.lostconnection.plotweb.desktop`, written by
/// `scripts/install-desktop.sh`). That match is the whole mechanism by which a
/// Wayland compositor finds the app's icon and groups its window: Wayland has no
/// protocol for a client to hand over a window icon, so the taskbar looks up the
/// surface's `app_id` in the desktop-entry database. Change one, change the other.
#[cfg(not(target_arch = "wasm32"))]
const APP_ID: &str = "dev.lostconnection.plotweb";

/// Desktop entry: run a native window via rinch's shell (winit/wgpu).
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    // `icon` covers X11/Windows, which take the bytes directly; `app_id` covers
    // Wayland, which resolves the icon through the desktop entry (see `APP_ID`).
    // Window *position* is deliberately not restored: Wayland gives clients no way
    // to place their own surface, and rinch exposes no accessor for the live window
    // size either, so there is nothing to save at exit.
    let props = rinch::WindowProps {
        title: "PlotWeb".into(),
        width: 1200,
        height: 800,
        icon: Some(include_bytes!("../assets/icon.png")),
        app_id: Some(APP_ID.to_string()),
        ..Default::default()
    };
    rinch::run_with_window_props(app, props, Some(theme_props()));
}

#[cfg(target_arch = "wasm32")]
fn main() {}
