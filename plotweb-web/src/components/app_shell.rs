use rinch::prelude::*;
use rinch_core::use_store;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};

use crate::api;
use crate::store::{AppStore, Route};

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
}

.app-shell {
    display: flex;
    height: 100vh;
}

.sidebar {
    width: 240px;
    min-width: 240px;
    padding: var(--rinch-spacing-md);
    border-right: 1px solid var(--rinch-color-gray-3);
    background: var(--rinch-color-body);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
}

.sidebar-header {
    padding: var(--rinch-spacing-sm) var(--rinch-spacing-xs);
    display: flex;
    align-items: center;
    justify-content: space-between;
}

.sidebar-nav {
    flex: 1;
}

.sidebar-footer {
    padding: var(--rinch-spacing-sm) 0;
    border-top: 1px solid var(--rinch-color-gray-3);
    display: flex;
    align-items: center;
    justify-content: space-between;
}

.main-content {
    flex: 1;
    overflow-y: auto;
    padding: var(--rinch-spacing-xl);
}

.main-content-full {
    flex: 1;
    overflow: hidden;
}

.auth-page {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
    background: var(--rinch-color-body);
}

.dashboard {
    max-width: 900px;
    margin: 0 auto;
}

.dashboard-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
}

.book-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: var(--rinch-spacing-md);
}

.book-card {
    cursor: pointer;
    transition: box-shadow 0.15s ease;
}

.book-card:hover {
    box-shadow: var(--rinch-shadow-md);
}

.book-card-content {
    cursor: pointer;
}

.book-page {
    max-width: 700px;
    margin: 0 auto;
}

.chapters-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
}

.chapter-list {
    display: flex;
    flex-direction: column;
    gap: var(--rinch-spacing-xs);
}

.chapter-item {
    cursor: pointer;
    transition: background 0.1s;
}

.chapter-item:hover {
    background: var(--rinch-color-gray-1);
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
}

.chapter-item-actions {
    display: flex;
    gap: 2px;
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
            style { {APP_SHELL_CSS} }

            match store.current_route.get() {
                Route::Login => div {
                    {crate::pages::login::login_page(__scope)}
                },
                Route::Register => div {
                    {crate::pages::register::register_page(__scope)}
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
