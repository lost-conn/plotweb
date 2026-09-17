//! Version History pane: commit list, expandable diff/preview, and "load more".

use rinch::prelude::*;
use plotweb_common::{Chapter, CommitDiff, CommitInfo};

use crate::api;

use super::super::BookPane;
use super::super::state::BookState;

fn render_history_commit(
    __scope: &mut RenderScope,
    commit: CommitInfo,
    preview_commit: Signal<Option<String>>,
    preview_chapters: Signal<Vec<Chapter>>,
    preview_content: Signal<Option<Chapter>>,
    show_restore_confirm: Signal<Option<String>>,
    bid_signal: Signal<String>,
    history_diff: Signal<Option<CommitDiff>>,
) -> NodeHandle {
    let oid_signal = Signal::new(commit.oid.clone());
    let short_oid = commit.oid[..7.min(commit.oid.len())].to_string();
    let is_expanded = move || preview_commit.get().as_deref() == Some(&oid_signal.get());

    rsx! {
        div {
            style: "border: 1px solid var(--rinch-color-border); border-radius: var(--rinch-radius-sm); padding: 12px; margin-bottom: 8px;",

            div {
                style: "display: flex; justify-content: space-between; align-items: flex-start;",
                div {
                    Text { size: "sm", weight: "500", {commit.message.clone()} }
                    div {
                        style: "display: flex; gap: 8px; margin-top: 4px;",
                        Text { size: "xs", color: "dimmed", {commit.created_at.clone()} }
                        Text { size: "xs", color: "dimmed", {short_oid} }
                    }
                }
                div {
                    style: "display: flex; gap: 4px; flex-shrink: 0;",
                    Button {
                        variant: "subtle",
                        size: "xs",
                        onclick: move || {
                            if is_expanded() {
                                preview_commit.set(None);
                                preview_chapters.set(Vec::new());
                                preview_content.set(None);
                                history_diff.set(None);
                            } else {
                                let bid = bid_signal.get();
                                let oid = oid_signal.get();
                                preview_commit.set(Some(oid.clone()));
                                preview_content.set(None);
                                history_diff.set(None);
                                let oid2 = oid.clone();
                                let bid2 = bid.clone();
                                api::get::<Vec<Chapter>>(
                                    &format!("/api/books/{}/history/{}/chapters", bid, oid),
                                    move |result| {
                                        if let Ok(chapters) = result {
                                            preview_chapters.set(chapters);
                                        }
                                    },
                                );
                                api::get::<CommitDiff>(
                                    &format!("/api/books/{}/history/{}/diff", bid2, oid2),
                                    move |result| {
                                        if let Ok(diff) = result {
                                            history_diff.set(Some(diff));
                                        }
                                    },
                                );
                            }
                        },
                        {move || if is_expanded() { "Hide" } else { "Preview" }}
                    }
                    Button {
                        variant: "light",
                        size: "xs",
                        color: "blue",
                        onclick: move || {
                            show_restore_confirm.set(Some(oid_signal.get()));
                        },
                        "Restore"
                    }
                }
            }

            if is_expanded() {
                div {
                    style: "margin-top: 12px; border-top: 1px solid var(--rinch-color-border); padding-top: 12px;",

                    // Show diff hunks for each changed chapter
                    for ch_diff in history_diff.get().map(|d| d.changed_chapters).unwrap_or_default() {
                        {render_chapter_diff(__scope, ch_diff)}
                    }

                    if preview_chapters.get().is_empty() && history_diff.get().is_none() {
                        Text { size: "sm", color: "dimmed", "Loading..." }
                    }

                    // Chapter preview list
                    for ch in preview_chapters.get() {
                        {render_history_chapter_preview(
                            __scope,
                            ch.clone(),
                            preview_content,
                            bid_signal,
                            preview_commit,
                        )}
                    }
                }
            }
        }
    }
}

fn render_chapter_diff(
    __scope: &mut RenderScope,
    ch_diff: plotweb_common::ChapterDiff,
) -> NodeHandle {
    // NOTE: this renders the server's raw git line-diff of the stored content
    // string. Now that content is DocNode JSON, that diff is a JSON line-diff
    // (functional but not prose-friendly). A prose-aware / CRDT-native diff is
    // owned by the Phase-3 "CRDT-native history / diff / restore" card.
    // Pre-render all diff lines as HTML
    let mut diff_html = String::new();
    for hunk in &ch_diff.hunks {
        for line in &hunk.lines {
            let bg = match line.origin.as_str() {
                "+" => "background: rgba(40, 167, 69, 0.15);",
                "-" => "background: rgba(220, 53, 69, 0.15); text-decoration: line-through;",
                _ => "",
            };
            let prefix = match line.origin.as_str() {
                "+" => "+ ",
                "-" => "- ",
                _ => "  ",
            };
            let escaped = line.content.trim_end_matches('\n')
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            diff_html.push_str(&format!(
                "<div style=\"padding: 1px 8px; white-space: pre-wrap; {}\">{}{}</div>",
                bg, prefix, escaped
            ));
        }
    }

    let header = format!("{} ({})", ch_diff.chapter_title, ch_diff.change_type);

    // Build the diff pane up-front so we can inject into its handle directly.
    // The old `id` + `spawn_local` + `setTimeout(50ms)` + `query_selector` dance
    // existed only to wait for the node to appear; we have it right here, and
    // `set_inner_html` is cross-platform (web + native).
    let diff_el = rsx! {
        div {
            style: "font-family: monospace; font-size: 13px; line-height: 1.6; margin-top: 4px; border: 1px solid var(--rinch-color-border); border-radius: var(--rinch-radius-sm); overflow: hidden;",
        }
    };
    diff_el.set_inner_html(&diff_html);

    rsx! {
        div {
            style: "margin-bottom: 12px;",
            Text { size: "sm", weight: "600", {header} }
            {diff_el}
        }
    }
}

/// Build the history preview pane and publish its handle into `preview_el`.
///
/// The pane lives inside an rsx `if` (a `show_dom` branch), so the node only
/// exists once it's selected. Publishing the handle from the branch render lets
/// the click handler inject straight into it — no `id`, no `query_selector`.
fn history_preview_node(
    __scope: &mut RenderScope,
    preview_el: Signal<Option<NodeHandle>>,
) -> NodeHandle {
    let el = rsx! {
        div {
            style: "padding: 8px 12px; margin: 4px 0; background: var(--rinch-color-body); border-radius: var(--rinch-radius-sm); border: 1px solid var(--rinch-color-border); max-height: 400px; overflow-y: auto; font-size: 14px; line-height: 1.6;",
        }
    };
    preview_el.set(Some(el.clone()));
    el
}

fn render_history_chapter_preview(
    __scope: &mut RenderScope,
    chapter: Chapter,
    preview_content: Signal<Option<Chapter>>,
    bid_signal: Signal<String>,
    preview_commit: Signal<Option<String>>,
) -> NodeHandle {
    let cid_signal = Signal::new(chapter.id.clone());
    // Handle to the preview pane, published by `history_preview_node` when the
    // `if is_selected()` branch renders.
    let preview_el: Signal<Option<NodeHandle>> = Signal::new(None);
    let is_selected = move || {
        preview_content
            .get()
            .as_ref()
            .map(|c| c.id == cid_signal.get())
            .unwrap_or(false)
    };

    rsx! {
        div {
            style: "margin-bottom: 4px;",

            div {
                style: {move || if is_selected() {
                    "cursor: pointer; padding: 6px 8px; border-radius: var(--rinch-radius-sm); background: var(--rinch-color-blue-light);"
                } else {
                    "cursor: pointer; padding: 6px 8px; border-radius: var(--rinch-radius-sm);"
                }},
                onclick: move || {
                    if is_selected() {
                        preview_content.set(None);
                    } else {
                        let bid = bid_signal.get();
                        let ch_id = cid_signal.get();
                        if let Some(commit) = preview_commit.get() {
                            api::get::<Chapter>(
                                &format!("/api/books/{}/history/{}/chapters/{}", bid, commit, ch_id),
                                move |result| {
                                if let Ok(full_ch) = result {
                                    let html = crate::pages::editor_utils::sanitize_html(&crate::pages::editor_utils::content_to_display_html(&full_ch.content));
                                    // Setting `preview_content` synchronously renders
                                    // the `if is_selected()` branch below, which
                                    // publishes the pane's handle — so it's ready to
                                    // inject into on the next line. Replaces the old
                                    // `setTimeout(50ms)` + `query_selector` wait.
                                    preview_content.set(Some(full_ch));
                                    if let Some(el) = preview_el.get() {
                                        el.set_inner_html(&html);
                                    }
                                }
                                },
                            );
                        }
                    }
                },
                Text { size: "sm", {chapter.title.clone()} }
            }

            if is_selected() {
                {history_preview_node(__scope, preview_el)}
            }
        }
    }
}

/// Render the History pane (CSS toggle, always in DOM).
pub(in crate::pages::book) fn render(__scope: &mut RenderScope, state: BookState) -> NodeHandle {
    let BookState {
        active_pane,
        history_commits,
        history_preview_commit,
        history_preview_chapters,
        history_preview_content,
        show_restore_confirm,
        bid_signal,
        history_diff,
        ..
    } = state;
    rsx! {
        div {
            class: "book-main-scroll",
            style: {move || if matches!(active_pane.get(), BookPane::History) { "" } else { "display:none;" }},

            div { class: "chapters-pane",
                div { class: "chapters-pane-header",
                    Title { order: 3, "Version History" }
                }

                Space { h: "sm" }
                Text { size: "sm", color: "dimmed",
                    "Browse previous versions of your book. You can preview any version and restore it if needed."
                }
                Space { h: "md" }

                if history_commits.get().is_empty() {
                    Text { color: "dimmed", "No history available." }
                }

                for commit in history_commits.get() {
                    {render_history_commit(
                        __scope,
                        commit.clone(),
                        history_preview_commit,
                        history_preview_chapters,
                        history_preview_content,
                        show_restore_confirm,
                        bid_signal,
                        history_diff,
                    )}
                }

                if !history_commits.get().is_empty() {
                    Space { h: "md" }
                    Button {
                        variant: "subtle",
                        size: "sm",
                        onclick: {
                            let bid = bid_signal.get();
                            move || {
                                let offset = history_commits.get().len();
                                let bid = bid.clone();
                                api::get::<Vec<CommitInfo>>(
                                    &format!("/api/books/{}/history?offset={}", bid, offset),
                                    move |result| {
                                        if let Ok(more) = result {
                                            if !more.is_empty() {
                                                history_commits.update(|list| list.extend(more));
                                            }
                                        }
                                    },
                                );
                            }
                        },
                        "Load more"
                    }
                }
            }
        }
    }
}
