use wasm_bindgen::JsCast;
use rinch::prelude::*;
use rinch_core::use_store;
use rinch_core::Signal;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{
    BetaBookmark, BetaChapterSummary, BetaFeedback, BetaReaderView, Book, Chapter,
    CreateBetaFeedbackRequest, CreateBetaReplyRequest, CreateBookmarkRequest,
    UpdateReadingProgressRequest,
};

use crate::api;
use crate::fonts;
use crate::pages::editor_utils;
use crate::router;
use crate::store::{AppStore, Route};

const READER_CSS: &str = r#"
.reader-workspace {
    display: flex;
    height: 100dvh;
    overflow: hidden;
}

.reader-sidebar {
    width: 260px;
    min-width: 260px;
    background: var(--pw-color-deep);
    border-right: 1px solid var(--rinch-color-border);
    display: flex;
    flex-direction: column;
    overflow: hidden;
}

.reader-sidebar-cover {
    width: 100%;
    aspect-ratio: 2 / 3;
    object-fit: cover;
    display: block;
    border-bottom: 1px solid var(--rinch-color-border);
}

.reader-sidebar-title {
    padding: 20px 16px 8px;
    font-family: 'Macondo Swash Caps', cursive;
    font-size: 18px;
    color: var(--rinch-color-text);
    border-bottom: 1px solid var(--rinch-color-border);
}

.reader-sidebar-meta {
    padding: 8px 16px;
    font-size: 12px;
    color: var(--rinch-color-dimmed);
    border-bottom: 1px solid var(--rinch-color-border);
}

.reader-sidebar-chapters {
    flex: 1;
    overflow-y: auto;
    padding: 8px 0;
}

.reader-sidebar-footer {
    padding: 8px 12px;
    border-top: 1px solid var(--rinch-color-border);
}

.reader-chapter-item {
    padding: 8px 16px;
    cursor: pointer;
    font-size: 14px;
    color: var(--rinch-color-text);
    transition: background 0.15s;
    display: flex;
    align-items: center;
    gap: 8px;
}

.reader-chapter-item:hover {
    background: var(--rinch-color-surface);
}

.reader-chapter-item.active {
    background: var(--rinch-color-surface);
    color: var(--rinch-color-teal-4);
    font-weight: 600;
}

.reader-chapter-num {
    font-size: 11px;
    color: var(--rinch-color-dimmed);
    min-width: 20px;
}

.reader-main-pane {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
}

.reader-topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 20px;
    border-bottom: 1px solid var(--rinch-color-border);
    background: var(--pw-color-deep);
    flex-shrink: 0;
}

/* Paginated reading column (always paged — no scroll). */
.reader-reading-col {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-width: 0;
}

.reader-viewport {
    flex: 1;
    overflow: hidden;
    position: relative;
    display: flex;
    justify-content: center;
    background: var(--rinch-color-body);
    touch-action: pan-y;
}

/* The fixed window that clips exactly one page. Vertical reading margins live
   here (consistent per page); horizontal margins are applied to the columns
   element in JS so every page is inset symmetrically. */
.reader-page-frame {
    width: 100%;
    max-width: 760px;
    height: 100%;
    padding: 40px 0;
    box-sizing: border-box;
    overflow: hidden;
    position: relative;
}

.reader-content {
    height: 100%;
    box-sizing: border-box;
    font-size: 16px;
    line-height: 1.8;
    color: var(--rinch-color-text);
    -webkit-font-smoothing: antialiased;
    user-select: text;
    /* Multi-column pagination: column-width / column-gap / horizontal padding
       are set imperatively once content dimensions are known. */
    column-fill: auto;
    transition: transform 0.28s ease;
    will-change: transform;
}

/* Page controls bar under the reading column. */
.reader-pagebar {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
    padding: 6px 16px;
    border-top: 1px solid var(--rinch-color-border);
    background: var(--pw-color-deep);
    flex-shrink: 0;
    font-size: 13px;
    color: var(--rinch-color-dimmed);
    user-select: none;
}

.reader-pagebar-indicator {
    min-width: 70px;
    text-align: center;
}

/* Bookmarks list in the sidebar. */
.reader-sidebar-bookmarks {
    border-top: 1px solid var(--rinch-color-border);
    padding: 8px 0;
    max-height: 30%;
    overflow-y: auto;
    flex-shrink: 0;
}

.reader-bookmarks-title {
    padding: 4px 16px 6px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--rinch-color-dimmed);
}

.reader-bookmark-item {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px 4px 16px;
    font-size: 13px;
}

.reader-bookmark-item:hover {
    background: var(--rinch-color-surface);
}

.reader-bookmark-label {
    flex: 1;
    cursor: pointer;
    color: var(--rinch-color-teal-4);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.reader-content p { margin: 0 0 8px 0; }
.reader-content h1 { font-size: 2em; font-weight: 700; margin: 32px 0 12px 0; }
.reader-content h2 { font-size: 1.5em; font-weight: 700; margin: 28px 0 10px 0; }
.reader-content h3 { font-size: 1.25em; font-weight: 600; margin: 24px 0 8px 0; }
.reader-content blockquote {
    border-left: 3px solid var(--rinch-color-teal-8);
    padding-left: 16px;
    margin: 16px 0;
    color: var(--rinch-color-dimmed);
}
.reader-content img {
    max-width: 100%;
    height: auto;
    border-radius: var(--rinch-radius-sm);
    margin: 16px 0;
    display: block;
}
.reader-content strong { font-weight: 700; }
.reader-content em { font-style: italic; }

/* Feedback tooltip */
.feedback-tooltip {
    position: fixed;
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-teal-6);
    border-radius: 6px;
    padding: 8px 12px;
    box-shadow: 0 4px 12px rgba(0,0,0,0.3);
    z-index: 1000;
    display: none;
}

.feedback-tooltip.visible {
    display: block;
}

.feedback-tooltip textarea {
    width: 280px;
    min-height: 60px;
    padding: 8px;
    border: 1px solid var(--rinch-color-border);
    border-radius: 4px;
    background: var(--rinch-color-body);
    color: var(--rinch-color-text);
    font-size: 13px;
    font-family: inherit;
    resize: vertical;
    outline: none;
}

.feedback-tooltip textarea:focus {
    border-color: var(--rinch-color-teal-6);
}

.feedback-tooltip-actions {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    margin-top: 6px;
}

/* Feedback sidebar */
.reader-feedback-panel {
    width: 320px;
    min-width: 320px;
    background: var(--pw-color-deep);
    border-left: 1px solid var(--rinch-color-border);
    display: flex;
    flex-direction: column;
    overflow: hidden;
}

.reader-feedback-panel.hidden {
    display: none;
}

.reader-feedback-header {
    padding: 12px 16px;
    border-bottom: 1px solid var(--rinch-color-border);
    font-weight: 600;
    font-size: 14px;
    display: flex;
    align-items: center;
    justify-content: space-between;
}

.reader-feedback-list {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
}

.feedback-card {
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
    border-radius: 6px;
    padding: 10px 12px;
    margin-bottom: 8px;
    font-size: 13px;
}

.feedback-card.resolved {
    opacity: 0.5;
}

.feedback-quote {
    font-style: italic;
    color: var(--rinch-color-teal-4);
    font-size: 12px;
    padding: 4px 8px;
    border-left: 2px solid var(--rinch-color-teal-7);
    margin-bottom: 6px;
    word-break: break-word;
}

.feedback-comment {
    color: var(--rinch-color-text);
    margin-bottom: 4px;
    word-break: break-word;
}

.feedback-meta {
    font-size: 11px;
    color: var(--rinch-color-dimmed);
}

.feedback-replies {
    margin-top: 8px;
    padding-top: 6px;
    border-top: 1px solid var(--rinch-color-border);
}

.feedback-reply {
    padding: 4px 0;
    font-size: 12px;
}

.feedback-reply-author {
    font-weight: 600;
    color: var(--rinch-color-teal-4);
}

.feedback-reply-author.owner {
    color: var(--rinch-color-teal-3);
}

.feedback-reply-input {
    display: flex;
    gap: 4px;
    margin-top: 4px;
}

.feedback-reply-input textarea {
    flex: 1;
    padding: 4px 8px;
    border: 1px solid var(--rinch-color-border);
    border-radius: 4px;
    background: var(--rinch-color-body);
    color: var(--rinch-color-text);
    font-size: 12px;
    font-family: inherit;
    outline: none;
    resize: none;
    min-height: 28px;
    max-height: 120px;
    overflow-y: auto;
}

/* Welcome screen */
.reader-welcome {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    text-align: center;
    padding: 40px;
}

.reader-welcome h2 {
    font-family: 'Macondo Swash Caps', cursive;
    color: var(--rinch-color-teal-4);
    margin-bottom: 8px;
}

/* Highlighted text in reader */
.reader-content mark.beta-highlight {
    background: var(--rinch-color-teal-9);
    color: var(--rinch-color-teal-2);
    padding: 1px 2px;
    border-radius: 2px;
    cursor: pointer;
}

/* Error state */
.reader-error {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100dvh;
    text-align: center;
    padding: 40px;
}

.reader-error h2 {
    font-family: 'Macondo Swash Caps', cursive;
    color: var(--rinch-color-teal-4);
    margin-bottom: 8px;
}

/* Mobile topbar */
.reader-mobile-topbar {
    display: none;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid var(--rinch-color-border);
    background: var(--pw-color-deep);
    flex-shrink: 0;
}

.reader-sidebar-backdrop { display: none; }
.reader-feedback-backdrop { display: none; }

@media (max-width: 768px) {
    .reader-mobile-topbar { display: flex; }
    .reader-topbar { display: none; }
    .reader-page-frame { padding: 20px 0; }

    /* Sidebar drawer */
    .reader-sidebar {
        display: none;
        position: fixed; top: 0; left: 0; bottom: 0;
        z-index: 200; width: 280px; min-width: 280px;
    }
    .reader-sidebar.open { display: flex; }
    .reader-sidebar-backdrop.open {
        display: block;
        position: fixed; top: 0; left: 0; right: 0; bottom: 0;
        background: rgba(0,0,0,0.5); z-index: 199;
    }

    /* Feedback bottom sheet */
    .reader-feedback-panel {
        display: none !important;
        position: fixed; bottom: 0; left: 0; right: 0;
        height: 60vh; z-index: 200;
        width: 100%; min-width: 100%;
        border-radius: 12px 12px 0 0;
        border-top: 1px solid var(--rinch-color-border);
        border-left: none;
    }
    .reader-feedback-panel.mobile-open { display: flex !important; }
    .reader-feedback-backdrop.open {
        display: block;
        position: fixed; top: 0; left: 0; right: 0; bottom: 0;
        background: rgba(0,0,0,0.3); z-index: 199;
    }

    /* Tooltip as bottom sheet on mobile */
    .feedback-tooltip.visible {
        left: 0 !important; right: 0 !important;
        top: auto !important; bottom: 0 !important;
        width: 100%; border-radius: 12px 12px 0 0;
        padding: 16px;
    }
    .feedback-tooltip textarea { width: 100%; }
}
"#;

/// Where the reader gets its data. Beta readers hit the token-scoped public
/// endpoints (with feedback + progress persistence); author preview reads the
/// authenticated book/chapter endpoints and never writes anything back.
#[derive(Clone)]
enum ReaderSource {
    Beta(String),          // beta link token
    AuthorPreview(String), // book_id
}

/// Horizontal reading margin (px) for the paginated column, by viewport width.
fn side_pad(vw: f64) -> f64 {
    if vw < 640.0 { 18.0 } else { 48.0 }
}

fn reader_content_el() -> Option<web_sys::HtmlElement> {
    web_sys::window()?
        .document()?
        .query_selector("#reader-content")
        .ok()
        .flatten()?
        .dyn_into()
        .ok()
}

/// Apply the multi-column styling to `#reader-content` (sized to the live
/// viewport) and return the resulting total page count. Reading `scroll_width`
/// forces the synchronous reflow we need before counting pages.
fn measure_and_style() -> i32 {
    let Some(el) = reader_content_el() else { return 1 };
    let vw = el.client_width() as f64;
    if vw <= 1.0 {
        return 1;
    }
    let pad = side_pad(vw);
    let col_w = (vw - 2.0 * pad).max(1.0);
    let style = el.style();
    let _ = style.set_property("column-width", &format!("{}px", col_w));
    let _ = style.set_property("column-gap", &format!("{}px", pad));
    let _ = style.set_property("padding-left", &format!("{}px", pad));
    let _ = style.set_property("padding-right", &format!("{}px", pad));
    let stride = vw - pad; // = col_w + gap
    let sw = el.scroll_width() as f64;
    // scroll_width == pad + n * stride, so (sw - pad) / stride == n.
    (((sw - pad) / stride) - 0.01).ceil().max(1.0) as i32
}

/// Translate the columns element so `page` is the visible page.
fn apply_page_transform(page: i32) {
    if let Some(el) = reader_content_el() {
        let vw = el.client_width() as f64;
        let pad = side_pad(vw);
        let stride = (vw - pad).max(0.0);
        let _ = el
            .style()
            .set_property("transform", &format!("translateX(-{}px)", (page as f64) * stride));
    }
}

/// True if the current document selection is empty (used so a text-selection
/// drag for feedback isn't mistaken for a page swipe).
fn selection_is_empty() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_selection().ok().flatten())
        .map(|s| s.to_string().as_string().unwrap_or_default().trim().is_empty())
        .unwrap_or(true)
}

/// The id of the chapter `delta` positions from the active one in reading order
/// (`-1` = previous, `+1` = next), or `None` at the ends. Lets paging past a
/// chapter boundary flip into the adjacent chapter.
fn adjacent_chapter_id(
    view_data: Signal<Option<BetaReaderView>>,
    active_chapter_id: Signal<Option<String>>,
    delta: i32,
) -> Option<String> {
    let view = view_data.get()?;
    let current = active_chapter_id.get()?;
    let idx = view.chapters.iter().position(|c| c.id == current)?;
    let target = idx as i32 + delta;
    if target < 0 || target as usize >= view.chapters.len() {
        return None;
    }
    Some(view.chapters[target as usize].id.clone())
}

fn reader_bookmark_item<F, D, DO>(
    __scope: &mut RenderScope,
    bm: BetaBookmark,
    open: F,
    del: D,
) -> NodeHandle
where
    F: Fn(String, i32) + 'static + Copy,
    D: Fn(String) -> DO + 'static + Copy,
    DO: Fn() + 'static,
{
    let cid = bm.chapter_id.clone();
    let page = bm.page as i32;
    let label = bm.label.clone();
    let del_id = bm.id.clone();
    rsx! {
        div { class: "reader-bookmark-item", key: bm.id,
            div {
                class: "reader-bookmark-label",
                onclick: move || open(cid.clone(), page),
                {label}
            }
            ActionIcon {
                variant: "subtle",
                size: "xs",
                color: "red",
                onclick: del(del_id),
                {render_tabler_icon(__scope, TablerIcon::Trash, TablerIconStyle::Outline)}
            }
        }
    }
}

fn reader_chapter_item<F, FO>(
    __scope: &mut RenderScope,
    ch_id: String,
    ch_title: String,
    ch_sort: i64,
    active_chapter_id: Signal<Option<String>>,
    load_chapter: F,
) -> NodeHandle
where
    F: Fn(String) -> FO + 'static + Copy,
    FO: Fn() + 'static,
{
    let cid = std::rc::Rc::new(ch_id.clone());
    rsx! {
        div {
            class: {
                let cid = cid.clone();
                move || if active_chapter_id.get().as_deref() == Some(cid.as_str()) {
                    "reader-chapter-item active"
                } else {
                    "reader-chapter-item"
                }
            },
            onclick: load_chapter(ch_id),
            span { class: "reader-chapter-num",
                {format!("{}.", ch_sort + 1)}
            }
            {ch_title}
        }
    }
}

fn reader_feedback_card<F, FO>(
    __scope: &mut RenderScope,
    fb: BetaFeedback,
    reply_to_feedback: F,
) -> NodeHandle
where
    F: Fn(String) -> FO + 'static + Copy,
    FO: Fn() + 'static,
{
    let _fb_id = fb.id.clone();
    let fb_id2 = fb.id.clone();
    let fb_id3 = fb.id.clone();
    let fb_id_enter = fb.id.clone();
    let class = if fb.resolved { "feedback-card resolved" } else { "feedback-card" };
    let fb_comment = fb.comment.clone();
    let fb_created = fb.created_at.clone();
    let quote_style = if fb.selected_text.is_empty() { "display:none" } else { "" };
    let quote_text = if !fb.selected_text.is_empty() {
        let t = if fb.selected_text.chars().count() > 100 {
            let truncated: String = fb.selected_text.chars().take(100).collect();
            format!("{}...", truncated)
        } else {
            fb.selected_text.clone()
        };
        format!("\u{201c}{}\u{201d}", t)
    } else {
        String::new()
    };
    let reply_nodes: Vec<NodeHandle> = fb.replies.iter().map(|r| {
        reply_item(__scope, r.author_type.clone(), r.author_name.clone(), r.content.clone())
    }).collect();

    let reply_submit_id = __scope.register_handler(reply_to_feedback(fb_id_enter));
    let reply_box = rsx! {
        div { class: "feedback-reply-input",
            textarea {
                id: {format!("reply-input-{}", fb_id2)},
                placeholder: "Reply...",
                rows: "1",
            }
            ActionIcon {
                variant: "subtle",
                size: "xs",
                onclick: reply_to_feedback(fb_id3),
                {render_tabler_icon(__scope, TablerIcon::Send, TablerIconStyle::Outline)}
            }
        }
    };
    reply_box.set_attribute("data-onsubmit", &reply_submit_id.0.to_string());

    rsx! {
        div {
            class: class,
            key: _fb_id,

            div { class: "feedback-quote", style: quote_style, {quote_text} }
            div { class: "feedback-comment", {fb_comment} }
            div { class: "feedback-meta", {fb_created} }

            div { class: "feedback-replies",
                {reply_nodes}
            }

            {reply_box}
        }
    }
}

fn reply_item(
    __scope: &mut RenderScope,
    author_type: String,
    author_name: String,
    content: String,
) -> NodeHandle {
    let class_str = if author_type == "owner" { "feedback-reply-author owner" } else { "feedback-reply-author" };
    rsx! {
        div { class: "feedback-reply",
            span { class: class_str, {format!("{}: ", author_name)} }
            {content}
        }
    }
}

#[component]
pub fn reader_page(token: String) -> NodeHandle {
    reader_body(__scope, ReaderSource::Beta(token))
}

#[component]
pub fn reader_preview_page(book_id: String) -> NodeHandle {
    reader_body(__scope, ReaderSource::AuthorPreview(book_id))
}

/// Shared reader implementation for both beta readers and author preview.
fn reader_body(__scope: &mut RenderScope, source: ReaderSource) -> NodeHandle {
    let store = use_store::<AppStore>();

    let is_preview = matches!(source, ReaderSource::AuthorPreview(_));
    let (token, book_id) = match &source {
        ReaderSource::Beta(t) => (t.clone(), String::new()),
        ReaderSource::AuthorPreview(b) => (String::new(), b.clone()),
    };
    let token_signal = Signal::new(token.clone());
    let book_id_signal = Signal::new(book_id.clone());

    let view_data: Signal<Option<BetaReaderView>> = Signal::new(None);
    let current_chapter: Signal<Option<Chapter>> = Signal::new(None);
    let active_chapter_id: Signal<Option<String>> = Signal::new(None);
    let feedback_list: Signal<Vec<BetaFeedback>> = Signal::new(Vec::new());
    let bookmarks: Signal<Vec<BetaBookmark>> = Signal::new(Vec::new());
    let show_feedback_panel: Signal<bool> = Signal::new(true);
    let sidebar_open: Signal<bool> = Signal::new(false);
    let mobile_feedback_open: Signal<bool> = Signal::new(false);
    let error_msg: Signal<Option<String>> = Signal::new(None);

    // Pagination state
    let current_page: Signal<i32> = Signal::new(0);
    let total_pages: Signal<i32> = Signal::new(1);
    // Debounce handle for progress saves (a window timeout id, or None).
    let progress_timer: Signal<Option<i32>> = Signal::new(None);

    // Tooltip state
    let tooltip_visible: Signal<bool> = Signal::new(false);
    let tooltip_x: Signal<i32> = Signal::new(0);
    let tooltip_y: Signal<i32> = Signal::new(0);
    let tooltip_selected_text: Signal<String> = Signal::new(String::new());
    let tooltip_context: Signal<String> = Signal::new(String::new());
    let tooltip_comment: Signal<String> = Signal::new(String::new());

    // ── Auto last-page: persist reading position, debounced (beta only) ──────
    let save_progress = move |chapter_id: String, page: i32| {
        if is_preview {
            return;
        }
        let window = match web_sys::window() {
            Some(w) => w,
            None => return,
        };
        if let Some(h) = progress_timer.get() {
            window.clear_timeout_with_handle(h);
        }
        let tok = token_signal.get();
        let closure = wasm_bindgen::closure::Closure::once(move || {
            let req = UpdateReadingProgressRequest { chapter_id, page: page as i64 };
            api::put::<_, serde_json::Value>(
                &format!("/api/beta/{}/progress", tok),
                &req,
                move |_result| {},
            );
        });
        let handle = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                500,
            )
            .unwrap_or(-1);
        closure.forget();
        progress_timer.set(Some(handle));
    };

    // ── Pagination: (re)measure the column layout and place a page ───────────
    // Deferred to a rAF because inner_html fills / reflows asynchronously.
    let repaginate = move |target_page: i32| {
        let closure = wasm_bindgen::closure::Closure::once(move || {
            let total = measure_and_style();
            total_pages.set(total);
            let clamped = target_page.max(0).min((total - 1).max(0));
            current_page.set(clamped);
            apply_page_transform(clamped);
            if let Some(cid) = active_chapter_id.get() {
                save_progress(cid, clamped);
            }
        });
        if let Some(w) = web_sys::window() {
            w.request_animation_frame(closure.as_ref().unchecked_ref()).ok();
        }
        closure.forget();
    };

    // ── Open a chapter, optionally resuming at `target_page` ─────────────────
    let open_chapter = move |chapter_id: String, target_page: i32| {
        active_chapter_id.set(Some(chapter_id.clone()));
        tooltip_visible.set(false);
        sidebar_open.set(false);
        current_page.set(0);
        total_pages.set(1);
        let cid = chapter_id.clone();
        let url = if is_preview {
            format!("/api/books/{}/chapters/{}", book_id_signal.get(), cid)
        } else {
            format!("/api/beta/{}/chapters/{}", token_signal.get(), cid)
        };
        api::get::<Chapter>(&url, move |result| {
            if let Ok(ch) = result {
                // Guard: a newer chapter may have been selected while this
                // request was in flight. Don't inject stale content.
                if active_chapter_id.get().as_deref() != Some(cid.as_str()) {
                    return;
                }
                current_chapter.set(Some(ch.clone()));
                let content_html = if ch.content.is_empty() {
                    "<p><em>This chapter is empty.</em></p>".to_string()
                } else {
                    editor_utils::content_to_display_html(&ch.content)
                };
                let guard_cid = cid.clone();
                editor_utils::with_element_when_ready("#reader-content".to_string(), move |el| {
                    if active_chapter_id.get().as_deref() != Some(guard_cid.as_str()) {
                        return;
                    }
                    el.set_inner_html(&editor_utils::sanitize_html(&content_html));
                    repaginate(target_page);
                    // A second pass catches late reflow (e.g. images loading).
                    let closure = wasm_bindgen::closure::Closure::once(move || {
                        repaginate(current_page.get());
                    });
                    if let Some(w) = web_sys::window() {
                        w.set_timeout_with_callback_and_timeout_and_arguments_0(
                            closure.as_ref().unchecked_ref(),
                            250,
                        ).ok();
                    }
                    closure.forget();
                });
            }
        });
    };

    // ── Turn to a page; paging off either end flips to the adjacent chapter ───
    let go_to_page = move |page: i32| {
        let total = total_pages.get();
        // Past the last page → open the next chapter at its first page.
        if page > total - 1 {
            if let Some(next_id) = adjacent_chapter_id(view_data, active_chapter_id, 1) {
                open_chapter(next_id, 0);
            }
            return;
        }
        // Before the first page → open the previous chapter at its last page
        // (a large target page is clamped to the last by repaginate).
        if page < 0 {
            if let Some(prev_id) = adjacent_chapter_id(view_data, active_chapter_id, -1) {
                open_chapter(prev_id, i32::MAX);
            }
            return;
        }
        let clamped = page.max(0).min((total - 1).max(0));
        if clamped == current_page.get() {
            return;
        }
        current_page.set(clamped);
        apply_page_transform(clamped);
        if let Some(cid) = active_chapter_id.get() {
            save_progress(cid, clamped);
        }
    };

    // ── Bookmarks (beta only) ────────────────────────────────────────────────
    let add_bookmark = move || {
        if is_preview {
            return;
        }
        let (Some(cid), Some(ch)) = (active_chapter_id.get(), current_chapter.get()) else {
            return;
        };
        let page = current_page.get();
        let label = format!("Ch. {} \u{b7} p.{}", ch.title, page + 1);
        let tok = token_signal.get();
        let req = CreateBookmarkRequest { chapter_id: cid, page: page as i64, label };
        api::post::<_, BetaBookmark>(
            &format!("/api/beta/{}/bookmarks", tok),
            &req,
            move |result| {
                if let Ok(bm) = result {
                    bookmarks.update(|list| list.push(bm));
                }
            },
        );
    };

    let delete_bookmark = move |id: String| {
        move || {
            if is_preview {
                return;
            }
            let tok = token_signal.get();
            let bid = id.clone();
            api::delete_req::<serde_json::Value>(
                &format!("/api/beta/{}/bookmarks/{}", tok, bid),
                move |result| {
                    if result.is_ok() {
                        bookmarks.update(|list| list.retain(|b| b.id != bid));
                    }
                },
            );
        }
    };

    // ── Load book view (branches on source) ──────────────────────────────────
    match source.clone() {
        ReaderSource::Beta(tok) => {
            let tok2 = tok.clone();
            api::get::<BetaReaderView>(&format!("/api/beta/{}", tok2), move |result| {
                match result {
                    Ok(data) => {
                        if let Some(ref fs) = data.font_settings {
                            fonts::load_book_fonts(fs);
                        }
                        bookmarks.set(data.bookmarks.clone());
                        let resume = data.last_chapter_id.clone();
                        let resume_page = data.last_page as i32;
                        view_data.set(Some(data));
                        // Resume where the reader left off, if permitted.
                        if let Some(cid) = resume {
                            open_chapter(cid, resume_page);
                        }
                    }
                    Err(e) => {
                        error_msg.set(Some(e.message));
                    }
                }
            });

            // Fetch feedback
            let tokf = tok.clone();
            api::get::<Vec<BetaFeedback>>(&format!("/api/beta/{}/feedback", tokf), move |result| {
                if let Ok(fb) = result {
                    feedback_list.set(fb);
                }
            });

            // Check session first, then auto-claim if logged in.
            // Auth check must come first — the claim endpoint's Session extractor
            // could create a new empty session that overwrites the valid cookie.
            {
                let tokc = tok.clone();
                let store = use_store::<AppStore>();
                if store.current_user.get().is_none() {
                    api::get::<plotweb_common::User>("/api/auth/me", move |result| {
                        if let Ok(user) = result {
                            store.current_user.set(Some(user));
                        }
                        if store.current_user.get().is_some() {
                            api::post::<_, serde_json::Value>(&format!("/api/beta/{}/claim", tokc), &serde_json::json!({}), move |_result| {});
                        }
                    });
                } else if store.current_user.get().is_some() {
                    api::post::<_, serde_json::Value>(&format!("/api/beta/{}/claim", tokc), &serde_json::json!({}), move |_result| {});
                }
            }

            // Connect WebSocket for real-time feedback
            {
                let ws_url = crate::ws::ws_url(&format!("/api/beta/{}/feedback/ws", tok));
                crate::ws::connect_feedback_ws(&ws_url, move |msg| {
                    match msg {
                        crate::ws::WsMessage::NewFeedback(fb) => {
                            feedback_list.update(|list| {
                                if !list.iter().any(|f| f.id == fb.id) {
                                    list.insert(0, fb);
                                }
                            });
                        }
                        crate::ws::WsMessage::NewReply { feedback_id, reply } => {
                            feedback_list.update(|list| {
                                if let Some(fb) = list.iter_mut().find(|f| f.id == feedback_id) {
                                    if !fb.replies.iter().any(|r| r.id == reply.id) {
                                        fb.replies.push(reply);
                                    }
                                }
                            });
                        }
                        crate::ws::WsMessage::FeedbackResolved { feedback_id, resolved } => {
                            feedback_list.update(|list| {
                                if let Some(fb) = list.iter_mut().find(|f| f.id == feedback_id) {
                                    fb.resolved = resolved;
                                }
                            });
                        }
                        crate::ws::WsMessage::FeedbackDeleted { feedback_id } => {
                            feedback_list.update(|list| list.retain(|f| f.id != feedback_id));
                        }
                    }
                });
            }
        }
        ReaderSource::AuthorPreview(bid) => {
            // Build an equivalent in-memory view from the authenticated author
            // endpoints. No feedback / progress / bookmark writes in this mode.
            let bid_ch = bid.clone();
            api::get::<Book>(&format!("/api/books/{}", bid), move |book| {
                api::get::<Vec<Chapter>>(&format!("/api/books/{}/chapters", bid_ch), move |chapters| {
                match (book, chapters) {
                    (Ok(book), Ok(chs)) => {
                        if let Some(fs) = &book.font_settings {
                            fonts::load_book_fonts(fs);
                        }
                        let mut summaries: Vec<BetaChapterSummary> = chs
                            .iter()
                            .map(|c| BetaChapterSummary {
                                id: c.id.clone(),
                                title: c.title.clone(),
                                sort_order: c.sort_order,
                            })
                            .collect();
                        summaries.sort_by_key(|s| s.sort_order);
                        let view = BetaReaderView {
                            book_title: book.title.clone(),
                            book_description: book.description.clone(),
                            reader_name: "Preview".to_string(),
                            chapters: summaries,
                            font_settings: book.font_settings.clone(),
                            cover_image: book.cover_image.clone(),
                            last_chapter_id: None,
                            last_page: 0,
                            bookmarks: Vec::new(),
                        };
                        view_data.set(Some(view));
                    }
                    (Err(e), _) | (_, Err(e)) => {
                        error_msg.set(Some(e.message));
                    }
                }
                });
            });
        }
    }

    // Sidebar chapter click → open at page 0.
    let load_chapter = move |chapter_id: String| {
        move || {
            open_chapter(chapter_id.clone(), 0);
        }
    };

    // Feedback text-selection is beta-only (no feedback panel in preview).
    if !is_preview {
    // Shared selection handler for both mouse and touch
    let handle_selection = std::rc::Rc::new(move |client_x: i32, client_y: i32| {
        let doc = web_sys::window().unwrap().document().unwrap();
        let sel = match doc.get_selection().ok().flatten() {
            Some(s) => s,
            None => return,
        };

        let text = sel.to_string().as_string().unwrap_or_default();
        if text.trim().is_empty() {
            tooltip_visible.set(false);
            return;
        }

        // Get context block (parent paragraph text)
        let context = if sel.range_count() > 0 {
            if let Ok(range) = sel.get_range_at(0) {
                let container = range.common_ancestor_container().ok();
                let mut node = container;
                let mut block_text = String::new();
                // Walk up to find block element
                while let Some(n) = node {
                    if let Ok(el) = n.clone().dyn_into::<web_sys::Element>() {
                        let tag = el.tag_name().to_lowercase();
                        if matches!(tag.as_str(), "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "li") {
                            block_text = el.text_content().unwrap_or_default();
                            break;
                        }
                    }
                    node = n.parent_node();
                }
                if block_text.len() > 200 {
                    block_text.truncate(200);
                }
                block_text
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        tooltip_selected_text.set(text);
        tooltip_context.set(context);
        tooltip_comment.set(String::new());
        tooltip_x.set(client_x);
        tooltip_y.set(client_y);
        tooltip_visible.set(true);
    });

    // Set up text selection listener for feedback tooltip (mouse)
    {
        let window = web_sys::window().unwrap();
        let handle = handle_selection.clone();
        let mouseup_closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::MouseEvent| {
            let target = match event.target() {
                Some(t) => t,
                None => return,
            };
            let el: web_sys::Element = match target.dyn_into() {
                Ok(e) => e,
                Err(_) => return,
            };
            if el.closest("#reader-content").ok().flatten().is_none() {
                return;
            }
            handle(event.client_x(), event.client_y() - 10);
        }) as Box<dyn FnMut(_)>);
        window.document().unwrap()
            .add_event_listener_with_callback("mouseup", mouseup_closure.as_ref().unchecked_ref())
            .ok();
        mouseup_closure.forget();
    }

    // Set up text selection listener for feedback tooltip (touch)
    {
        let window = web_sys::window().unwrap();
        let handle = handle_selection.clone();
        let touchend_closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::TouchEvent| {
            let target: web_sys::EventTarget = match event.target() {
                Some(t) => t,
                None => return,
            };
            let el: web_sys::Element = match target.dyn_into() {
                Ok(e) => e,
                Err(_) => return,
            };
            if el.closest("#reader-content").ok().flatten().is_none() {
                return;
            }
            // Delay to let mobile browser finalize selection
            let handle = handle.clone();
            let closure = wasm_bindgen::closure::Closure::once(move || {
                handle(0, 0);
            });
            web_sys::window().unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    closure.as_ref().unchecked_ref(),
                    100,
                ).ok();
            closure.forget();
        }) as Box<dyn FnMut(_)>);
        window.document().unwrap()
            .add_event_listener_with_callback("touchend", touchend_closure.as_ref().unchecked_ref())
            .ok();
        touchend_closure.forget();
    }
    } // end if !is_preview (feedback selection)

    // ── Keyboard paging (Arrow left/right), ignoring text inputs ─────────────
    {
        let keydown_closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
            if let Some(t) = event.target() {
                if let Ok(el) = t.dyn_into::<web_sys::Element>() {
                    let tag = el.tag_name().to_lowercase();
                    if tag == "textarea" || tag == "input" {
                        return;
                    }
                }
            }
            match event.key().as_str() {
                "ArrowLeft" => go_to_page(current_page.get() - 1),
                "ArrowRight" => go_to_page(current_page.get() + 1),
                _ => {}
            }
        }) as Box<dyn FnMut(_)>);
        web_sys::window().unwrap().document().unwrap()
            .add_event_listener_with_callback("keydown", keydown_closure.as_ref().unchecked_ref())
            .ok();
        keydown_closure.forget();
    }

    // ── Touch swipe paging (skipped while text is selected for feedback) ─────
    {
        let swipe_start = std::rc::Rc::new(std::cell::Cell::new(0.0f64));
        {
            let s = swipe_start.clone();
            let touchstart_closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::TouchEvent| {
                if let Some(t) = event.touches().get(0) {
                    s.set(t.client_x() as f64);
                }
            }) as Box<dyn FnMut(_)>);
            web_sys::window().unwrap().document().unwrap()
                .add_event_listener_with_callback("touchstart", touchstart_closure.as_ref().unchecked_ref())
                .ok();
            touchstart_closure.forget();
        }
        {
            let s = swipe_start.clone();
            let touchend_closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::TouchEvent| {
                let within = event
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .and_then(|el| el.closest("#reader-viewport").ok().flatten())
                    .is_some();
                if !within || !selection_is_empty() {
                    return;
                }
                let end_x = event.changed_touches().get(0).map(|t| t.client_x() as f64).unwrap_or(s.get());
                let dx = end_x - s.get();
                if dx.abs() > 45.0 {
                    if dx < 0.0 {
                        go_to_page(current_page.get() + 1);
                    } else {
                        go_to_page(current_page.get() - 1);
                    }
                }
            }) as Box<dyn FnMut(_)>);
            web_sys::window().unwrap().document().unwrap()
                .add_event_listener_with_callback("touchend", touchend_closure.as_ref().unchecked_ref())
                .ok();
            touchend_closure.forget();
        }
    }

    // ── Re-flow pages on window resize ───────────────────────────────────────
    {
        let resize_closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |_event: web_sys::Event| {
            repaginate(current_page.get());
        }) as Box<dyn FnMut(_)>);
        web_sys::window().unwrap()
            .add_event_listener_with_callback("resize", resize_closure.as_ref().unchecked_ref())
            .ok();
        resize_closure.forget();
    }

    // Submit feedback
    let submit_feedback = move || {
        let comment = tooltip_comment.get();
        if comment.trim().is_empty() {
            return;
        }
        let chapter_id = match active_chapter_id.get() {
            Some(id) => id,
            None => return,
        };
        let selected_text = tooltip_selected_text.get();
        let context_block = tooltip_context.get();
        let tok = token_signal.get();

        tooltip_visible.set(false);

        let req = CreateBetaFeedbackRequest {
            chapter_id,
            selected_text,
            context_block,
            comment,
        };
        let tok_refresh = tok.clone();
        api::post::<_, serde_json::Value>(&format!("/api/beta/{}/feedback", tok), &req, move |result| {
            if result.is_ok() {
                // Refresh feedback list
                api::get::<Vec<BetaFeedback>>(&format!("/api/beta/{}/feedback", tok_refresh), move |fb_result| {
                    if let Ok(fb) = fb_result {
                        feedback_list.set(fb);
                    }
                });
            }
        });
    };

    // Reply to feedback
    let reply_to_feedback = move |feedback_id: String| {
        move || {
            let doc = web_sys::window().unwrap().document().unwrap();
            let selector = format!("#reply-input-{}", feedback_id);
            let input: web_sys::HtmlTextAreaElement = match doc.query_selector(&selector).ok().flatten() {
                Some(el) => el.dyn_into().unwrap(),
                None => return,
            };
            let content = input.value();
            if content.trim().is_empty() {
                return;
            }
            input.set_value("");
            let tok = token_signal.get();
            let fid = feedback_id.clone();
            let req = CreateBetaReplyRequest { content };
            let tok_refresh = tok.clone();
            api::post::<_, serde_json::Value>(
                &format!("/api/beta/{}/feedback/{}/replies", tok, fid),
                &req,
                move |result| {
                    if result.is_ok() {
                        // Refresh
                        api::get::<Vec<BetaFeedback>>(&format!("/api/beta/{}/feedback", tok_refresh), move |fb_result| {
                            if let Ok(fb) = fb_result {
                                feedback_list.set(fb);
                            }
                        });
                    }
                },
            );
        }
    };

    let toggle_feedback = move || {
        show_feedback_panel.update(|v| *v = !*v);
        // Toggling the side panel changes the reading measure → re-flow pages.
        repaginate(current_page.get());
    };

    rsx! {
        Fragment {
            style { {READER_CSS} }
            style { {editor_utils::EDITOR_CSS} }

            // Font styles from book settings
            style {
                {move || {
                    let data = view_data.get();
                    let fs = data.as_ref()
                        .and_then(|d| d.font_settings.as_ref())
                        .cloned()
                        .unwrap_or_default();

                    let h1 = fs.h1.as_deref().unwrap_or("Macondo Swash Caps");
                    let body = fs.body.as_deref().unwrap_or("Playwrite DE Grund");
                    let quote = fs.quote.as_deref().unwrap_or("inherit");
                    let p_spacing = fs.paragraph_spacing.unwrap_or(8.0);
                    let p_indent = fs.paragraph_indent.unwrap_or(0.0);

                    format!(
                        ".reader-workspace {{ --rinch-font-family: '{body}', serif; font-family: '{body}', serif; }}
                         .reader-sidebar-title {{ font-family: '{h1}', cursive; }}
                         .reader-topbar {{ font-family: '{body}', serif; }}
                         .reader-topbar > div:first-child .rinch-text {{ font-family: '{h1}', cursive; }}
                         .reader-mobile-topbar {{ font-family: '{body}', serif; }}
                         .reader-mobile-topbar > .rinch-text {{ font-family: '{h1}', cursive; }}
                         .feedback-tooltip {{ font-family: '{body}', serif; }}
                         .feedback-tooltip .rinch-button {{ font-family: '{body}', serif; }}
                         .feedback-quote {{ font-family: '{body}', serif; }}
                         .reader-feedback-panel {{ font-family: '{body}', serif; }}
                         .reader-content {{ font-family: '{body}', serif; }}
                         .reader-content p {{ margin: 0 0 {p_spacing}px 0; text-indent: {p_indent}px; }}
                         .reader-welcome h2 {{ font-family: '{h1}', cursive; }}
                         .reader-content h1, .reader-content h2,
                         .reader-content h3 {{ font-family: '{h1}', cursive; }}
                         .reader-content blockquote {{ font-family: '{quote}', serif; }}"
                    )
                }}
            }

            if error_msg.get().is_some() {
                div { class: "reader-error",
                    div {
                        h2 { "PlotWeb" }
                        Text { color: "dimmed", size: "lg",
                            {move || error_msg.get().unwrap_or_default()}
                        }
                    }
                }
            } else if view_data.get().is_none() {
                div { class: "reader-welcome",
                    Text { color: "dimmed", "Loading..." }
                }
            } else {
                div { class: "reader-workspace",
                    // Mobile sidebar backdrop
                    div {
                        class: {move || if sidebar_open.get() { "reader-sidebar-backdrop open" } else { "reader-sidebar-backdrop" }},
                        onclick: move || sidebar_open.set(false),
                    }

                    // Sidebar
                    div {
                        class: {move || if sidebar_open.get() { "reader-sidebar open" } else { "reader-sidebar" }},
                        img {
                            class: "reader-sidebar-cover",
                            style: {move || if view_data.get().and_then(|d| d.cover_image.clone()).is_some() { String::new() } else { "display:none;".to_string() }},
                            src: {move || view_data.get().and_then(|d| d.cover_image.clone()).unwrap_or_default()},
                            alt: "Book cover",
                        }
                        div { class: "reader-sidebar-title",
                            {move || view_data.get().map(|d| d.book_title.clone()).unwrap_or_default()}
                        }
                        div { class: "reader-sidebar-meta",
                            "Reading as: "
                            strong {
                                {move || view_data.get().map(|d| d.reader_name.clone()).unwrap_or_default()}
                            }
                        }
                        div { class: "reader-sidebar-chapters",
                            for ch in view_data.get().map(|d| d.chapters.clone()).unwrap_or_default() {
                                {reader_chapter_item(__scope, ch.id.clone(), ch.title.clone(), ch.sort_order, active_chapter_id, load_chapter)}
                            }
                        }
                        if !is_preview && !bookmarks.get().is_empty() {
                            div { class: "reader-sidebar-bookmarks",
                                div { class: "reader-bookmarks-title", "Bookmarks" }
                                for bm in bookmarks.get() {
                                    {reader_bookmark_item(__scope, bm, open_chapter, delete_bookmark)}
                                }
                            }
                        }
                        if store.current_user.get().is_some() {
                            div { class: "reader-sidebar-footer",
                                Button {
                                    variant: "subtle",
                                    size: "xs",
                                    onclick: move || router::navigate(Route::Dashboard),
                                    "\u{2190} Dashboard"
                                }
                            }
                        }
                    }

                    // Main pane
                    div { class: "reader-main-pane",
                        // Mobile topbar
                        div { class: "reader-mobile-topbar",
                            ActionIcon {
                                variant: "subtle",
                                size: "sm",
                                onclick: move || sidebar_open.update(|v| *v = !*v),
                                {render_tabler_icon(__scope, TablerIcon::Menu2, TablerIconStyle::Outline)}
                            }
                            Text { weight: "600", size: "sm",
                                {move || current_chapter.get().map(|c| c.title.clone()).unwrap_or_else(|| "Select a chapter".into())}
                            }
                            div {
                                style: "display: flex; align-items: center; gap: 4px;",
                                if !is_preview {
                                    ActionIcon {
                                        variant: "subtle",
                                        size: "sm",
                                        onclick: add_bookmark,
                                        {render_tabler_icon(__scope, TablerIcon::Bookmark, TablerIconStyle::Outline)}
                                    }
                                    ActionIcon {
                                        variant: {move || if mobile_feedback_open.get() { "filled".to_string() } else { "subtle".to_string() }},
                                        size: "sm",
                                        onclick: move || mobile_feedback_open.update(|v| *v = !*v),
                                        {render_tabler_icon(__scope, TablerIcon::MessageCircle, TablerIconStyle::Outline)}
                                    }
                                }
                            }
                        }

                        // Desktop topbar
                        div { class: "reader-topbar",
                            div {
                                style: "display: flex; align-items: center; gap: 8px;",
                                Text { weight: "600",
                                    {move || current_chapter.get().map(|c| c.title.clone()).unwrap_or_else(|| "Select a chapter".into())}
                                }
                            }
                            div {
                                style: "display: flex; align-items: center; gap: 8px;",
                                if is_preview {
                                    Badge { variant: "light", size: "sm", "Preview" }
                                }
                                if !is_preview {
                                    Text { size: "xs", color: "dimmed", "Select text to leave feedback" }
                                    ActionIcon {
                                        variant: "subtle",
                                        size: "sm",
                                        onclick: add_bookmark,
                                        {render_tabler_icon(__scope, TablerIcon::Bookmark, TablerIconStyle::Outline)}
                                    }
                                    ActionIcon {
                                        variant: {move || if show_feedback_panel.get() { "filled".to_string() } else { "subtle".to_string() }},
                                        size: "sm",
                                        onclick: toggle_feedback,
                                        {render_tabler_icon(__scope, TablerIcon::MessageCircle, TablerIconStyle::Outline)}
                                    }
                                }
                            }
                        }

                        if current_chapter.get().is_none() {
                            div { class: "reader-welcome",
                                div {
                                    h2 { "Welcome" }
                                    Text { color: "dimmed",
                                        {move || format!(
                                            "Select a chapter from the sidebar to start reading."
                                        )}
                                    }
                                }
                            }
                        } else {
                            div {
                                style: "display: flex; flex: 1; overflow: hidden;",
                                // Paginated reading column + page controls
                                div { class: "reader-reading-col",
                                    div { class: "reader-viewport", id: "reader-viewport",
                                        div { class: "reader-page-frame",
                                            div {
                                                class: "reader-content",
                                                id: "reader-content",
                                            }
                                        }
                                    }
                                    div { class: "reader-pagebar",
                                        ActionIcon {
                                            variant: "subtle",
                                            size: "sm",
                                            onclick: move || go_to_page(current_page.get() - 1),
                                            {render_tabler_icon(__scope, TablerIcon::ChevronLeft, TablerIconStyle::Outline)}
                                        }
                                        div { class: "reader-pagebar-indicator",
                                            {move || format!("{} / {}", current_page.get() + 1, total_pages.get())}
                                        }
                                        ActionIcon {
                                            variant: "subtle",
                                            size: "sm",
                                            onclick: move || go_to_page(current_page.get() + 1),
                                            {render_tabler_icon(__scope, TablerIcon::ChevronRight, TablerIconStyle::Outline)}
                                        }
                                    }
                                }

                                // Mobile feedback backdrop
                                div {
                                    class: {move || if !is_preview && mobile_feedback_open.get() { "reader-feedback-backdrop open" } else { "reader-feedback-backdrop" }},
                                    onclick: move || mobile_feedback_open.set(false),
                                }

                                // Feedback panel (beta only)
                                if !is_preview {
                                div {
                                    class: {move || {
                                        let desktop = show_feedback_panel.get();
                                        let mobile = mobile_feedback_open.get();
                                        match (desktop, mobile) {
                                            (true, true) => "reader-feedback-panel mobile-open",
                                            (true, false) => "reader-feedback-panel",
                                            (false, true) => "reader-feedback-panel hidden mobile-open",
                                            (false, false) => "reader-feedback-panel hidden",
                                        }
                                    }},
                                    div { class: "reader-feedback-header",
                                        "Feedback"
                                        Badge {
                                            variant: "light",
                                            size: "sm",
                                            {move || {
                                                let ch_id = active_chapter_id.get();
                                                let count = feedback_list.get().iter()
                                                    .filter(|f| ch_id.as_ref().is_some_and(|cid| *cid == f.chapter_id))
                                                    .count();
                                                format!("{}", count)
                                            }}
                                        }
                                    }
                                    div { class: "reader-feedback-list",
                                        for fb in feedback_list.get().into_iter().filter(|f| active_chapter_id.get().as_ref().is_some_and(|cid| *cid == f.chapter_id)) {
                                            {reader_feedback_card(__scope, fb, reply_to_feedback)}
                                        }

                                        if feedback_list.get().iter().filter(|f| active_chapter_id.get().as_ref().is_some_and(|cid| *cid == f.chapter_id)).count() == 0 {
                                            Center {
                                                style: "padding: 20px 0;",
                                                Text { color: "dimmed", size: "sm", "No feedback for this chapter yet." }
                                            }
                                        }
                                    }
                                }
                                } // end if !is_preview (feedback panel)
                            }
                        }
                    }
                }
            }

            // Feedback tooltip (floating, beta only)
            if !is_preview {
            div {
                class: {move || if tooltip_visible.get() { "feedback-tooltip visible" } else { "feedback-tooltip" }},
                style: {move || format!(
                    "left: {}px; top: {}px;",
                    tooltip_x.get().max(10).min(web_sys::window().map(|w| w.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(800.0) as i32 - 310).unwrap_or(500)),
                    tooltip_y.get().max(10),
                )},
                textarea {
                    placeholder: "Leave your feedback...",
                    id: "feedback-tooltip-textarea",
                }
                div { class: "feedback-tooltip-actions",
                    Button {
                        variant: "subtle",
                        size: "xs",
                        onclick: move || tooltip_visible.set(false),
                        "Cancel"
                    }
                    Button {
                        size: "xs",
                        onclick: move || {
                            // Read textarea value
                            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                                if let Ok(Some(el)) = doc.query_selector("#feedback-tooltip-textarea") {
                                    let textarea: web_sys::HtmlTextAreaElement = el.dyn_into().unwrap();
                                    tooltip_comment.set(textarea.value());
                                    textarea.set_value("");
                                }
                            }
                            submit_feedback();
                        },
                        "Submit"
                    }
                }
            }
            } // end if !is_preview (feedback tooltip)
        }
    }
}
