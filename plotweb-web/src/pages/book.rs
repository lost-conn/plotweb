use rinch::prelude::*;
use rinch_core::use_store;
use plotweb_common::{Book, Chapter, CreateChapterRequest, ReorderChaptersRequest};

use crate::api;
use crate::store::{AppStore, Route};

#[component]
pub fn book_page(book_id: String) -> NodeHandle {
    let store = use_store::<AppStore>();
    let show_chapter_modal = Signal::new(false);
    let new_chapter_title = Signal::new(String::new());
    let book_id_for_fetch = book_id.clone();

    // Fetch book and chapters
    let bid = book_id.clone();
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(book) = api::get::<Book>(&format!("/api/books/{}", bid)).await {
            store.current_book.set(Some(book));
        }
        if let Ok(chapters) = api::get::<Vec<Chapter>>(&format!("/api/books/{}/chapters", bid)).await {
            store.chapters.set(chapters);
        }
    });

    let open_editor = move |chapter_id: String| {
        let bid = book_id.clone();
        move || {
            store.current_route.set(Route::Editor(bid.clone(), chapter_id.clone()));
        }
    };

    let add_chapter = {
        let bid = book_id_for_fetch.clone();
        move || {
            let title = new_chapter_title.get();
            if title.trim().is_empty() {
                return;
            }
            show_chapter_modal.set(false);
            let bid = bid.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let req = CreateChapterRequest { title };
                if let Ok(chapter) = api::post::<_, Chapter>(
                    &format!("/api/books/{}/chapters", bid),
                    &req,
                ).await {
                    store.chapters.update(|ch| ch.push(chapter));
                }
            });
        }
    };

    let bid_for_move = book_id_for_fetch.clone();
    let move_chapter = move |chapter_id: String, direction: i32| {
        let bid = bid_for_move.clone();
        move || {
            store.chapters.update(|chapters| {
                let pos = chapters.iter().position(|c| c.id == chapter_id);
                if let Some(idx) = pos {
                    let new_idx = idx as i32 + direction;
                    if new_idx >= 0 && (new_idx as usize) < chapters.len() {
                        chapters.swap(idx, new_idx as usize);
                    }
                }
            });
            // Save new order
            let ids: Vec<String> = store.chapters.get().iter().map(|c| c.id.clone()).collect();
            let bid = bid.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let req = ReorderChaptersRequest { chapter_ids: ids };
                api::put::<_, serde_json::Value>(&format!("/api/books/{}/chapters/reorder", bid), &req).await.ok();
            });
        }
    };

    let delete_chapter = move |chapter_id: String| {
        let bid = book_id_for_fetch.clone();
        move || {
            let cid = chapter_id.clone();
            let bid = bid.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if api::delete_req::<serde_json::Value>(
                    &format!("/api/books/{}/chapters/{}", bid, cid)
                ).await.is_ok() {
                    store.chapters.update(|ch| ch.retain(|c| c.id != cid));
                }
            });
        }
    };

    let go_dashboard = move || {
        store.current_route.set(Route::Dashboard);
    };

    rsx! {
        Fragment {
            div { class: "book-page",
                div { class: "book-header",
                    Group {
                        Button {
                            variant: "subtle",
                            onclick: go_dashboard,
                            "Back to Books"
                        }
                    }
                    Space { h: "md" }
                    if store.current_book.get().is_some() {
                        Title { order: 2, {store.current_book.get().unwrap().title.clone()} }
                        if !store.current_book.get().unwrap().description.is_empty() {
                            Text { color: "dimmed", {store.current_book.get().unwrap().description.clone()} }
                        }
                    }
                }

                Space { h: "lg" }

                div { class: "chapters-header",
                    Title { order: 4, "Chapters" }
                    Button {
                        size: "sm",
                        onclick: move || {
                            new_chapter_title.set(String::new());
                            show_chapter_modal.set(true);
                        },
                        "Add Chapter"
                    }
                }
                Space { h: "md" }

                if store.chapters.get().is_empty() {
                    Center {
                        style: "padding: 40px 0;",
                        Text { color: "dimmed", "No chapters yet. Add one to start writing!" }
                    }
                }

                div { class: "chapter-list",
                    for (i, chapter) in store.chapters.get().into_iter().enumerate() {
                        Paper {
                            key: chapter.id.clone(),
                            shadow: "xs",
                            p: "md",
                            radius: "sm",
                            class: "chapter-item",

                            div {
                                class: "chapter-item-content",
                                div {
                                    class: "chapter-item-left",
                                    onclick: open_editor(chapter.id.clone()),

                                    Badge {
                                        variant: "light",
                                        size: "sm",
                                        {format!("{}", i + 1)}
                                    }
                                    Text { weight: "500", {chapter.title.clone()} }
                                }
                                div {
                                    class: "chapter-item-actions",
                                    ActionIcon {
                                        variant: "subtle",
                                        size: "sm",
                                        disabled: i == 0,
                                        onclick: move_chapter(chapter.id.clone(), -1),
                                        {rinch_tabler_icons::render_tabler_icon(
                                            __scope,
                                            rinch_tabler_icons::TablerIcon::ChevronUp,
                                            rinch_tabler_icons::TablerIconStyle::Outline,
                                        )}
                                    }
                                    ActionIcon {
                                        variant: "subtle",
                                        size: "sm",
                                        disabled: i == store.chapters.get().len() - 1,
                                        onclick: move_chapter(chapter.id.clone(), 1),
                                        {rinch_tabler_icons::render_tabler_icon(
                                            __scope,
                                            rinch_tabler_icons::TablerIcon::ChevronDown,
                                            rinch_tabler_icons::TablerIconStyle::Outline,
                                        )}
                                    }
                                    ActionIcon {
                                        variant: "subtle",
                                        color: "red",
                                        size: "sm",
                                        onclick: delete_chapter(chapter.id.clone()),
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
            }

            // Add Chapter Modal
            Modal {
                opened_fn: move || show_chapter_modal.get(),
                onclose: move || show_chapter_modal.set(false),
                title: "Add Chapter",

                TextInput {
                    label: "Chapter Title",
                    placeholder: "Enter chapter title",
                    value_fn: move || new_chapter_title.get(),
                    oninput: move |v: String| new_chapter_title.set(v),
                }
                Space { h: "lg" }
                Group {
                    justify: "flex-end",
                    Button {
                        variant: "subtle",
                        onclick: move || show_chapter_modal.set(false),
                        "Cancel"
                    }
                    Button {
                        onclick: add_chapter,
                        "Add"
                    }
                }
            }
        }
    }
}
