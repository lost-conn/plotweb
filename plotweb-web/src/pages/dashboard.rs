use rinch::prelude::*;
use rinch_core::use_store;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{Book, CreateBookRequest};

use crate::api;
use crate::store::{AppStore, Route};

const DASHBOARD_CSS: &str = r#"
.dash-topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 24px;
    border-bottom: 1px solid var(--rinch-color-border);
    background: var(--pw-color-deep);
    flex-shrink: 0;
}

.dash-topbar-right {
    display: flex;
    align-items: center;
    gap: 8px;
}

.dash-body {
    flex: 1;
    overflow-y: auto;
    padding: 40px 48px;
}

.book-shelf {
    display: flex;
    flex-direction: row;
    gap: 24px;
    padding: 16px 0;
    overflow-x: auto;
    scroll-snap-type: x proximity;
}

.book-card {
    position: relative;
    width: 180px;
    min-width: 180px;
    height: 260px;
    border-radius: 12px 12px 4px 4px;
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
    cursor: pointer;
    transition: transform 0.15s ease, box-shadow 0.2s ease;
    scroll-snap-align: start;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    overflow: hidden;
}

.book-card::before {
    content: '';
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 4px;
    background: var(--rinch-color-teal-6);
    border-radius: 12px 0 0 4px;
}

.book-card:hover {
    transform: translateY(-6px);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.2);
}

.book-card-title {
    padding: 12px 16px 12px 16px;
    font-weight: 600;
    font-size: 14px;
    line-height: 1.3;
    color: var(--rinch-color-text);
    word-break: break-word;
}

.book-card-delete {
    position: absolute;
    top: 8px;
    right: 8px;
    opacity: 0;
    transition: opacity 0.15s ease;
}

.book-card:hover .book-card-delete {
    opacity: 1;
}

.book-card-new {
    position: relative;
    width: 180px;
    min-width: 180px;
    height: 260px;
    border-radius: 12px 12px 4px 4px;
    border: 2px dashed var(--rinch-color-border);
    background: transparent;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-direction: column;
    gap: 8px;
    color: var(--rinch-color-dimmed);
    transition: border-color 0.15s ease, color 0.15s ease;
    scroll-snap-align: start;
}

.book-card-new:hover {
    border-color: var(--rinch-color-teal-6);
    color: var(--rinch-color-teal-4);
}

.dash-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 80px 0;
    text-align: center;
}

@media (max-width: 640px) {
    .dash-body {
        padding: 24px 16px;
    }
    .book-shelf {
        flex-wrap: wrap;
        justify-content: center;
        overflow-x: visible;
        scroll-snap-type: none;
    }
    .book-card, .book-card-new {
        width: 140px;
        min-width: 140px;
        height: 200px;
    }
    .book-card-title {
        font-size: 13px;
        padding: 8px 12px;
    }
}
"#;

#[component]
pub fn dashboard_page() -> NodeHandle {
    let store = use_store::<AppStore>();
    let show_modal = Signal::new(false);
    let new_title = Signal::new(String::new());
    let new_desc = Signal::new(String::new());

    // Fetch books on mount
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(books) = api::get::<Vec<Book>>("/api/books").await {
            store.books.set(books);
        }
    });

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

    let open_modal = move || {
        new_title.set(String::new());
        new_desc.set(String::new());
        show_modal.set(true);
    };

    let create_book = move || {
        let title = new_title.get();
        if title.trim().is_empty() {
            return;
        }
        let desc = new_desc.get();
        show_modal.set(false);
        wasm_bindgen_futures::spawn_local(async move {
            let req = CreateBookRequest {
                title,
                description: desc,
            };
            if let Ok(book) = api::post::<_, Book>("/api/books", &req).await {
                store.books.update(|books| books.insert(0, book));
            }
        });
    };

    let open_book = move |id: String| {
        move || {
            store.current_route.set(Route::Book(id.clone()));
        }
    };

    let delete_book = move |id: String| {
        move || {
            let id = id.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if api::delete_req::<serde_json::Value>(&format!("/api/books/{}", id)).await.is_ok() {
                    store.books.update(|books| books.retain(|b| b.id != id));
                }
            });
        }
    };

    rsx! {
        Fragment {
            style { {DASHBOARD_CSS} }

            // Top bar
            div { class: "dash-topbar",
                Title { order: 3, "PlotWeb" }
                div { class: "dash-topbar-right",
                    ActionIcon {
                        variant: "subtle",
                        size: "sm",
                        onclick: toggle_dark,
                        {render_tabler_icon(
                            __scope,
                            if store.dark_mode.get() { TablerIcon::Sun } else { TablerIcon::Moon },
                            TablerIconStyle::Outline,
                        )}
                    }
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
                        onclick: logout,
                        {render_tabler_icon(__scope, TablerIcon::Logout, TablerIconStyle::Outline)}
                    }
                }
            }

            // Body
            div { class: "dash-body",
                if store.books.get().is_empty() {
                    div { class: "dash-empty",
                        Text { size: "lg", color: "dimmed", "No books yet" }
                        Space { h: "sm" }
                        Text { size: "sm", color: "dimmed", "Create your first book to get started" }
                        Space { h: "lg" }
                        Button {
                            onclick: open_modal,
                            "New Book"
                        }
                    }
                }

                if !store.books.get().is_empty() {
                    div { class: "book-shelf",
                        for book in store.books.get() {
                            div {
                                key: book.id.clone(),
                                class: "book-card",
                                onclick: open_book(book.id.clone()),

                                div { class: "book-card-delete",
                                    ActionIcon {
                                        variant: "subtle",
                                        color: "red",
                                        size: "xs",
                                        onclick: delete_book(book.id.clone()),
                                        {render_tabler_icon(
                                            __scope,
                                            TablerIcon::Trash,
                                            TablerIconStyle::Outline,
                                        )}
                                    }
                                }
                                div { class: "book-card-title",
                                    {book.title.clone()}
                                }
                            }
                        }

                        div {
                            class: "book-card-new",
                            onclick: open_modal,
                            {render_tabler_icon(__scope, TablerIcon::Plus, TablerIconStyle::Outline)}
                            Text { size: "sm", "New Book" }
                        }
                    }
                }
            }

            // New Book Modal
            Modal {
                opened_fn: move || show_modal.get(),
                onclose: move || show_modal.set(false),
                title: "Create New Book",

                TextInput {
                    label: "Title",
                    placeholder: "Book title",
                    value_fn: move || new_title.get(),
                    oninput: move |v: String| new_title.set(v),
                    onsubmit: create_book,
                }
                Space { h: "md" }
                Textarea {
                    label: "Description",
                    placeholder: "What's this book about?",
                    value_fn: move || new_desc.get(),
                    oninput: move |v: String| new_desc.set(v),
                }
                Space { h: "lg" }
                Group {
                    justify: "flex-end",
                    Button {
                        variant: "subtle",
                        onclick: move || show_modal.set(false),
                        "Cancel"
                    }
                    Button {
                        onclick: create_book,
                        "Create"
                    }
                }
            }
        }
    }
}
