//! Beta-reader feedback cards: the editor sidebar's per-chapter list, the beta
//! readers overview's all-feedback list, and the shared reply-thread renderer.

use std::collections::HashMap;

use rinch::prelude::*;
// Both are reached only from inside `web_only!` blocks below, so on the native
// target they are genuinely unused. Gated rather than deleted — matches how
// `main.rs` imports `JsCast`.
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{BetaFeedback, Chapter};

#[cfg(target_arch = "wasm32")]
use super::panes::editor::scroll_to_text_in_editor;

/// Render a feedback card in the editor sidebar.
pub(super) fn editor_feedback_item<AR, RF, DF, ARO, RFO, DFO>(
    __scope: &mut RenderScope,
    fb: BetaFeedback,
    scroll_target: Signal<Option<(String, String)>>,
    reply_drafts: Signal<HashMap<String, String>>,
    author_reply: AR,
    resolve_feedback: RF,
    delete_feedback: DF,
) -> NodeHandle
where
    AR: Fn(String) -> ARO + 'static + Copy,
    RF: Fn(String) -> RFO + 'static + Copy,
    DF: Fn(String) -> DFO + 'static + Copy,
    ARO: Fn() + 'static,
    RFO: Fn() + 'static,
    DFO: Fn() + 'static,
{
    let _fb_id = fb.id.clone();
    let fb_id3 = fb.id.clone();
    let fb_id4 = fb.id.clone();
    let fb_id5 = fb.id.clone();
    let fb_id_enter = fb.id.clone();
    let fb_id_value = std::rc::Rc::new(fb.id.clone());
    let fb_id_input = fb.id.clone();
    let class = if fb.resolved { "feedback-card resolved" } else { "feedback-card" };
    let reply_input_id = format!("author-reply-{}", fb.id);

    let reader_line = fb.reader_name.clone();
    let from_agent = fb.is_agent();
    let quote_style = if fb.selected_text.is_empty() { "display:none" } else { "" };
    // Store scroll data in a signal so the onclick closure only captures Copy types (signals)
    let scroll_data: Signal<(String, String)> = Signal::new((fb.selected_text.clone(), fb.context_block.clone()));
    let quote_line = if !fb.selected_text.is_empty() {
        format!("\u{201c}{}\u{201d}", ellipsize(&fb.selected_text, 80))
    } else { String::new() };
    let comment_line = fb.comment.clone();
    let reply_nodes: Vec<NodeHandle> = fb.replies.iter().map(|r| {
        book_reply_item(__scope, r.author_type.clone(), r.author_name.clone(), r.content.clone())
    }).collect();

    let reply_submit_id = __scope.register_handler(author_reply(fb_id_enter));
    let reply_box = rsx! {
        div { class: "feedback-reply-input",
            textarea {
                id: reply_input_id,
                placeholder: "Reply...",
                rows: "1",
                value: {
                    let k = fb_id_value.clone();
                    move || reply_drafts.get().get(k.as_str()).cloned().unwrap_or_default()
                },
                oninput: move |v: String| {
                    let key = fb_id_input.clone();
                    reply_drafts.update(|m| { m.insert(key, v); });
                },
            }
            ActionIcon { variant: "subtle", size: "xs", onclick: author_reply(fb_id3),
                {render_tabler_icon(__scope, TablerIcon::Send, TablerIconStyle::Outline)}
            }
        }
    };
    reply_box.set_attribute("data-onsubmit", &reply_submit_id.0.to_string());

    rsx! {
        div {
            class: class,
            key: _fb_id,
            div { class: "feedback-reader-name",
                {reader_line}
                if from_agent {
                    {agent_mark(__scope)}
                }
            }
            div { class: "feedback-quote", style: quote_style,
                onclick: move || {
                    let data = scroll_data.get();
                    // Set the signal (for cross-chapter navigation). This half
                    // works on both targets and must stay outside `web_only!`.
                    scroll_target.set(Some(data.clone()));
                    // Also immediately scroll (for same-chapter clicks).
                    // Web-only: `scroll_to_text_in_editor` walks the DOM editor.
                    crate::web_only! {
                        let closure = wasm_bindgen::closure::Closure::once(move || {
                            scroll_to_text_in_editor(&data.0, &data.1);
                        });
                        if let Some(w) = crate::platform::window() {
                            w.set_timeout_with_callback_and_timeout_and_arguments_0(
                                closure.as_ref().unchecked_ref(), 50,
                            ).ok();
                        }
                        closure.forget();
                    }
                },
                {quote_line}
            }
            div { class: "feedback-comment", {comment_line} }
            div { class: "feedback-replies",
                {reply_nodes}
            }
            div { class: "feedback-actions",
                {reply_box}
                div { style: "display: flex; gap: 4px; margin-top: 4px;",
                    ActionIcon { variant: "subtle", size: "xs", onclick: resolve_feedback(fb_id4),
                        {render_tabler_icon(__scope, TablerIcon::Check, TablerIconStyle::Outline)}
                    }
                    ActionIcon { variant: "subtle", size: "xs", color: "red", onclick: delete_feedback(fb_id5),
                        {render_tabler_icon(__scope, TablerIcon::Trash, TablerIconStyle::Outline)}
                    }
                }
            }
        }
    }
}

/// Render a feedback card in the beta readers overview pane.
pub(super) fn overview_feedback_item<AR, RF, DF, NF, ARO, RFO, DFO, NFO>(
    __scope: &mut RenderScope,
    fb: BetaFeedback,
    ch_title: String,
    reply_drafts: Signal<HashMap<String, String>>,
    author_reply: AR,
    resolve_feedback: RF,
    delete_feedback: DF,
    navigate_to_feedback: NF,
) -> NodeHandle
where
    AR: Fn(String) -> ARO + 'static + Copy,
    RF: Fn(String) -> RFO + 'static + Copy,
    DF: Fn(String) -> DFO + 'static + Copy,
    NF: Fn(String, String, String) -> NFO + 'static + Copy,
    ARO: Fn() + 'static,
    RFO: Fn() + 'static,
    DFO: Fn() + 'static,
    NFO: Fn() + 'static,
{
    let _fb_id = fb.id.clone();
    let fb_id2 = fb.id.clone();
    let fb_id3 = fb.id.clone();
    let fb_id4 = fb.id.clone();
    let fb_id5 = fb.id.clone();
    let fb_id_enter = fb.id.clone();
    let fb_id_value = std::rc::Rc::new(fb.id.clone());
    let fb_id_input = fb.id.clone();
    let class = if fb.resolved { "feedback-card resolved" } else { "feedback-card" };
    let fb_reader = fb.reader_name.clone();
    let from_agent = fb.is_agent();
    let fb_comment = fb.comment.clone();
    let quote_style = if fb.selected_text.is_empty() { "display:none" } else { "" };
    let nav_chapter_id = fb.chapter_id.clone();
    let nav_selected_text = fb.selected_text.clone();
    let nav_context_block = fb.context_block.clone();
    let quote_text = if !fb.selected_text.is_empty() {
        format!("\u{201c}{}\u{201d}", ellipsize(&fb.selected_text, 100))
    } else { String::new() };
    let reply_nodes: Vec<NodeHandle> = fb.replies.iter().map(|r| {
        book_reply_item(__scope, r.author_type.clone(), r.author_name.clone(), r.content.clone())
    }).collect();

    let reply_submit_id = __scope.register_handler(author_reply(fb_id_enter));
    let reply_box = rsx! {
        div { class: "feedback-reply-input",
            textarea {
                id: {format!("author-reply-{}", fb_id4)},
                placeholder: "Reply...",
                rows: "1",
                value: {
                    let k = fb_id_value.clone();
                    move || reply_drafts.get().get(k.as_str()).cloned().unwrap_or_default()
                },
                oninput: move |v: String| {
                    let key = fb_id_input.clone();
                    reply_drafts.update(|m| { m.insert(key, v); });
                },
            }
            ActionIcon {
                variant: "subtle",
                size: "xs",
                onclick: author_reply(fb_id5),
                {render_tabler_icon(__scope, TablerIcon::Send, TablerIconStyle::Outline)}
            }
        }
    };
    reply_box.set_attribute("data-onsubmit", &reply_submit_id.0.to_string());

    rsx! {
        Paper {
            key: _fb_id,
            shadow: "xs",
            p: "sm",
            radius: "sm",
            class: class,

            div {
                style: "display: flex; align-items: center; justify-content: space-between; margin-bottom: 4px;",
                div {
                    style: "display: flex; align-items: center; gap: 6px;",
                    Badge { variant: "light", size: "xs", {fb_reader} }
                    if from_agent {
                        {agent_mark(__scope)}
                    }
                    Text { size: "xs", color: "dimmed", {ch_title} }
                }
                div {
                    style: "display: flex; gap: 4px;",
                    ActionIcon {
                        variant: "subtle",
                        size: "xs",
                        onclick: resolve_feedback(fb_id2),
                        {render_tabler_icon(__scope, TablerIcon::Check, TablerIconStyle::Outline)}
                    }
                    ActionIcon {
                        variant: "subtle",
                        size: "xs",
                        color: "red",
                        onclick: delete_feedback(fb_id3),
                        {render_tabler_icon(__scope, TablerIcon::Trash, TablerIconStyle::Outline)}
                    }
                }
            }

            div { class: "feedback-quote", style: quote_style,
                onclick: navigate_to_feedback(nav_chapter_id, nav_selected_text, nav_context_block),
                {quote_text}
            }
            div { class: "feedback-comment", {fb_comment} }

            div { class: "feedback-replies",
                {reply_nodes}
            }

            {reply_box}
        }
    }
}

pub(super) fn book_reply_item(
    __scope: &mut RenderScope,
    author_type: String,
    author_name: String,
    content: String,
) -> NodeHandle {
    let class_str = match author_type.as_str() {
        "owner" => "feedback-reply-author owner",
        plotweb_common::REPLY_AUTHOR_AGENT => "feedback-reply-author agent",
        _ => "feedback-reply-author",
    };
    let from_agent = author_type == plotweb_common::REPLY_AUTHOR_AGENT;
    rsx! {
        div { class: "feedback-reply",
            span { class: class_str,
                {author_name}
                if from_agent {
                    {agent_mark(__scope)}
                }
                ": "
            }
            {content}
        }
    }
}

/// The quiet marker beside the name on anything the author's AI agent wrote — its
/// review comments and its replies. Deliberately small: the comment is what matters,
/// the marker only says where it came from.
fn agent_mark(__scope: &mut RenderScope) -> NodeHandle {
    rsx! {
        span { class: "feedback-agent-mark", title: "Written by an AI agent using one of your access tokens", "AI" }
    }
}

/// At most `max` characters of `s`, with an ellipsis if it was cut. By characters,
/// not bytes: a quote is prose, and slicing it at a byte offset panics on the first
/// curly quote or accented letter that straddles the cut.
fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}...", s.chars().take(max).collect::<String>())
    } else {
        s.to_string()
    }
}

#[allow(dead_code)] // Not currently wired into any pane — see the same note above the pre-split definition.
pub(super) fn render_feedback_overview<AR, RF, DF, NF, ARO, RFO, DFO, NFO>(
    __scope: &mut RenderScope,
    feedback: &[BetaFeedback],
    chapters: &[Chapter],
    reply_drafts: Signal<HashMap<String, String>>,
    author_reply: AR,
    resolve_feedback: RF,
    delete_feedback: DF,
    navigate_to_feedback: NF,
) -> NodeHandle
where
    AR: Fn(String) -> ARO + 'static + Copy,
    RF: Fn(String) -> RFO + 'static + Copy,
    DF: Fn(String) -> DFO + 'static + Copy,
    NF: Fn(String, String, String) -> NFO + 'static + Copy,
    ARO: Fn() + 'static,
    RFO: Fn() + 'static,
    DFO: Fn() + 'static,
    NFO: Fn() + 'static,
{
    let nodes: Vec<NodeHandle> = feedback.iter().map(|fb| {
        let ch_title = chapters.iter()
            .find(|c| c.id == fb.chapter_id)
            .map(|c| c.title.clone())
            .unwrap_or_else(|| String::from("Unknown"));
        overview_feedback_item(__scope, fb.clone(), ch_title, reply_drafts, author_reply, resolve_feedback, delete_feedback, navigate_to_feedback)
    }).collect();
    rsx! { {nodes} }
}
