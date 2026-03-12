use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use rinch::prelude::*;
use rinch_core::use_store;
use plotweb_common::{Book, Chapter, CreateChapterRequest, FontSettings, ReorderChaptersRequest, UpdateBookRequest};

use crate::api;
use crate::fonts;
use crate::store::{AppStore, Route};

/// CSS for the typography settings section.
const TYPOGRAPHY_CSS: &str = r#"
.typography-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    padding: 8px 0;
    color: var(--rinch-color-dimmed);
    font-size: 14px;
    user-select: none;
    transition: color 0.15s;
}
.typography-toggle:hover { color: var(--rinch-color-text); }

.typography-section {
    padding: 16px 0;
}

.font-selector-grid {
    display: grid;
    grid-template-columns: 120px 1fr;
    gap: 8px 16px;
    align-items: center;
}

.font-selector-label {
    font-size: 13px;
    color: var(--rinch-color-dimmed);
    text-align: right;
}

.font-picker {
    position: relative;
}

.font-picker input {
    width: 100%;
    box-sizing: border-box;
    padding: 6px 10px;
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--rinch-radius-sm);
    background: var(--rinch-color-surface);
    color: var(--rinch-color-text);
    font-size: 13px;
    outline: none;
    transition: border-color 0.15s;
}
.font-picker input:focus {
    border-color: var(--rinch-color-teal-6);
}
.font-picker input::placeholder {
    color: var(--rinch-color-dimmed);
    opacity: 0.7;
}

.font-picker .font-dropdown {
    display: none;
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    max-height: 240px;
    overflow-y: auto;
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
    border-top: none;
    border-radius: 0 0 var(--rinch-radius-sm) var(--rinch-radius-sm);
    z-index: 100;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
}
.font-picker .font-dropdown.open {
    display: block;
}

.font-dropdown .font-option {
    padding: 6px 10px;
    font-size: 13px;
    cursor: pointer;
    display: flex;
    justify-content: space-between;
    align-items: center;
    color: var(--rinch-color-text);
}
.font-dropdown .font-option:hover,
.font-dropdown .font-option.active {
    background: var(--rinch-color-teal-9);
}
.font-dropdown .font-option .font-category {
    font-size: 11px;
    color: var(--rinch-color-dimmed);
    margin-left: 8px;
    flex-shrink: 0;
}
.font-dropdown .font-option-default {
    padding: 6px 10px;
    font-size: 13px;
    cursor: pointer;
    color: var(--rinch-color-dimmed);
    font-style: italic;
    border-bottom: 1px solid var(--rinch-color-border);
}
.font-dropdown .font-option-default:hover {
    background: var(--rinch-color-teal-9);
}
.font-dropdown .font-empty {
    padding: 12px 10px;
    font-size: 13px;
    color: var(--rinch-color-dimmed);
    text-align: center;
}
.font-dropdown .font-loading {
    padding: 12px 10px;
    font-size: 13px;
    color: var(--rinch-color-dimmed);
    text-align: center;
}

.font-preview-box {
    margin-top: 16px;
    padding: 20px;
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--rinch-radius-sm);
    background: var(--rinch-color-body);
}

.font-preview-box h3 {
    font-size: 1.5em;
    margin: 0 0 8px 0;
    font-weight: 700;
}

.font-preview-box h4 {
    font-size: 1.15em;
    margin: 0 0 8px 0;
    font-weight: 600;
    color: var(--rinch-color-dimmed);
}

.font-preview-box .preview-body {
    margin: 0 0 8px 0;
    line-height: 1.7;
}

.font-preview-box blockquote {
    border-left: 3px solid var(--rinch-color-teal-8);
    padding-left: 12px;
    margin: 8px 0;
    color: var(--rinch-color-dimmed);
}

.font-preview-box code {
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
    padding: 2px 5px;
    border-radius: 3px;
    font-size: 0.9em;
}
"#;

/// Slot definitions: (data attribute name, label, default placeholder, category filter)
const SLOTS: &[(&str, &str, &str, &str)] = &[
    ("h1", "Heading 1", "Macondo Swash Caps", ""),
    ("h2", "Heading 2", "Macondo Swash Caps", ""),
    ("h3", "Heading 3+", "Macondo Swash Caps", ""),
    ("body", "Body Text", "Andada Pro", ""),
    ("quote", "Blockquote", "inherit", ""),
    ("code", "Code", "monospace", "monospace"),
];

/// Build the HTML for the font picker grid.
fn build_picker_grid_html(fs: &FontSettings) -> String {
    let mut html = String::new();
    for &(slot, label, default, _) in SLOTS {
        let current = match slot {
            "h1" => fs.h1.as_deref(),
            "h2" => fs.h2.as_deref(),
            "h3" => fs.h3.as_deref(),
            "body" => fs.body.as_deref(),
            "quote" => fs.quote.as_deref(),
            "code" => fs.code.as_deref(),
            _ => None,
        };
        let display_value = current.unwrap_or("");
        let placeholder = if default == "inherit" || default == "monospace" {
            format!("Search fonts... (default: {})", default)
        } else {
            format!("Search fonts... (default: {})", default)
        };
        html.push_str(&format!(
            "<span class='font-selector-label'>{label}</span>\
             <div class='font-picker' data-font-slot='{slot}'>\
               <input type='text' value='{value}' placeholder='{placeholder}' autocomplete='off' />\
               <div class='font-dropdown'></div>\
             </div>",
            label = label,
            slot = slot,
            value = display_value,
            placeholder = placeholder,
        ));
    }
    html
}

#[component]
pub fn book_page(book_id: String) -> NodeHandle {
    let store = use_store::<AppStore>();
    let show_chapter_modal = Signal::new(false);
    let new_chapter_title = Signal::new(String::new());
    let book_id_for_fetch = book_id.clone();

    let typography_open = Signal::new(false);
    let font_settings = Signal::new(FontSettings::default());
    let save_timer_id = Signal::new(0_i32);
    let listeners_attached = Signal::new(false);

    // Trigger font catalog fetch
    fonts::fetch_font_catalog();

    // Fetch book and chapters
    let bid = book_id.clone();
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(book) = api::get::<Book>(&format!("/api/books/{}", bid)).await {
            font_settings.set(book.font_settings.clone().unwrap_or_default());
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
            let ids: Vec<String> = store.chapters.get().iter().map(|c| c.id.clone()).collect();
            let bid = bid.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let req = ReorderChaptersRequest { chapter_ids: ids };
                api::put::<_, serde_json::Value>(&format!("/api/books/{}/chapters/reorder", bid), &req).await.ok();
            });
        }
    };

    let bid_for_save = book_id_for_fetch.clone();
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

    // Setup font picker inputs after typography section opens
    let setup_font_pickers = move || {
        let bid_for_save = bid_for_save.clone();
        let closure = wasm_bindgen::closure::Closure::once(move || {
            if listeners_attached.get() {
                return;
            }
            let document = web_sys::window().unwrap().document().unwrap();

            // Populate the picker grid
            let fs = font_settings.get();
            if let Ok(Some(grid)) = document.query_selector("#font-selector-grid") {
                grid.set_inner_html(&build_picker_grid_html(&fs));
            }

            // Populate preview
            update_preview(&fs);

            // Attach event listeners to each font picker
            for &(slot_name, _, _, cat_filter) in SLOTS {
                let selector = format!(".font-picker[data-font-slot='{}']", slot_name);
                let picker_el = match document.query_selector(&selector) {
                    Ok(Some(el)) => el,
                    _ => continue,
                };
                let input_el: web_sys::HtmlInputElement = match picker_el.query_selector("input") {
                    Ok(Some(el)) => el.dyn_into().unwrap(),
                    _ => continue,
                };
                let dropdown_el = match picker_el.query_selector(".font-dropdown") {
                    Ok(Some(el)) => el,
                    _ => continue,
                };

                let slot = slot_name.to_string();
                let cat = cat_filter.to_string();
                let bid = bid_for_save.clone();
                let input_ref = input_el.clone();
                let dropdown_ref = dropdown_el.clone();

                // Helper: apply a font selection
                let apply_font = {
                    let slot = slot.clone();
                    let bid = bid.clone();
                    let input_ref = input_ref.clone();
                    let dropdown_ref = dropdown_ref.clone();
                    move |family: String| {
                        let new_value = if family.is_empty() { None } else { Some(family.clone()) };
                        input_ref.set_value(&family);
                        dropdown_ref.class_list().remove_1("open").ok();

                        let mut fs = font_settings.get();
                        match slot.as_str() {
                            "h1" => fs.h1 = new_value,
                            "h2" => fs.h2 = new_value,
                            "h3" => fs.h3 = new_value,
                            "body" => fs.body = new_value,
                            "quote" => fs.quote = new_value,
                            "code" => fs.code = new_value,
                            _ => {}
                        }

                        fonts::load_book_fonts(&fs);
                        font_settings.set(fs.clone());
                        store.current_book.update(|book| {
                            if let Some(b) = book {
                                b.font_settings = Some(fs.clone());
                            }
                        });

                        // Debounced save
                        let prev = save_timer_id.get();
                        if prev != 0 {
                            if let Some(w) = web_sys::window() {
                                w.clear_timeout_with_handle(prev);
                            }
                        }
                        let bid = bid.clone();
                        let save_closure = wasm_bindgen::closure::Closure::once(move || {
                            wasm_bindgen_futures::spawn_local(async move {
                                let req = UpdateBookRequest {
                                    title: None,
                                    description: None,
                                    font_settings: Some(fs),
                                };
                                api::put::<_, serde_json::Value>(
                                    &format!("/api/books/{}", bid),
                                    &req,
                                ).await.ok();
                            });
                        });
                        let id = web_sys::window()
                            .unwrap()
                            .set_timeout_with_callback_and_timeout_and_arguments_0(
                                save_closure.as_ref().unchecked_ref(),
                                500,
                            )
                            .unwrap_or(0);
                        save_closure.forget();
                        save_timer_id.set(id);
                        update_preview(&font_settings.get());
                    }
                };

                // Input handler: filter and show dropdown
                let apply_for_input = apply_font.clone();
                let dropdown_for_input = dropdown_ref.clone();
                let cat_for_input = cat.clone();
                let input_handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |_event: web_sys::Event| {
                    let query = input_ref.value().to_lowercase();
                    let catalog = fonts::font_catalog().get();
                    let dropdown = &dropdown_for_input;

                    if catalog.is_empty() {
                        dropdown.set_inner_html("<div class='font-loading'>Loading fonts...</div>");
                        dropdown.class_list().add_1("open").ok();
                        return;
                    }

                    let filtered: Vec<&fonts::FontInfo> = catalog.iter()
                        .filter(|f| {
                            if !cat_for_input.is_empty() && f.category != cat_for_input {
                                return false;
                            }
                            if query.is_empty() {
                                return true;
                            }
                            f.family.to_lowercase().contains(&query)
                        })
                        .take(50)
                        .collect();

                    let mut html = String::from(
                        "<div class='font-option-default' data-font-value=''>Reset to default</div>"
                    );

                    if filtered.is_empty() {
                        html.push_str("<div class='font-empty'>No fonts found</div>");
                    } else {
                        for f in &filtered {
                            html.push_str(&format!(
                                "<div class='font-option' data-font-value='{family}'>\
                                   <span>{family}</span>\
                                   <span class='font-category'>{cat}</span>\
                                 </div>",
                                family = f.family,
                                cat = f.category,
                            ));
                        }
                    }

                    dropdown.set_inner_html(&html);
                    dropdown.class_list().add_1("open").ok();

                    // Attach click handlers to options
                    let options = dropdown.query_selector_all("[data-font-value]").unwrap();
                    for i in 0..options.length() {
                        if let Some(opt) = options.item(i) {
                            let el: web_sys::Element = opt.dyn_into().unwrap();
                            let value = el.get_attribute("data-font-value").unwrap_or_default();
                            let apply = apply_for_input.clone();
                            let click_handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |e: web_sys::Event| {
                                e.prevent_default();
                                e.stop_propagation();
                                let v = value.clone();
                                // Load the font for preview before applying
                                if !v.is_empty() {
                                    fonts::load_single_font(&v);
                                }
                                apply(v);
                            }) as Box<dyn FnMut(_)>);
                            el.add_event_listener_with_callback("mousedown", click_handler.as_ref().unchecked_ref()).ok();
                            click_handler.forget();
                        }
                    }
                }) as Box<dyn FnMut(_)>);
                input_el.add_event_listener_with_callback("input", input_handler.as_ref().unchecked_ref()).ok();
                input_handler.forget();

                // Focus handler: show dropdown
                let input_for_focus = input_el.clone();
                let dropdown_for_focus = dropdown_ref.clone();
                let cat_for_focus = cat.clone();
                let apply_for_focus = apply_font.clone();
                let focus_handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |_event: web_sys::Event| {
                    // Trigger the same filtering as input
                    let query = input_for_focus.value().to_lowercase();
                    let catalog = fonts::font_catalog().get();
                    let dropdown = &dropdown_for_focus;

                    if catalog.is_empty() {
                        dropdown.set_inner_html("<div class='font-loading'>Loading fonts...</div>");
                        dropdown.class_list().add_1("open").ok();
                        return;
                    }

                    let filtered: Vec<&fonts::FontInfo> = catalog.iter()
                        .filter(|f| {
                            if !cat_for_focus.is_empty() && f.category != cat_for_focus {
                                return false;
                            }
                            if query.is_empty() {
                                return true;
                            }
                            f.family.to_lowercase().contains(&query)
                        })
                        .take(50)
                        .collect();

                    let mut html = String::from(
                        "<div class='font-option-default' data-font-value=''>Reset to default</div>"
                    );
                    if filtered.is_empty() {
                        html.push_str("<div class='font-empty'>No fonts found</div>");
                    } else {
                        for f in &filtered {
                            html.push_str(&format!(
                                "<div class='font-option' data-font-value='{family}'>\
                                   <span>{family}</span>\
                                   <span class='font-category'>{cat}</span>\
                                 </div>",
                                family = f.family,
                                cat = f.category,
                            ));
                        }
                    }
                    dropdown.set_inner_html(&html);
                    dropdown.class_list().add_1("open").ok();

                    let options = dropdown.query_selector_all("[data-font-value]").unwrap();
                    for i in 0..options.length() {
                        if let Some(opt) = options.item(i) {
                            let el: web_sys::Element = opt.dyn_into().unwrap();
                            let value = el.get_attribute("data-font-value").unwrap_or_default();
                            let apply = apply_for_focus.clone();
                            let click_handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |e: web_sys::Event| {
                                e.prevent_default();
                                e.stop_propagation();
                                let v = value.clone();
                                if !v.is_empty() {
                                    fonts::load_single_font(&v);
                                }
                                apply(v);
                            }) as Box<dyn FnMut(_)>);
                            el.add_event_listener_with_callback("mousedown", click_handler.as_ref().unchecked_ref()).ok();
                            click_handler.forget();
                        }
                    }
                }) as Box<dyn FnMut(_)>);
                input_el.add_event_listener_with_callback("focus", focus_handler.as_ref().unchecked_ref()).ok();
                focus_handler.forget();

                // Blur handler: close dropdown
                let dropdown_for_blur = dropdown_ref.clone();
                let blur_handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |_event: web_sys::Event| {
                    dropdown_for_blur.class_list().remove_1("open").ok();
                }) as Box<dyn FnMut(_)>);
                input_el.add_event_listener_with_callback("blur", blur_handler.as_ref().unchecked_ref()).ok();
                blur_handler.forget();
            }

            listeners_attached.set(true);
        });
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                100,
            )
            .ok();
        closure.forget();
    };

    let toggle_typography = move || {
        let opening = !typography_open.get();
        typography_open.set(opening);
        if opening {
            listeners_attached.set(false);
            setup_font_pickers();
        }
    };

    rsx! {
        Fragment {
            style { {TYPOGRAPHY_CSS} }
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

                Space { h: "md" }

                // Typography section
                div {
                    class: "typography-toggle",
                    onclick: toggle_typography,
                    {move || if typography_open.get() { "\u{25be} Typography" } else { "\u{25b8} Typography" }}
                }

                if typography_open.get() {
                    div { class: "typography-section",
                        div { class: "font-selector-grid", id: "font-selector-grid" }
                        Space { h: "md" }
                        Text { size: "sm", color: "dimmed", "Preview:" }
                        div { class: "font-preview-box", id: "font-preview" }
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

/// Update the font preview box styles.
fn update_preview(fs: &FontSettings) {
    let document = match web_sys::window().and_then(|w| w.document()) {
        Some(d) => d,
        None => return,
    };

    if let Ok(Some(el)) = document.query_selector("#font-preview") {
        let h1 = fs.h1.as_deref().unwrap_or("Macondo Swash Caps");
        let h2 = fs.h2.as_deref().unwrap_or("Macondo Swash Caps");
        let body = fs.body.as_deref().unwrap_or("Andada Pro");
        let quote = fs.quote.as_deref().unwrap_or("inherit");
        let code = fs.code.as_deref().unwrap_or("monospace");

        el.set_inner_html(&format!(
            "<h3 style=\"font-family: '{}', cursive\">Chapter Title</h3>\
             <h4 style=\"font-family: '{}', cursive\">Scene Heading</h4>\
             <p class='preview-body' style=\"font-family: '{}', serif\">The quick brown fox jumps over the lazy dog. She stared out the window, watching the rain trace paths down the glass.</p>\
             <blockquote style=\"font-family: '{}', serif\">\u{201c}All that glitters is not gold.\u{201d}</blockquote>\
             <p><code style=\"font-family: '{}', monospace\">const story = new Adventure();</code></p>",
            h1, h2, body, quote, code
        ));
    }
}
