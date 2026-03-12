use rinch::prelude::*;
use rinch_core::use_store;
use plotweb_common::{Book, CreateBookRequest};

use crate::api;
use crate::store::{AppStore, Route};

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
            div { class: "dashboard",
                div { class: "dashboard-header",
                    Title { order: 2, "Your Books" }
                    Button {
                        onclick: open_modal,
                        "New Book"
                    }
                }
                Space { h: "lg" }

                if store.books.get().is_empty() {
                    Center {
                        style: "padding: 60px 0;",
                        div {
                            style: "text-align: center;",
                            Text { size: "lg", color: "dimmed", "No books yet" }
                            Space { h: "sm" }
                            Text { size: "sm", color: "dimmed", "Create your first book to get started" }
                        }
                    }
                }

                div { class: "book-grid",
                    for book in store.books.get() {
                        Card {
                            key: book.id.clone(),
                            shadow: "sm",
                            padding: "lg",
                            radius: "md",
                            class: "book-card",

                            div {
                                class: "book-card-content",
                                onclick: open_book(book.id.clone()),
                                Title { order: 4, {book.title.clone()} }
                                Text { size: "sm", color: "dimmed", {book.description.clone()} }
                                Space { h: "sm" }
                                Badge {
                                    variant: "light",
                                    {format!("{} chapters", book.chapter_count.unwrap_or(0))}
                                }
                            }
                            Space { h: "sm" }
                            div {
                                style: "display: flex; justify-content: flex-end;",
                                ActionIcon {
                                    variant: "subtle",
                                    color: "red",
                                    size: "sm",
                                    onclick: delete_book(book.id.clone()),
                                    {rinch_tabler_icons::render_tabler_icon(
                                        __scope,
                                        rinch_tabler_icons::TablerIcon::Trash,
                                        rinch_tabler_icons::TablerIconStyle::Outline,
                                    )}
                                }
                            }
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
