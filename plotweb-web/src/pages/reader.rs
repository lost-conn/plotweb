use wasm_bindgen::JsCast;
use rinch::prelude::*;
use rinch_core::use_store;
use rinch_core::Signal;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{
    BetaFeedback, BetaReaderView, Chapter,
    CreateBetaFeedbackRequest, CreateBetaReplyRequest,
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

.reader-scroll {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    background: var(--rinch-color-body);
}

.reader-content {
    max-width: 720px;
    margin: 0 auto;
    padding: 48px;
    font-size: 16px;
    line-height: 1.8;
    color: var(--rinch-color-text);
    -webkit-font-smoothing: antialiased;
    user-select: text;
    cursor: text;
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
    .reader-content { padding: 16px; }

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
    let class = if fb.resolved { "feedback-card resolved" } else { "feedback-card" };
    let fb_comment = fb.comment.clone();
    let fb_created = fb.created_at.clone();
    let quote_style = if fb.selected_text.is_empty() { "display:none" } else { "" };
    let quote_text = if !fb.selected_text.is_empty() {
        let t = if fb.selected_text.len() > 100 {
            format!("{}...", &fb.selected_text[..100])
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

            div { class: "feedback-reply-input",
                onsubmit: reply_to_feedback(fb_id3.clone()),
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
    let store = use_store::<AppStore>();
    let view_data: Signal<Option<BetaReaderView>> = Signal::new(None);
    let current_chapter: Signal<Option<Chapter>> = Signal::new(None);
    let active_chapter_id: Signal<Option<String>> = Signal::new(None);
    let feedback_list: Signal<Vec<BetaFeedback>> = Signal::new(Vec::new());
    let show_feedback_panel: Signal<bool> = Signal::new(true);
    let sidebar_open: Signal<bool> = Signal::new(false);
    let mobile_feedback_open: Signal<bool> = Signal::new(false);
    let error_msg: Signal<Option<String>> = Signal::new(None);

    // Tooltip state
    let tooltip_visible: Signal<bool> = Signal::new(false);
    let tooltip_x: Signal<i32> = Signal::new(0);
    let tooltip_y: Signal<i32> = Signal::new(0);
    let tooltip_selected_text: Signal<String> = Signal::new(String::new());
    let tooltip_context: Signal<String> = Signal::new(String::new());
    let tooltip_comment: Signal<String> = Signal::new(String::new());

    let token_signal = Signal::new(token.clone());

    // Fetch book view
    let tok = token.clone();
    wasm_bindgen_futures::spawn_local(async move {
        match api::get::<BetaReaderView>(&format!("/api/beta/{}", tok)).await {
            Ok(data) => {
                if let Some(ref fs) = data.font_settings {
                    fonts::load_book_fonts(fs);
                }
                view_data.set(Some(data));
            }
            Err(e) => {
                error_msg.set(Some(e.message));
            }
        }
    });

    // Fetch feedback
    let tok = token.clone();
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(fb) = api::get::<Vec<BetaFeedback>>(&format!("/api/beta/{}/feedback", tok)).await {
            feedback_list.set(fb);
        }
    });

    // Check session first, then auto-claim if logged in.
    // Auth check must come first — the claim endpoint's Session extractor
    // could create a new empty session that overwrites the valid cookie.
    {
        let tok = token.clone();
        let store = use_store::<AppStore>();
        wasm_bindgen_futures::spawn_local(async move {
            // Check if we have a valid session
            if store.current_user.get().is_none() {
                if let Ok(user) = api::get::<plotweb_common::User>("/api/auth/me").await {
                    store.current_user.set(Some(user));
                }
            }
            // Only claim if we're logged in
            if store.current_user.get().is_some() {
                api::post::<_, serde_json::Value>(&format!("/api/beta/{}/claim", tok), &serde_json::json!({})).await.ok();
            }
        });
    }

    // Connect WebSocket for real-time feedback
    {
        let tok = token.clone();
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

    // Load chapter function
    let load_chapter = move |chapter_id: String| {
        move || {
            active_chapter_id.set(Some(chapter_id.clone()));
            tooltip_visible.set(false);
            sidebar_open.set(false);
            let tok = token_signal.get();
            let cid = chapter_id.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(ch) = api::get::<Chapter>(
                    &format!("/api/beta/{}/chapters/{}", tok, cid),
                ).await {
                    current_chapter.set(Some(ch.clone()));
                    // Render content after a tick
                    let window = web_sys::window().unwrap();
                    let closure = wasm_bindgen::closure::Closure::once(move || {
                        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                            if let Ok(Some(el)) = doc.query_selector("#reader-content") {
                                if ch.content.is_empty() {
                                    el.set_inner_html("<p><em>This chapter is empty.</em></p>");
                                } else {
                                    el.set_inner_html(&editor_utils::markdown_to_html(&ch.content));
                                }
                            }
                        }
                    });
                    window.set_timeout_with_callback_and_timeout_and_arguments_0(
                        closure.as_ref().unchecked_ref(),
                        50,
                    ).ok();
                    closure.forget();
                }
            });
        }
    };

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

        wasm_bindgen_futures::spawn_local(async move {
            let req = CreateBetaFeedbackRequest {
                chapter_id,
                selected_text,
                context_block,
                comment,
            };
            if api::post::<_, serde_json::Value>(&format!("/api/beta/{}/feedback", tok), &req).await.is_ok() {
                // Refresh feedback list
                if let Ok(fb) = api::get::<Vec<BetaFeedback>>(&format!("/api/beta/{}/feedback", tok)).await {
                    feedback_list.set(fb);
                }
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
            wasm_bindgen_futures::spawn_local(async move {
                let req = CreateBetaReplyRequest { content };
                if api::post::<_, serde_json::Value>(
                    &format!("/api/beta/{}/feedback/{}/replies", tok, fid),
                    &req,
                ).await.is_ok() {
                    // Refresh
                    if let Ok(fb) = api::get::<Vec<BetaFeedback>>(&format!("/api/beta/{}/feedback", tok)).await {
                        feedback_list.set(fb);
                    }
                }
            });
        }
    };

    let toggle_feedback = move || {
        show_feedback_panel.update(|v| *v = !*v);
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
                            ActionIcon {
                                variant: {move || if mobile_feedback_open.get() { "filled".to_string() } else { "subtle".to_string() }},
                                size: "sm",
                                onclick: move || mobile_feedback_open.update(|v| *v = !*v),
                                {render_tabler_icon(__scope, TablerIcon::MessageCircle, TablerIconStyle::Outline)}
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
                                Text { size: "xs", color: "dimmed", "Select text to leave feedback" }
                                ActionIcon {
                                    variant: {move || if show_feedback_panel.get() { "filled".to_string() } else { "subtle".to_string() }},
                                    size: "sm",
                                    onclick: toggle_feedback,
                                    {render_tabler_icon(__scope, TablerIcon::MessageCircle, TablerIconStyle::Outline)}
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
                                div { class: "reader-scroll",
                                    div {
                                        class: "reader-content",
                                        id: "reader-content",
                                    }
                                }

                                // Mobile feedback backdrop
                                div {
                                    class: {move || if mobile_feedback_open.get() { "reader-feedback-backdrop open" } else { "reader-feedback-backdrop" }},
                                    onclick: move || mobile_feedback_open.set(false),
                                }

                                // Feedback panel
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
                            }
                        }
                    }
                }
            }

            // Feedback tooltip (floating)
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
        }
    }
}
