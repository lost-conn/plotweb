use rinch::prelude::*;
use rinch_core::use_store;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};

use crate::api;
use crate::store::{AppStore, Route};

/// Returns a `:root { ... }` CSS block with warm color overrides for the given mode.
fn warm_overrides(dark: bool) -> String {
    let font = "--rinch-font-family: 'Andada Pro', Georgia, 'Times New Roman', serif;";
    if dark {
        format!(":root {{
  {font}
  --rinch-color-body: #1C1917;
  --rinch-color-surface: #262220;
  --rinch-color-text: #E7E0D8;
  --rinch-color-dimmed: #9C9489;
  --rinch-color-border: #3D3733;
  --rinch-color-placeholder: #6B6359;
  --pw-color-deep: #1A1714;
  --pw-color-deepest: #14120F;
}}")
    } else {
        format!(":root {{
  {font}
  --rinch-color-body: #FAF8F5;
  --rinch-color-surface: #FFFFFF;
  --rinch-color-text: #2C2520;
  --rinch-color-dimmed: #8C8278;
  --rinch-color-border: #E0D8CF;
  --rinch-color-placeholder: #A89E94;
  --pw-color-deep: #F3F0EC;
  --pw-color-deepest: #EDE9E3;
}}")
    }
}

/// CSS for the app shell layout.
const APP_SHELL_CSS: &str = r#"
* {
    box-sizing: border-box;
    margin: 0;
    padding: 0;
}

html, body {
    height: 100vh;
    font-family: var(--rinch-font-family);
    background: var(--rinch-color-body);
    color: var(--rinch-color-text);
    overflow: hidden;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
}

h1, h2, h3, h4, h5, h6, .rinch-title {
    font-family: 'Macondo Swash Caps', cursive;
}

/* ── App Shell ─────────────────────────────────────────── */

.app-shell {
    display: flex;
    height: 100vh;
}

.sidebar {
    width: 250px;
    min-width: 250px;
    padding: var(--rinch-spacing-md) var(--rinch-spacing-md) var(--rinch-spacing-sm);
    border-right: 1px solid var(--rinch-color-border);
    background: var(--pw-color-deep);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
}

.sidebar-header {
    padding: var(--rinch-spacing-sm) var(--rinch-spacing-xs);
    margin-bottom: var(--rinch-spacing-xs);
    display: flex;
    align-items: center;
    justify-content: space-between;
}

.sidebar-header h3 {
    letter-spacing: 0.02em;
}

.sidebar-nav {
    flex: 1;
    padding-top: var(--rinch-spacing-xs);
}

.sidebar-footer {
    padding: var(--rinch-spacing-sm) var(--rinch-spacing-xs);
    margin-top: var(--rinch-spacing-sm);
    border-top: 1px solid var(--rinch-color-border);
    display: flex;
    align-items: center;
    justify-content: space-between;
}

.main-content {
    flex: 1;
    overflow-y: auto;
    padding: 40px 48px;
    background: var(--rinch-color-body);
}

.main-content-full {
    flex: 1;
    overflow: hidden;
    background: var(--rinch-color-body);
}

/* ── Auth Pages ────────────────────────────────────────── */

.auth-page {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
    background: linear-gradient(
        160deg,
        var(--pw-color-deepest) 0%,
        var(--rinch-color-body) 50%,
        var(--pw-color-deep) 100%
    );
}

.auth-page .rinch-paper {
    border: 1px solid var(--rinch-color-border);
    background: var(--rinch-color-surface);
}

.auth-page h2 {
    color: var(--rinch-color-teal-4);
}

/* ── Dashboard ─────────────────────────────────────────── */

.dashboard {
    max-width: 960px;
    margin: 0 auto;
}

.dashboard-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--rinch-spacing-sm);
}

.dashboard-header h2 {
    color: var(--rinch-color-text);
    font-weight: 600;
}

.book-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: var(--rinch-spacing-lg);
}

.book-card {
    cursor: pointer;
    transition: transform 0.15s ease, box-shadow 0.2s ease, border-color 0.2s ease;
    border: 1px solid var(--rinch-color-border);
    background: var(--rinch-color-surface);
}

.book-card:hover {
    box-shadow: var(--rinch-shadow-md);
    transform: translateY(-2px);
    border-color: var(--rinch-color-teal-8);
}

.book-card h4 {
    color: var(--rinch-color-text);
}

.book-card-content {
    cursor: pointer;
}

/* ── Book Page ─────────────────────────────────────────── */

.book-page {
    max-width: 720px;
    margin: 0 auto;
}

.book-header h2 {
    color: var(--rinch-color-text);
}

.chapters-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
}

.chapter-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
}

.chapter-item {
    cursor: pointer;
    transition: background 0.12s ease, border-color 0.12s ease;
    border: 1px solid transparent;
    background: var(--rinch-color-surface);
}

.chapter-item:hover {
    background: var(--rinch-color-border);
    border-color: var(--rinch-color-border);
}

.chapter-item-content {
    display: flex;
    align-items: center;
    justify-content: space-between;
}

.chapter-item-left {
    display: flex;
    align-items: center;
    gap: var(--rinch-spacing-sm);
    flex: 1;
    cursor: pointer;
    padding: 2px 0;
}

.chapter-item-actions {
    display: flex;
    gap: 2px;
    opacity: 0.4;
    transition: opacity 0.15s ease;
}

.chapter-item:hover .chapter-item-actions {
    opacity: 1;
}

/* ── Scrollbar ─────────────────────────────────────────── */

::-webkit-scrollbar {
    width: 8px;
}

::-webkit-scrollbar-track {
    background: transparent;
}

::-webkit-scrollbar-thumb {
    background: var(--rinch-color-border);
    border-radius: 4px;
}

::-webkit-scrollbar-thumb:hover {
    background: var(--rinch-color-dimmed);
}

/* ── Selection ─────────────────────────────────────────── */

::selection {
    background: var(--rinch-color-teal-8);
    color: var(--rinch-color-teal-1);
}
"#;

fn sidebar(
    __scope: &mut RenderScope,
    store: AppStore,
    toggle_dark: impl Fn() + 'static + Copy,
    logout: impl Fn() + 'static + Copy,
) -> NodeHandle {
    rsx! {
        div { class: "sidebar",
            div { class: "sidebar-header",
                Title { order: 3, "PlotWeb" }
                ActionIcon {
                    variant: "subtle",
                    size: "sm",
                    onclick: move || toggle_dark(),
                    {render_tabler_icon(
                        __scope,
                        if store.dark_mode.get() { TablerIcon::Sun } else { TablerIcon::Moon },
                        TablerIconStyle::Outline,
                    )}
                }
            }
            Space { h: "md" }

            div { class: "sidebar-nav",
                NavLink {
                    label: "My Books",
                    active_fn: move || matches!(store.current_route.get(), Route::Dashboard),
                    onclick: move || store.current_route.set(Route::Dashboard),
                }
            }

            div { class: "sidebar-footer",
                Text {
                    size: "sm",
                    color: "dimmed",
                    {|| {
                        let store = use_store::<AppStore>();
                        store.current_user.get()
                            .map(|u| u.username.clone())
                            .unwrap_or_default()
                    }}
                }
                ActionIcon {
                    variant: "subtle",
                    size: "sm",
                    onclick: move || logout(),
                    {render_tabler_icon(__scope, TablerIcon::Logout, TablerIconStyle::Outline)}
                }
            }
        }
    }
}

#[component]
pub fn app_shell() -> NodeHandle {
    let store = use_store::<AppStore>();

    let logout = move || {
        wasm_bindgen_futures::spawn_local(async move {
            api::post::<_, serde_json::Value>("/api/auth/logout", &serde_json::json!({})).await.ok();
            store.current_user.set(None);
            store.current_route.set(Route::Login);
        });
    };

    let toggle_dark = move || {
        store.dark_mode.update(|d| *d = !*d);
    };

    rsx! {
        Fragment {
            style { {|| warm_overrides(store.dark_mode.get())} }
            style { {APP_SHELL_CSS} }

            match store.current_route.get() {
                Route::Login => div {
                    {crate::pages::login::login_page(__scope)}
                },
                Route::Register => div {
                    {crate::pages::register::register_page(__scope)}
                },
                Route::ThemePreview => div {
                    class: "theme-preview-standalone",
                    style: "height: 100vh; overflow-y: auto; padding: 40px 48px; background: var(--rinch-color-body);",
                    {crate::pages::theme_preview::theme_preview_page(__scope)}
                },
                Route::Editor(book_id, chapter_id) => div { class: "app-shell",
                    div { class: "main-content-full",
                        {crate::pages::editor::editor_page(__scope, book_id, chapter_id)}
                    }
                },
                Route::Dashboard => div { class: "app-shell",
                    {sidebar(__scope, store, toggle_dark, logout)}
                    div { class: "main-content",
                        {crate::pages::dashboard::dashboard_page(__scope)}
                    }
                },
                Route::Book(id) => div { class: "app-shell",
                    {sidebar(__scope, store, toggle_dark, logout)}
                    div { class: "main-content",
                        {crate::pages::book::book_page(__scope, id)}
                    }
                },
            }
        }
    }
}
