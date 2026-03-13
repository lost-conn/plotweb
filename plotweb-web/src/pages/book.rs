use wasm_bindgen::JsCast;
use rinch::prelude::*;
use rinch_core::use_store;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{Book, Chapter, CreateChapterRequest, FontSettings, ReorderChaptersRequest, UpdateBookRequest, UpdateChapterRequest};

use crate::api;
use crate::fonts;
use crate::pages::editor_utils;
use crate::store::{AppStore, Route};

/// What the main pane shows.
#[derive(Clone, PartialEq)]
enum BookPane {
    Chapters,
    Editor(String),
    Typography,
}

/// CSS for the typography settings section.
const TYPOGRAPHY_CSS: &str = r#"
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

/// CSS for the book workspace layout.
const BOOK_WORKSPACE_CSS: &str = r#"
.book-workspace {
    display: flex;
    height: 100vh;
}

.book-sidebar {
    width: 250px;
    min-width: 250px;
    padding: var(--rinch-spacing-md) var(--rinch-spacing-md) var(--rinch-spacing-sm);
    border-right: 1px solid var(--rinch-color-border);
    background: var(--pw-color-deep);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
}

.book-sidebar-title {
    padding: 4px var(--rinch-spacing-xs);
    margin-bottom: var(--rinch-spacing-xs);
    font-weight: 700;
    font-size: 16px;
    color: var(--rinch-color-text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: 'Macondo Swash Caps', cursive;
}

.book-sidebar-nav {
    flex: 1;
    padding-top: var(--rinch-spacing-xs);
}

.sidebar-section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px var(--rinch-spacing-xs);
    margin-top: var(--rinch-spacing-sm);
    cursor: pointer;
    color: var(--rinch-color-dimmed);
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    transition: color 0.15s;
}

.sidebar-section-header:hover {
    color: var(--rinch-color-text);
}

.sidebar-section-header.active {
    color: var(--rinch-color-teal-4);
}

.sidebar-chapter-list {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 4px 0;
}

.sidebar-chapter-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 8px 6px 16px;
    border-radius: var(--rinch-radius-sm);
    cursor: pointer;
    font-size: 14px;
    color: var(--rinch-color-text);
    transition: background 0.12s ease;
}

.sidebar-chapter-item:hover {
    background: var(--rinch-color-border);
}

.sidebar-chapter-item.active {
    background: var(--rinch-color-teal-9);
    color: var(--rinch-color-teal-3);
}

.sidebar-chapter-actions {
    display: flex;
    gap: 1px;
    opacity: 0;
    transition: opacity 0.15s;
}

.sidebar-chapter-item:hover .sidebar-chapter-actions {
    opacity: 1;
}

.sidebar-chapter-name {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}

.book-sidebar-footer {
    padding: var(--rinch-spacing-sm) var(--rinch-spacing-xs);
    margin-top: var(--rinch-spacing-sm);
    border-top: 1px solid var(--rinch-color-border);
    display: flex;
    flex-direction: column;
    gap: 8px;
}

.book-sidebar-footer-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
}

.book-main-pane {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    background: var(--rinch-color-body);
}

.book-main-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 40px 48px;
}

/* ── Chapters pane ──────────────────────────────────── */

.chapters-pane {
    max-width: 720px;
    margin: 0 auto;
}

.chapters-pane-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--rinch-spacing-md);
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

/* ── Mobile hamburger bar ──────────────────────────── */

.mobile-topbar {
    display: none;
    align-items: center;
    padding: 8px 16px;
    border-bottom: 1px solid var(--rinch-color-border);
    background: var(--pw-color-deep);
    flex-shrink: 0;
}

.sidebar-backdrop {
    display: none;
}

@media (max-width: 768px) {
    .book-sidebar {
        display: none;
        position: fixed;
        top: 0;
        left: 0;
        bottom: 0;
        z-index: 200;
        width: 280px;
        min-width: 280px;
    }
    .book-sidebar.open {
        display: flex;
    }
    .sidebar-backdrop {
        display: none;
        position: fixed;
        top: 0;
        left: 0;
        right: 0;
        bottom: 0;
        background: rgba(0, 0, 0, 0.5);
        z-index: 199;
    }
    .sidebar-backdrop.open {
        display: block;
    }
    .mobile-topbar {
        display: flex;
    }
    .book-main-scroll {
        padding: 24px 16px;
    }
}
"#;

/// Slot definitions for font pickers
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
        let placeholder = format!("Search fonts... (default: {})", default);
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

/// Render a single sidebar chapter item. Extracted to avoid rsx for-loop move issues.
fn sidebar_chapter_item<O, M, FO, FM>(
    __scope: &mut RenderScope,
    id: String,
    title: String,
    active_pane: Signal<BookPane>,
    open_chapter: O,
    move_chapter: M,
) -> NodeHandle
where
    O: Fn(String) -> FO + 'static + Copy,
    M: Fn(String, i32) -> FM + 'static + Copy,
    FO: Fn() + 'static,
    FM: Fn() + 'static,
{
    let cid = id.clone();
    let cid2 = id.clone();
    let cid3 = id.clone();
    rsx! {
        div {
            key: id.clone(),
            class: {
                let cid = id.clone();
                move || {
                    if active_pane.get() == BookPane::Editor(cid.clone()) {
                        "sidebar-chapter-item active"
                    } else {
                        "sidebar-chapter-item"
                    }
                }
            },

            div {
                class: "sidebar-chapter-name",
                onclick: open_chapter(cid),
                {title}
            }
            div { class: "sidebar-chapter-actions",
                ActionIcon {
                    variant: "subtle",
                    size: "xs",
                    onclick: move_chapter(cid2, -1),
                    {render_tabler_icon(__scope, TablerIcon::ChevronUp, TablerIconStyle::Outline)}
                }
                ActionIcon {
                    variant: "subtle",
                    size: "xs",
                    onclick: move_chapter(cid3, 1),
                    {render_tabler_icon(__scope, TablerIcon::ChevronDown, TablerIconStyle::Outline)}
                }
            }
        }
    }
}

/// Switch to editing a chapter. Saves current chapter if editing, clears timers, loads new chapter.
/// All parameters are Copy or Clone so this can be called from rsx! closures without ownership issues.
fn do_switch_chapter(
    active_pane: Signal<BookPane>,
    auto_save_timer_id: Signal<i32>,
    save_status: Signal<&'static str>,
    editor_loaded: Signal<bool>,
    chapter_title: Signal<String>,
    store: AppStore,
    bid: &str,
    new_chapter_id: &str,
) {
    // Clear any pending auto-save timer
    let prev = auto_save_timer_id.get();
    if prev != 0 {
        if let Some(w) = web_sys::window() {
            w.clear_timeout_with_handle(prev);
        }
        auto_save_timer_id.set(0);
    }

    // Save current chapter if we're editing
    if let BookPane::Editor(ref current_id) = active_pane.get() {
        let current_id = current_id.clone();
        let bid = bid.to_string();
        save_status.set("saving");
        wasm_bindgen_futures::spawn_local(async move {
            let content = match web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.query_selector("#editor-main").ok().flatten())
            {
                Some(el) => el.inner_html(),
                None => {
                    save_status.set("saved");
                    return;
                }
            };
            let markdown = editor_utils::html_to_markdown(&content);
            let req = UpdateChapterRequest { title: None, content: Some(markdown) };
            if api::put::<_, serde_json::Value>(
                &format!("/api/books/{}/chapters/{}", bid, current_id),
                &req,
            ).await.is_ok() {
                save_status.set("saved");
            }
        });
    }

    // Switch pane
    let new_cid = new_chapter_id.to_string();
    active_pane.set(BookPane::Editor(new_cid.clone()));
    save_status.set("saved");
    store.sidebar_open.set(false);

    // Load new chapter
    editor_loaded.set(false);
    let bid = bid.to_string();
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(chapter) = api::get::<Chapter>(
            &format!("/api/books/{}/chapters/{}", bid, new_cid),
        ).await {
            chapter_title.set(chapter.title.clone());
            editor_loaded.set(true);

            let content = chapter.content.clone();
            let window = web_sys::window().unwrap();
            let closure = wasm_bindgen::closure::Closure::once(move || {
                if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                    if let Ok(Some(el)) = doc.query_selector("#editor-main") {
                        if content.is_empty() {
                            el.set_inner_html("<p></p>");
                        } else {
                            el.set_inner_html(&editor_utils::markdown_to_html(&content));
                        }
                    }
                }
            });
            window.set_timeout_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                100,
            ).ok();
            closure.forget();
        }
    });
}

#[component]
pub fn book_page(book_id: String) -> NodeHandle {
    let store = use_store::<AppStore>();
    let show_chapter_modal = Signal::new(false);
    let new_chapter_title = Signal::new(String::new());

    let active_pane: Signal<BookPane> = Signal::new(BookPane::Chapters);
    let chapter_title = Signal::new(String::new());
    let save_status = Signal::new("saved");
    let editor_loaded = Signal::new(false);
    let auto_save_timer_id = Signal::new(0_i32);

    let bid_signal = Signal::new(book_id.clone());
    let font_settings = Signal::new(FontSettings::default());
    let font_save_timer_id = Signal::new(0_i32);
    let listeners_attached = Signal::new(false);

    // Trigger font catalog fetch
    fonts::fetch_font_catalog();

    // Fetch book and chapters
    let bid = book_id.clone();
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(book) = api::get::<Book>(&format!("/api/books/{}", bid)).await {
            let fs = book.font_settings.clone().unwrap_or_default();
            fonts::load_book_fonts(&fs);
            font_settings.set(fs);
            store.current_book.set(Some(book));
        }
        if let Ok(chapters) = api::get::<Vec<Chapter>>(&format!("/api/books/{}/chapters", bid)).await {
            store.chapters.set(chapters);
        }
    });

    // ── Save helper for the editor ──────────────────────────────
    let save_content = move |chapter_id_to_save: String| {
        let bid = bid_signal.get();
        save_status.set("saving");
        wasm_bindgen_futures::spawn_local(async move {
            let content = match web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.query_selector("#editor-main").ok().flatten())
            {
                Some(el) => el.inner_html(),
                None => {
                    save_status.set("saved");
                    return;
                }
            };

            let markdown = editor_utils::html_to_markdown(&content);
            let req = UpdateChapterRequest {
                title: None,
                content: Some(markdown),
            };
            if api::put::<_, serde_json::Value>(
                &format!("/api/books/{}/chapters/{}", bid, chapter_id_to_save),
                &req,
            ).await.is_ok() {
                save_status.set("saved");
            } else {
                save_status.set("unsaved");
            }
        });
    };

    // Chapter switching is handled by do_switch_chapter() free function

    // ── Set up Ctrl+S and auto-save (once, on mount) ────────────
    {
        let save = save_content.clone();
        let window = web_sys::window().unwrap();

        // Ctrl+S
        let save_for_key = save.clone();
        let keydown = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
            if (event.ctrl_key() || event.meta_key()) && event.key() == "s" {
                event.prevent_default();
                if let BookPane::Editor(ref cid) = active_pane.get() {
                    save_for_key(cid.clone());
                }
            }
        }) as Box<dyn FnMut(_)>);
        window.add_event_listener_with_callback("keydown", keydown.as_ref().unchecked_ref()).ok();
        keydown.forget();

        // Auto-save on input with 3s debounce
        let save_for_input = save;
        let input_closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::Event| {
            let dominated_by_editor = event
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                .and_then(|el| el.closest(".editor-content[contenteditable]").ok().flatten())
                .is_some();
            if !dominated_by_editor {
                return;
            }

            save_status.set("unsaved");
            let prev = auto_save_timer_id.get();
            if prev != 0 {
                web_sys::window().unwrap().clear_timeout_with_handle(prev);
            }
            let save_fn = save_for_input.clone();
            let closure = wasm_bindgen::closure::Closure::once(move || {
                if let BookPane::Editor(ref cid) = active_pane.get() {
                    save_fn(cid.clone());
                }
            });
            let id = web_sys::window().unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    closure.as_ref().unchecked_ref(),
                    3000,
                ).unwrap_or(0);
            closure.forget();
            auto_save_timer_id.set(id);
        }) as Box<dyn FnMut(_)>);
        let doc = window.document().unwrap();
        doc.add_event_listener_with_callback("input", input_closure.as_ref().unchecked_ref()).ok();
        input_closure.forget();
    }

    // ── Chapter actions ─────────────────────────────────────────
    let add_chapter = move || {
        let title = new_chapter_title.get();
        if title.trim().is_empty() {
            return;
        }
        show_chapter_modal.set(false);
        let bid = bid_signal.get();
        wasm_bindgen_futures::spawn_local(async move {
            let req = CreateChapterRequest { title };
            if let Ok(chapter) = api::post::<_, Chapter>(
                &format!("/api/books/{}/chapters", bid),
                &req,
            ).await {
                store.chapters.update(|ch| ch.push(chapter));
            }
        });
    };

    let move_chapter = move |chapter_id: String, direction: i32| {
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
            let bid = bid_signal.get();
            wasm_bindgen_futures::spawn_local(async move {
                let req = ReorderChaptersRequest { chapter_ids: ids };
                api::put::<_, serde_json::Value>(&format!("/api/books/{}/chapters/reorder", bid), &req).await.ok();
            });
        }
    };

    let delete_chapter = move |chapter_id: String| {
        move || {
            let cid = chapter_id.clone();
            let bid = bid_signal.get();
            wasm_bindgen_futures::spawn_local(async move {
                if api::delete_req::<serde_json::Value>(
                    &format!("/api/books/{}/chapters/{}", bid, cid)
                ).await.is_ok() {
                    if active_pane.get() == BookPane::Editor(cid.clone()) {
                        active_pane.set(BookPane::Chapters);
                    }
                    store.chapters.update(|ch| ch.retain(|c| c.id != cid));
                }
            });
        }
    };

    let go_dashboard = move || {
        store.current_route.set(Route::Dashboard);
    };

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

    let toggle_sidebar = move || {
        store.sidebar_open.update(|o| *o = !*o);
    };

    // ── Typography font picker setup ────────────────────────────
    let setup_font_pickers = move || {
        let bid_for_save = bid_signal.get();
        let closure = wasm_bindgen::closure::Closure::once(move || {
            if listeners_attached.get() {
                return;
            }
            let document = web_sys::window().unwrap().document().unwrap();

            let fs = font_settings.get();
            if let Ok(Some(grid)) = document.query_selector("#font-selector-grid") {
                grid.set_inner_html(&build_picker_grid_html(&fs));
            }

            update_preview(&fs);

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

                        let prev = font_save_timer_id.get();
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
                        font_save_timer_id.set(id);
                        update_preview(&font_settings.get());
                    }
                };

                // Input handler
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
                            if query.is_empty() { return true; }
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
                            let apply = apply_for_input.clone();
                            let click_handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |e: web_sys::Event| {
                                e.prevent_default();
                                e.stop_propagation();
                                let v = value.clone();
                                if !v.is_empty() { fonts::load_single_font(&v); }
                                apply(v);
                            }) as Box<dyn FnMut(_)>);
                            el.add_event_listener_with_callback("mousedown", click_handler.as_ref().unchecked_ref()).ok();
                            click_handler.forget();
                        }
                    }
                }) as Box<dyn FnMut(_)>);
                input_el.add_event_listener_with_callback("input", input_handler.as_ref().unchecked_ref()).ok();
                input_handler.forget();

                // Focus handler
                let input_for_focus = input_el.clone();
                let dropdown_for_focus = dropdown_ref.clone();
                let cat_for_focus = cat.clone();
                let apply_for_focus = apply_font.clone();
                let focus_handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |_event: web_sys::Event| {
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
                            if query.is_empty() { return true; }
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
                                if !v.is_empty() { fonts::load_single_font(&v); }
                                apply(v);
                            }) as Box<dyn FnMut(_)>);
                            el.add_event_listener_with_callback("mousedown", click_handler.as_ref().unchecked_ref()).ok();
                            click_handler.forget();
                        }
                    }
                }) as Box<dyn FnMut(_)>);
                input_el.add_event_listener_with_callback("focus", focus_handler.as_ref().unchecked_ref()).ok();
                focus_handler.forget();

                // Blur handler
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

    // Factory closures capture only Copy types (Signals) so they are Copy themselves.
    // This lets the rsx macro use them in multiple for-loops without move issues.
    let open_chapter = move |chapter_id: String| {
        move || do_switch_chapter(active_pane, auto_save_timer_id, save_status, editor_loaded, chapter_title, store, &bid_signal.get(), &chapter_id)
    };

    // ── Sidebar click handlers ──────────────────────────────────
    let open_chapters_pane = move || {
        active_pane.set(BookPane::Chapters);
        store.sidebar_open.set(false);
    };

    let open_typography_pane = {
        let setup = setup_font_pickers.clone();
        move || {
            active_pane.set(BookPane::Typography);
            listeners_attached.set(false);
            setup();
            store.sidebar_open.set(false);
        }
    };

    let go_back_to_chapters = move || {
        active_pane.set(BookPane::Chapters);
    };

    rsx! {
        Fragment {
            style { {TYPOGRAPHY_CSS} }
            style { {BOOK_WORKSPACE_CSS} }
            style { {editor_utils::EDITOR_CSS} }
            // Editor font styles
            style {
                {move || {
                    let fs = store.current_book.get()
                        .and_then(|b| b.font_settings.clone())
                        .unwrap_or_default();

                    let h1 = fs.h1.as_deref().unwrap_or("Macondo Swash Caps");
                    let h2 = fs.h2.as_deref().unwrap_or("Macondo Swash Caps");
                    let h3 = fs.h3.as_deref().unwrap_or("Macondo Swash Caps");
                    let body = fs.body.as_deref().unwrap_or("Andada Pro");
                    let quote = fs.quote.as_deref().unwrap_or("inherit");
                    let code = fs.code.as_deref().unwrap_or("monospace");

                    format!(
                        ".book-workspace {{ font-family: '{body}', serif; }}
                         .book-workspace h1, .book-workspace h2,
                         .book-workspace h3, .book-workspace h4,
                         .book-workspace h5, .book-workspace h6,
                         .book-workspace .rinch-title,
                         .book-workspace .book-sidebar-title {{ font-family: '{h1}', cursive; }}
                         .book-workspace .sidebar-chapter-item,
                         .book-workspace .chapter-item {{ font-family: '{body}', serif; }}
                         .editor-content {{ font-family: '{body}', serif; }}
                         .editor-content h1 {{ font-family: '{h1}', cursive; }}
                         .editor-content h2 {{ font-family: '{h2}', cursive; }}
                         .editor-content h3, .editor-content h4,
                         .editor-content h5, .editor-content h6 {{ font-family: '{h3}', cursive; }}
                         .editor-content blockquote {{ font-family: '{quote}', serif; }}
                         .editor-content code, .editor-content pre {{ font-family: '{code}', monospace; }}"
                    )
                }}
            }

            div { class: "book-workspace",
                // ── Backdrop for mobile sidebar ──
                div {
                    class: {move || if store.sidebar_open.get() { "sidebar-backdrop open" } else { "sidebar-backdrop" }},
                    onclick: move || store.sidebar_open.set(false),
                }

                // ── Sidebar ──
                div {
                    class: {move || if store.sidebar_open.get() { "book-sidebar open" } else { "book-sidebar" }},

                    // Book title
                    div { class: "book-sidebar-title",
                        {move || store.current_book.get().map(|b| b.title.clone()).unwrap_or_default()}
                    }
                    Space { h: "sm" }

                    div { class: "book-sidebar-nav",
                        // Chapters section header
                        div {
                            class: {move || if matches!(active_pane.get(), BookPane::Chapters) { "sidebar-section-header active" } else { "sidebar-section-header" }},
                            div {
                                onclick: open_chapters_pane,
                                "Chapters"
                            }
                            ActionIcon {
                                variant: "subtle",
                                size: "xs",
                                onclick: move || {
                                    new_chapter_title.set(String::new());
                                    show_chapter_modal.set(true);
                                },
                                {render_tabler_icon(__scope, TablerIcon::Plus, TablerIconStyle::Outline)}
                            }
                        }

                        // Chapter list — always visible
                        div { class: "sidebar-chapter-list",
                            for chapter in store.chapters.get() {
                                {sidebar_chapter_item(
                                    __scope,
                                    chapter.id.clone(),
                                    chapter.title.clone(),
                                    active_pane,
                                    open_chapter,
                                    move_chapter,
                                )}
                            }
                        }

                        // Typography section header
                        div {
                            class: {move || if matches!(active_pane.get(), BookPane::Typography) { "sidebar-section-header active" } else { "sidebar-section-header" }},
                            onclick: open_typography_pane,
                            "Typography"
                        }
                    }

                    // Footer
                    div { class: "book-sidebar-footer",
                        Button {
                            variant: "subtle",
                            size: "xs",
                            onclick: go_dashboard,
                            "\u{2190} Dashboard"
                        }
                        div { class: "book-sidebar-footer-row",
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
                            ActionIcon {
                                variant: "subtle",
                                size: "sm",
                                onclick: logout,
                                {render_tabler_icon(__scope, TablerIcon::Logout, TablerIconStyle::Outline)}
                            }
                        }
                    }
                }

                // ── Main Pane ──
                div { class: "book-main-pane",
                    // Mobile hamburger bar
                    div { class: "mobile-topbar",
                        ActionIcon {
                            variant: "subtle",
                            onclick: toggle_sidebar,
                            {render_tabler_icon(__scope, TablerIcon::Menu2, TablerIconStyle::Outline)}
                        }
                    }

                    // Chapters pane (CSS toggle, always in DOM)
                    div {
                        class: "book-main-scroll",
                        style: {move || if matches!(active_pane.get(), BookPane::Chapters) { "" } else { "display:none;" }},

                        div { class: "chapters-pane",
                            div { class: "chapters-pane-header",
                                Title { order: 3, "Chapters" }
                                Button {
                                    size: "sm",
                                    onclick: move || {
                                        new_chapter_title.set(String::new());
                                        show_chapter_modal.set(true);
                                    },
                                    "Add Chapter"
                                }
                            }

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
                                                onclick: open_chapter(chapter.id.clone()),

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
                                                    {render_tabler_icon(
                                                        __scope,
                                                        TablerIcon::ChevronUp,
                                                        TablerIconStyle::Outline,
                                                    )}
                                                }
                                                ActionIcon {
                                                    variant: "subtle",
                                                    size: "sm",
                                                    disabled: i == store.chapters.get().len() - 1,
                                                    onclick: move_chapter(chapter.id.clone(), 1),
                                                    {render_tabler_icon(
                                                        __scope,
                                                        TablerIcon::ChevronDown,
                                                        TablerIconStyle::Outline,
                                                    )}
                                                }
                                                ActionIcon {
                                                    variant: "subtle",
                                                    color: "red",
                                                    size: "sm",
                                                    onclick: delete_chapter(chapter.id.clone()),
                                                    {render_tabler_icon(
                                                        __scope,
                                                        TablerIcon::Trash,
                                                        TablerIconStyle::Outline,
                                                    )}
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Editor pane (CSS toggle, always in DOM — preserves undo history)
                    div {
                        class: "editor-layout",
                        style: {move || if matches!(active_pane.get(), BookPane::Editor(_)) { "" } else { "display:none;" }},

                        div { class: "editor-topbar",
                            div { class: "editor-topbar-left",
                                ActionIcon {
                                    variant: "subtle",
                                    onclick: go_back_to_chapters,
                                    {render_tabler_icon(__scope, TablerIcon::ArrowLeft, TablerIconStyle::Outline)}
                                }
                                Text { weight: "600", {|| chapter_title.get()} }
                            }
                            div {
                                class: {|| format!("save-indicator {}", save_status.get())},
                                {|| match save_status.get() {
                                    "saving" => "Saving...".to_string(),
                                    "saved" => "Saved".to_string(),
                                    _ => "Unsaved".to_string(),
                                }}
                            }
                        }

                        {editor_utils::editor_toolbar(__scope)}

                        div { class: "editor-scroll",
                            div {
                                contenteditable: "true",
                                class: "editor-content",
                                id: "editor-main",
                                p { "Start writing..." }
                            }
                        }
                    }

                    // Typography pane (CSS toggle)
                    div {
                        class: "book-main-scroll",
                        style: {move || if matches!(active_pane.get(), BookPane::Typography) { "" } else { "display:none;" }},

                        div { class: "chapters-pane",
                            Title { order: 3, "Typography" }
                            Space { h: "md" }
                            div { class: "typography-section",
                                div { class: "font-selector-grid", id: "font-selector-grid" }
                                Space { h: "md" }
                                Text { size: "sm", color: "dimmed", "Preview:" }
                                div { class: "font-preview-box", id: "font-preview" }
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
