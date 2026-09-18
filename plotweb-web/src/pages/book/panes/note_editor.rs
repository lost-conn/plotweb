//! Note editor pane: title, color picker, and the note's prose body. Notes are
//! out of scope for behaviour changes (a timeline rebuild follows later).

use rinch::prelude::*;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{SaveReceipt, UpdateNoteRequest};

use crate::api;
use crate::pages::editor_utils;
use crate::rinch_backend::Editor;

use super::super::state::BookState;
use super::super::BookPane;

/// The note editor's half of [`super::chapters::save_chapter_body`], including the same
/// one-answer fallback: a note body is carried by the same one writer, and lands nowhere in
/// the same way when the canonical copy cannot be read.
///
/// Title and colour are structure, which REST carries either way, so they ride along on
/// both attempts.
pub(in crate::pages::book) fn save_note_body(
    url: String,
    title: String,
    content: String,
    color: Option<String>,
    cut_over: bool,
    note_save_status: Signal<&'static str>,
    save_alert: Signal<Option<String>>,
) {
    // Facets are left absent, which means "leave them alone" — this is the body save,
    // and it must not undate an event as a side effect of a keystroke.
    let build = move |body: Option<String>| UpdateNoteRequest {
        title: Some(title.clone()),
        content: body,
        color: color.clone(),
        ..Default::default()
    };
    let req = build((!cut_over).then(|| content.clone()));
    let retry_url = url.clone();
    api::put::<_, SaveReceipt>(&url, &req, move |result| match result {
        Ok(receipt) if cut_over && !receipt.is_durable() => {
            let req = build(Some(content));
            api::put::<_, SaveReceipt>(&retry_url, &req, move |result| {
                apply_note_save_receipt(result, note_save_status, save_alert)
            });
        }
        other => apply_note_save_receipt(other, note_save_status, save_alert),
    });
}

/// A note body takes the same server path as a chapter's, so the same receipt decides
/// whether this counts as saved.
pub(in crate::pages::book) fn apply_note_save_receipt(
    result: Result<SaveReceipt, api::ApiError>,
    note_save_status: Signal<&'static str>,
    save_alert: Signal<Option<String>>,
) {
    match result {
        Ok(receipt) => {
            note_save_status.set(if receipt.is_durable() { "saved" } else { "error" });
            save_alert.set(receipt.warning);
        }
        Err(_) => note_save_status.set("error"),
    }
}

/// Render the Note Editor pane (CSS toggle).
pub(in crate::pages::book) fn render<GB, SN>(
    __scope: &mut RenderScope,
    state: BookState,
    book_id: String,
    saved_here_only: impl Fn() -> bool + Copy + 'static,
    go_back_to_notes: GB,
    schedule_note_save: SN,
) -> NodeHandle
where
    GB: Fn() + 'static + Copy,
    SN: Fn() + 'static + Copy,
{
    let BookState {
        active_pane,
        note_editor_title,
        note_save_status,
        save_alert,
        note_editor_color,
        note_handle,
        ..
    } = state;
    rsx! {
        div {
            style: {move || if matches!(active_pane.get(), BookPane::NoteEditor(_)) { "" } else { "display:none;" }},
            class: "note-editor-pane",

            div { class: "note-editor-topbar",
                div { class: "note-editor-topbar-left",
                    ActionIcon {
                        variant: "subtle",
                        onclick: go_back_to_notes,
                        {render_tabler_icon(__scope, TablerIcon::ArrowLeft, TablerIconStyle::Outline)}
                    }
                    Text { weight: "600", {move || note_editor_title.get()} }
                }
                div {
                    style: "display: flex; align-items: center; gap: 8px;",
                    div {
                        class: "note-save-indicator",
                        {move || match note_save_status.get() {
                            "saving" => "Saving...".to_string(),
                            "saved" if saved_here_only() => {
                                "Saved on this device".to_string()
                            }
                            "saved" => "Saved".to_string(),
                            _ => "Unsaved".to_string(),
                        }}
                    }
                }
            }

            if save_alert.get().is_some() {
                div {
                    style: "padding: 8px 16px 0;",
                    Alert {
                        color: "orange",
                        title: "This note isn't reaching the server",
                        {move || save_alert.get().unwrap_or_default()}
                    }
                }
            }

            div { class: "note-editor-body",
                TextInput {
                    label: "Title",
                    value_fn: move || note_editor_title.get(),
                    oninput: move |v: String| {
                        note_editor_title.set(v);
                        schedule_note_save();
                    },
                }
                Space { h: "sm" }
                Text { size: "sm", weight: "500", "Color" }
                Space { h: "xs" }
                div { class: "note-color-picker",
                    for color in ["teal", "blue", "violet", "pink", "red", "orange", "yellow", "green", "gray"] {
                        div {
                            key: color,
                            class: {
                                let c = color.to_string();
                                move || {
                                    let selected = note_editor_color.get().as_deref() == Some(&c) ||
                                        (note_editor_color.get().is_none() && c == "teal");
                                    if selected { "note-color-dot selected" } else { "note-color-dot" }
                                }
                            },
                            style: {
                                let c = color.to_string();
                                move || format!("background: var(--rinch-color-{}-6);", c)
                            },
                            onclick: {
                                let c = color.to_string();
                                move || {
                                    note_editor_color.set(Some(c.clone()));
                                    schedule_note_save();
                                }
                            },
                        }
                    }
                }
                Space { h: "md" }
                Text { size: "sm", weight: "500", "Content" }
                Space { h: "xs" }

                {editor_utils::editor_toolbar(__scope, note_handle.get(), book_id.clone(), schedule_note_save)}

                div {
                    class: "editor-content",
                    id: "note-editor-main",
                    style: "min-height: 200px; padding: 12px; border: 1px solid var(--rinch-color-border); border-radius: var(--rinch-radius-sm);",
                    Editor {
                        editor: note_handle.get(),
                        content: String::new(),
                    }
                }
            }
        }
    }
}
