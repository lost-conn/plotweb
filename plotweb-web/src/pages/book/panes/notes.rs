//! Notes pane: the horizontal note tree, drag/drop reordering, and the
//! floating drag ghost. Notes are out of scope for behavioural changes here —
//! this surface is being rebuilt around a timeline later — so only
//! `render_note_card`'s signature changes (20 params -> `BookState`), not its
//! markup or behaviour.

use rinch::prelude::*;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{MoveNoteRequest, Note, NotesResponse, NoteTree, UpdateNoteTreeRequest};

use crate::api;
use crate::pages::editor_utils;
use crate::store::AppStore;

use super::super::state::BookState;
use super::super::BookPane;

/// Perform the note move for the current drag/drop selection.
/// Reads `drop_target` + `dragging_note_id`; if both are set, issues the
/// MoveNoteRequest PUT and refetches the notes tree into the store.
/// Shared by the note card's and drop zone's `ondrop` handlers.
fn perform_note_move(
    store: AppStore,
    bid_signal: Signal<String>,
    drop_target: Signal<Option<(Option<String>, usize)>>,
    dragging_note_id: Signal<Option<String>>,
) {
    if let Some(target) = drop_target.get() {
        if let Some(drag_id) = dragging_note_id.get() {
            let bid = bid_signal.get();
            let req = MoveNoteRequest {
                note_id: drag_id,
                new_parent_id: target.0,
                index: target.1,
            };
            let bid_refresh = bid.clone();
            let bid_sync = bid.clone();
            api::put::<_, serde_json::Value>(
                &format!("/api/books/{}/notes/move", bid), &req,
                move |result| {
                    if result.is_ok() {
                        api::get::<NotesResponse>(
                            &format!("/api/books/{}/notes", bid_refresh),
                            move |resp_result| {
                                if let Ok(resp) = resp_result {
                                    crate::local_book::sync_notes(&bid_sync, &resp.notes, &resp.tree);
                                    store.notes.set(resp.notes);
                                    store.note_tree.set(Some(resp.tree));
                                }
                            },
                        );
                    }
                },
            );
        }
    }
}

/// Render a drop zone for reordering notes between siblings.
/// `parent_id` is None for root level, Some(id) for children of a note.
/// `index` is the insertion position among siblings.
fn render_drop_zone(
    __scope: &mut RenderScope,
    parent_id: Signal<Option<String>>,
    index: usize,
    drop_target: Signal<Option<(Option<String>, usize)>>,
    dragging_note_id: Signal<Option<String>>,
    bid_signal: Signal<String>,
    store: AppStore,
) -> NodeHandle {
    rsx! {
        div {
            class: {move || {
                if dragging_note_id.get().is_none() {
                    "note-drop-zone"
                } else {
                    let is_active = drop_target.get()
                        .map(|(pid, idx)| pid == parent_id.get() && idx == index)
                        .unwrap_or(false);
                    if is_active { "note-drop-zone active" } else { "note-drop-zone visible" }
                }
            }},
            ondragenter: move || {
                drop_target.set(Some((parent_id.get(), index)));
            },
            ondrop: move || {
                perform_note_move(store, bid_signal, drop_target, dragging_note_id);
            },
        }
    }
}

/// Helper to strip HTML tags and produce a text preview, preserving paragraph breaks.
fn note_content_preview(html: &str) -> String {
    if html.is_empty() {
        return String::new();
    }
    let text = html
        .replace("</p>", "\n")
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("</div>", "\n")
        .replace("</li>", "\n");
    let mut result = String::new();
    let mut in_tag = false;
    for ch in text.chars() {
        if ch == '<' { in_tag = true; }
        else if ch == '>' { in_tag = false; }
        else if !in_tag { result.push(ch); }
    }
    let mut cleaned = String::new();
    let mut last_was_newline = false;
    for ch in result.trim().chars() {
        if ch == '\n' {
            if !last_was_newline {
                cleaned.push('\n');
                last_was_newline = true;
            }
        } else {
            cleaned.push(ch);
            last_was_newline = false;
        }
    }
    cleaned.chars().take(200).collect()
}

/// Render a note card and its children recursively as a horizontal tree.
/// All data is read reactively from store signals inside closures.
fn render_note_card(
    __scope: &mut RenderScope,
    note_id: String,
    // The card's own parent + position among its siblings, so ondragover can
    // compute a before/after insertion point.
    parent_id: Option<String>,
    sibling_index: usize,
    store: AppStore,
    state: BookState,
) -> NodeHandle {
    let BookState {
        active_pane,
        bid_signal,
        new_note_title,
        new_note_parent_id,
        new_note_color,
        show_note_modal,
        note_editor_title,
        note_editor_color,
        note_handle,
        note_dirty,
        loaded_note_id,
        dragging_note_id,
        drop_target,
        ghost_visible,
        ghost_pos,
        ghost_label,
        ghost_color,
        ..
    } = state;
    let nid = Signal::new(note_id);
    let parent_sig: Signal<Option<String>> = Signal::new(parent_id);

    rsx! {
        div { class: "note-branch",
            div {
                style: "position: relative; flex-shrink: 0;",
                div {
                    class: {move || {
                        let mut cls = "note-card".to_string();
                        let n = nid.get();
                        if dragging_note_id.get().as_deref() == Some(n.as_str()) {
                            cls.push_str(" dragging");
                        }
                        if let Some((ref pid, _)) = drop_target.get() {
                            if pid.as_deref() == Some(n.as_str()) {
                                cls.push_str(" drop-child");
                            }
                        }
                        cls
                    }},
                    style: {move || {
                        let notes = store.notes.get();
                        let n = nid.get();
                        let color = notes.iter().find(|note| note.id == n)
                            .and_then(|note| note.color.clone())
                            .unwrap_or_else(|| "teal".to_string());
                        format!("--note-color: var(--rinch-color-{}-6);", color)
                    }},
                    draggable: "true",
                    ondragstart: move || {
                        let n = nid.get();
                        dragging_note_id.set(Some(n.clone()));
                        // Seed the floating ghost with this note's title + color.
                        let notes = store.notes.get();
                        let note = notes.iter().find(|note| note.id == n);
                        ghost_label.set(
                            note.map(|note| note.title.clone()).unwrap_or_default(),
                        );
                        ghost_color.set(
                            note.and_then(|note| note.color.clone())
                                .unwrap_or_else(|| "teal".to_string()),
                        );
                        ghost_visible.set(true);
                    },
                    ondragmove: move || {
                        // Follow the cursor with the ghost (cursor is in ClickContext).
                        let ctx = rinch_core::events::get_click_context();
                        ghost_pos.set((ctx.mouse_x, ctx.mouse_y));
                    },
                    ondragover: move || {
                        // Split the card into thirds: top => insert before, bottom
                        // => insert after, middle => drop as a child of this note.
                        let ctx = rinch_core::events::get_click_context();
                        let h = ctx.element_height;
                        let rel_y = ctx.relative_y();
                        if h > 0.0 && rel_y < h / 3.0 {
                            drop_target.set(Some((parent_sig.get(), sibling_index)));
                        } else if h > 0.0 && rel_y > h * 2.0 / 3.0 {
                            drop_target.set(Some((parent_sig.get(), sibling_index + 1)));
                        } else {
                            drop_target.set(Some((Some(nid.get()), 0)));
                        }
                    },
                    ondragleave: move || {
                        // Clear the indicator only if it still points at this card,
                        // so a freshly-entered target isn't wiped out.
                        if let Some((ref pid, _)) = drop_target.get() {
                            if pid.as_deref() == Some(nid.get().as_str()) {
                                drop_target.set(None);
                            }
                        }
                    },
                    ondrop: move || {
                        perform_note_move(store, bid_signal, drop_target, dragging_note_id);
                    },
                    ondragend: move || {
                        dragging_note_id.set(None);
                        drop_target.set(None);
                        ghost_visible.set(false);
                    },
                    ondragenter: move || {
                        drop_target.set(Some((Some(nid.get()), 0)));
                    },
                    onclick: move || {
                        let n = nid.get();
                        let bid = bid_signal.get();
                        // Opening another note ends the current note's editing session,
                        // so it is an exit like any other: write out the pending
                        // debounced edit while the pane still names the note the model
                        // holds. Must happen before the fetch below, because by the time
                        // its callback runs the pane has moved on and the edit is gone.
                        super::super::flush::flush_pending_edits(state, store);
                        // A load is now in flight: until it lands the model still holds
                        // the *outgoing* note, so no save may name the incoming one.
                        loaded_note_id.set(None);
                        api::get::<Note>(
                            &format!("/api/books/{}/notes/{}", bid, n),
                            move |result| {
                            if let Ok(note) = result {
                                note_editor_title.set(note.title);
                                note_editor_color.set(note.color);
                                active_pane.set(BookPane::NoteEditor(n.clone()));

                                // Load into the note editor model (synchronous).
                                // Legacy-tolerant: DocNode JSON if it parses, else the
                                // legacy raw-HTML path (notes were stored as HTML).
                                note_dirty.set(false);
                                loaded_note_id.set(Some(n.clone()));
                                let handle = note_handle.get();
                                editor_utils::load_note_content(&handle, &note.content);
                                handle.set_dark_mode(store.dark_mode.get());

                                // Local-first (deliverable 2): back this note body with
                                // a durable Automerge doc (`note:{id}`) — the body-CRDT
                                // mirror of the chapter path. Additive/dual-write.
                                crate::local_store::attach_note(
                                    handle.clone(),
                                    bid.clone(),
                                    n.clone(),
                                    note.content.clone(),
                                );
                            }
                            },
                        );
                        store.sidebar_open.set(false);
                    },

                    div { class: "note-card-header",
                        div { class: "note-card-title",
                            {move || {
                                let notes = store.notes.get();
                                let n = nid.get();
                                notes.iter().find(|note| note.id == n)
                                    .map(|note| note.title.clone())
                                    .unwrap_or_default()
                            }}
                        }
                        div { class: "note-card-actions",
                            ActionIcon {
                                variant: "subtle",
                                size: "xs",
                                onclick: move || {
                                    new_note_title.set(String::new());
                                    new_note_parent_id.set(Some(nid.get()));
                                    new_note_color.set("teal".to_string());
                                    show_note_modal.set(true);
                                },
                                {render_tabler_icon(__scope, TablerIcon::Plus, TablerIconStyle::Outline)}
                            }
                            ActionIcon {
                                variant: "subtle",
                                size: "xs",
                                color: "red",
                                onclick: move || {
                                    let n = nid.get();
                                    let bid = bid_signal.get();
                                    let bid_refresh = bid.clone();
                                    let bid_sync = bid_refresh.clone();
                                    api::delete_req::<serde_json::Value>(
                                        &format!("/api/books/{}/notes/{}", bid, n),
                                        move |result| {
                                            if result.is_ok() {
                                                api::get::<NotesResponse>(
                                                    &format!("/api/books/{}/notes", bid_refresh),
                                                    move |resp_result| {
                                                        if let Ok(resp) = resp_result {
                                                            crate::local_book::sync_notes(&bid_sync, &resp.notes, &resp.tree);
                                                            store.notes.set(resp.notes);
                                                            store.note_tree.set(Some(resp.tree));
                                                        }
                                                    },
                                                );
                                            }
                                        },
                                    );
                                },
                                {render_tabler_icon(__scope, TablerIcon::Trash, TablerIconStyle::Outline)}
                            }
                        }
                    }
                    div {
                        class: "note-card-preview",
                        {move || {
                            let notes = store.notes.get();
                            let n = nid.get();
                            note_content_preview(
                                &notes.iter().find(|note| note.id == n)
                                    .map(|note| note.content.clone())
                                    .unwrap_or_default()
                            )
                        }}
                    }
                }
                if store.note_tree.get().map(|t| t.children.get(&nid.get()).map(|c| !c.is_empty()).unwrap_or(false)).unwrap_or(false) {
                    button {
                        class: "note-collapse-btn",
                        onclick: move || {
                            if let Some(tree) = store.note_tree.get() {
                                let n = nid.get();
                                let mut new_tree = tree.clone();
                                if new_tree.collapsed.contains(&n) {
                                    new_tree.collapsed.retain(|id| id != &n);
                                } else {
                                    new_tree.collapsed.push(n.clone());
                                }
                                store.note_tree.set(Some(new_tree.clone()));
                                let bid = bid_signal.get();
                                crate::local_book::sync_notes(&bid, &store.notes.get(), &new_tree);
                                let tree_req = UpdateNoteTreeRequest {
                                    tree: NoteTree {
                                        root_order: new_tree.root_order,
                                        children: new_tree.children,
                                        collapsed: new_tree.collapsed,
                                    },
                                };
                                api::put::<_, serde_json::Value>(
                                    &format!("/api/books/{}/notes/tree", bid),
                                    &tree_req,
                                    move |_result| {},
                                );
                            }
                        },
                        {move || {
                            if store.note_tree.get().map(|t| t.collapsed.contains(&nid.get())).unwrap_or(false) {
                                "\u{25b8}"
                            } else {
                                "\u{25be}"
                            }
                        }}
                    }
                }
            }

            if store.note_tree.get().map(|t| {
                let n = nid.get();
                let has_children = t.children.get(&n).map(|c| !c.is_empty()).unwrap_or(false);
                let collapsed = t.collapsed.contains(&n);
                has_children && !collapsed
            }).unwrap_or(false) {
                div { class: "note-children",
                    for (_idx, child_id) in store.note_tree.get().and_then(|t| t.children.get(&nid.get()).cloned()).unwrap_or_default().into_iter().enumerate() {
                        div { key: child_id.clone(), style: "display: contents;",
                            {render_drop_zone(__scope, Signal::new(Some(nid.get())), _idx, drop_target, dragging_note_id, bid_signal, store)}
                            div { class: "note-child-row",
                                {render_note_card(
                                    __scope,
                                    child_id.clone(),
                                    Some(nid.get()),
                                    _idx,
                                    store,
                                    state,
                                )}
                            }
                        }
                    }
                    {render_drop_zone(__scope, Signal::new(Some(nid.get())), store.note_tree.get().and_then(|t| t.children.get(&nid.get()).map(|c| c.len())).unwrap_or(0), drop_target, dragging_note_id, bid_signal, store)}
                }
            }
        }
    }
}

/// Render the Notes pane (CSS toggle).
pub(in crate::pages::book) fn render(__scope: &mut RenderScope, state: BookState, store: AppStore) -> NodeHandle {
    let BookState {
        active_pane,
        bid_signal,
        new_note_title,
        new_note_parent_id,
        new_note_color,
        show_note_modal,
        dragging_note_id,
        drop_target,
        ghost_visible,
        ghost_pos,
        ghost_label,
        ghost_color,
        ..
    } = state;
    rsx! {
        div {
            class: "notes-pane",
            style: {move || if matches!(active_pane.get(), BookPane::Notes) { "" } else { "display:none;" }},

            div { class: "notes-pane-header",
                Title { order: 3, "Notes" }
                Button {
                    size: "sm",
                    onclick: move || {
                        new_note_title.set(String::new());
                        new_note_parent_id.set(None);
                        new_note_color.set("teal".to_string());
                        show_note_modal.set(true);
                    },
                    "Add Note"
                }
            }

            if store.notes.get().is_empty() {
                div { class: "notes-empty",
                    Text { color: "dimmed", "No notes yet. Add one to start organizing your ideas!" }
                }
            }

            if !store.notes.get().is_empty() {
                div { class: "notes-tree",
                    for (_idx, root_id) in store.note_tree.get().map(|t| t.root_order.clone()).unwrap_or_default().into_iter().enumerate() {
                        div { key: root_id.clone(), style: "display: contents;",
                            {render_drop_zone(__scope, Signal::new(None), _idx, drop_target, dragging_note_id, bid_signal, store)}
                            {render_note_card(
                                __scope,
                                root_id.clone(),
                                None,
                                _idx,
                                store,
                                state,
                            )}
                        }
                    }
                    {render_drop_zone(__scope, Signal::new(None), store.note_tree.get().map(|t| t.root_order.len()).unwrap_or(0), drop_target, dragging_note_id, bid_signal, store)}
                }
            }

            // Floating drag ghost — follows the cursor during a drag.
            // Positioned imperatively from ondragmove via signals; a
            // reactive style binding is fine (only node swaps must use
            // if/match). pointer-events:none so it never eats events.
            div {
                class: "note-drag-ghost",
                style: {move || {
                    let (x, y) = ghost_pos.get();
                    let display = if ghost_visible.get() { "block" } else { "none" };
                    format!(
                        "display: {}; left: {}px; top: {}px; --note-color: var(--rinch-color-{}-6);",
                        display, x, y, ghost_color.get(),
                    )
                }},
                {move || ghost_label.get()}
            }
        }
    }
}
