//! The notes workspace surface: the Tree / Timeline switcher, the shared filter bar,
//! and the vertical outline (`design/04-notes-wireframes.html`, take E).
//!
//! This file's header used to say notes were out of scope for behavioural change,
//! pending a timeline rebuild. That rebuild is now under way and this is card 3 of it,
//! so the horizontal drag-and-drop card tree — and `render_note_card`, the 20-parameter
//! recursive function the design pass deliberately left alone — are gone.
//!
//! Three things decide the shape here:
//!
//! * **One tree, not one section per facet.** Facets are not exclusive (a character with
//!   a lifespan is both an entity and an event), so sections would stop partitioning and
//!   a note would have to appear twice. Facet is a gutter glyph and a filter instead,
//!   and the tree stays what the author built — "everything about House Vaun in one
//!   place", whatever facets those notes carry.
//! * **Filter state and selection live above the switcher** (on `BookState`, see
//!   [`super::super::notes_filter`]), so when card 5's timeline lands, switching to it is
//!   a re-render and not a navigation: the same narrowing, the same selected note.
//! * **Interactive controls stay outside the draggable element.** rinch dispatches a
//!   non-activated draggable's click on the *drag source*, so a button nested inside a
//!   `draggable="true"` row never sees its own click (`rinch-web`'s `event_delegation`,
//!   pointerup path). The twist and the row actions are therefore siblings of
//!   `.note-card`, not children of it. `.note-card` itself stays the drag source and the
//!   row's own click target, which is also what keeps `notes-dnd.spec.ts` — mouse drag,
//!   touch long-press drag, and the drop-into-thirds geometry — pointed at the same
//!   element it always was.

use rinch::prelude::*;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{MoveNoteRequest, Note, NotesResponse, NoteTree, UpdateNoteTreeRequest};

use crate::api;
use crate::pages::editor_utils;
use crate::store::AppStore;

use super::super::notes_filter::{
    self, ChipState, Filter, Term, TreeFilter,
};
use super::super::state::BookState;
use super::super::BookPane;

/// Which view of the notes is on screen.
///
/// Card 5 adds `Timeline` here and an arm to [`render`]'s body; until it exists the tab
/// is rendered disabled rather than pointed at a placeholder, because a view that is
/// announced and then isn't one is worse than a view that is plainly not ready yet.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::pages::book) enum NotesView {
    Tree,
}

/// Perform the note move for the current drag/drop selection.
/// Reads `drop_target` + `dragging_note_id`; if both are set, issues the
/// MoveNoteRequest PUT and refetches the notes tree into the store.
/// Shared by the note row's and drop zone's `ondrop` handlers.
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
/// `index` is the insertion position among siblings — always the position in the *full*
/// sibling list, never in the filtered one, so a narrowed tree still moves notes to
/// where the author dropped them.
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

// ── Reading the store ────────────────────────────────────────────────────────

fn note_by_id(store: AppStore, id: &str) -> Option<Note> {
    store.notes.get().into_iter().find(|n| n.id == id)
}

/// The filter resolved against the current tree. Recomputed per read rather than cached
/// in a signal: it is a walk over a few hundred notes at most, and a cache would be one
/// more thing that can disagree with the notes it describes.
fn tree_filter(state: BookState, store: AppStore) -> TreeFilter {
    let tree = store.note_tree.get().unwrap_or(NoteTree {
        root_order: Vec::new(),
        children: Default::default(),
        collapsed: Vec::new(),
    });
    notes_filter::apply_to_tree(&store.notes.get(), &tree, &state.notes_filter.get())
}

/// One row's place among its siblings. A struct rather than a tuple so the `rsx!` for
/// loop's key closure names a field instead of destructuring a position it does not use.
#[derive(Clone, PartialEq)]
struct OutlineRow {
    id: String,
    /// Position among **all** the siblings, filtered-out ones included.
    index: usize,
}

/// The children of `parent` (or the roots, for `None`) that survive the filter, each
/// with its index among *all* the siblings — the index a move has to be expressed in.
fn visible_children(
    state: BookState,
    store: AppStore,
    parent: Option<&str>,
) -> Vec<OutlineRow> {
    let Some(tree) = store.note_tree.get() else {
        return Vec::new();
    };
    let all: Vec<String> = match parent {
        None => tree.root_order.clone(),
        Some(p) => tree.children.get(p).cloned().unwrap_or_default(),
    };
    let shown = tree_filter(state, store);
    all.into_iter()
        .enumerate()
        .filter(|(_, id)| shown.is_shown(id))
        .map(|(index, id)| OutlineRow { id, index })
        .collect()
}

fn sibling_count(store: AppStore, parent: Option<&str>) -> usize {
    store
        .note_tree
        .get()
        .map(|t| match parent {
            None => t.root_order.len(),
            Some(p) => t.children.get(p).map(|c| c.len()).unwrap_or(0),
        })
        .unwrap_or(0)
}

fn has_children(store: AppStore, id: &str) -> bool {
    store
        .note_tree
        .get()
        .map(|t| t.children.get(id).map(|c| !c.is_empty()).unwrap_or(false))
        .unwrap_or(false)
}

fn is_collapsed(store: AppStore, id: &str) -> bool {
    store
        .note_tree
        .get()
        .map(|t| t.collapsed.contains(&id.to_string()))
        .unwrap_or(false)
}

/// Refetch the notes and tree into the store — what every mutation here ends with.
fn refresh_notes(store: AppStore, bid: String) {
    api::get::<NotesResponse>(&format!("/api/books/{}/notes", bid), move |resp_result| {
        if let Ok(resp) = resp_result {
            crate::local_book::sync_notes(&bid, &resp.notes, &resp.tree);
            store.notes.set(resp.notes);
            store.note_tree.set(Some(resp.tree));
        }
    });
}

/// Fold or unfold a note, locally and then on the server (the collapse set is part of
/// the tree document, so it travels with it).
///
/// A childless row keeps its twist cell — the glyphs below it have to line up — but the
/// cell does nothing there, rather than writing a collapse nobody can see.
fn toggle_collapsed(store: AppStore, bid_signal: Signal<String>, id: String) {
    if !has_children(store, &id) {
        return;
    }
    let Some(tree) = store.note_tree.get() else {
        return;
    };
    let mut new_tree = tree.clone();
    if new_tree.collapsed.contains(&id) {
        new_tree.collapsed.retain(|c| c != &id);
    } else {
        new_tree.collapsed.push(id);
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

/// Open a note from a tree row.
///
/// **This is an exit.** Opening another note ends the current note's editing session, so
/// the pending debounced edit has to be written out while the pane still names the note
/// the editor model holds — before the fetch below, because by the time its callback
/// runs the pane has moved on and the edit is gone (see `pages/book/flush.rs`).
fn open_note_row(state: BookState, store: AppStore, note_id: String) {
    let bid = state.bid_signal.get();
    super::super::flush::flush_pending_edits(state, store);
    state.notes_selected.set(Some(note_id.clone()));
    // A load is now in flight: until it lands the model still holds the *outgoing*
    // note, so no save may name the incoming one.
    state.loaded_note_id.set(None);
    api::get::<Note>(
        &format!("/api/books/{}/notes/{}", bid, note_id),
        move |result| {
            if let Ok(note) = result {
                state.note_editor_title.set(note.title);
                state.note_editor_color.set(note.color);
                state.active_pane.set(BookPane::NoteEditor(note_id.clone()));

                // Load into the note editor model (synchronous). Legacy-tolerant:
                // DocNode JSON if it parses, else the legacy raw-HTML path (notes were
                // stored as HTML).
                state.note_dirty.set(false);
                state.loaded_note_id.set(Some(note_id.clone()));
                let handle = state.note_handle.get();
                editor_utils::load_note_content(&handle, &note.content);
                handle.set_dark_mode(store.dark_mode.get());

                // Local-first (deliverable 2): back this note body with a durable
                // Automerge doc (`note:{id}`) — the body-CRDT mirror of the chapter
                // path. Additive/dual-write.
                crate::local_store::attach_note(
                    handle.clone(),
                    bid.clone(),
                    note_id.clone(),
                    note.content.clone(),
                );
            }
        },
    );
    store.sidebar_open.set(false);
}

// ── The outline ──────────────────────────────────────────────────────────────

/// One row and, if it is unfolded, its children.
///
/// Replaces `render_note_card`. The parameter list is short because everything it used
/// to be handed one signal at a time now rides on `BookState`; what is left is the
/// row's own identity and its position among its siblings, which is what `ondragover`
/// needs to turn a cursor into an insertion point.
fn render_row(
    __scope: &mut RenderScope,
    state: BookState,
    store: AppStore,
    note_id: String,
    parent_id: Option<String>,
    sibling_index: usize,
) -> NodeHandle {
    let BookState {
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
        notes_selected,
        ..
    } = state;
    let nid = Signal::new(note_id);
    let parent_sig: Signal<Option<String>> = Signal::new(parent_id);

    rsx! {
        div { class: "note-branch",
            div {
                class: {move || {
                    let n = nid.get();
                    let mut cls = "note-row".to_string();
                    if tree_filter(state, store).is_muted(&n) {
                        // Kept only because something below it matched: this is the
                        // path to a result, not a result.
                        cls.push_str(" is-muted");
                    }
                    if notes_selected.get().as_deref() == Some(n.as_str()) {
                        cls.push_str(" is-selected");
                    }
                    cls
                }},

                // Outside `.note-card` on purpose — a click on an interactive
                // descendant of a draggable is dispatched on the draggable instead.
                div {
                    class: "note-twist",
                    onclick: move || toggle_collapsed(store, bid_signal, nid.get()),
                    {move || {
                        let n = nid.get();
                        if !has_children(store, &n) {
                            ""
                        } else if is_collapsed(store, &n) {
                            "\u{25b8}"
                        } else {
                            "\u{25be}"
                        }
                    }}
                }

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
                        let color = note_by_id(store, &nid.get())
                            .and_then(|note| note.color.clone())
                            .unwrap_or_else(|| "teal".to_string());
                        format!("--note-color: var(--rinch-color-{}-6);", color)
                    }},
                    draggable: "true",
                    ondragstart: move || {
                        let n = nid.get();
                        dragging_note_id.set(Some(n.clone()));
                        // Seed the floating ghost with this note's title + color.
                        let note = note_by_id(store, &n);
                        ghost_label.set(
                            note.as_ref().map(|note| note.title.clone()).unwrap_or_default(),
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
                        // Split the row into thirds: top => insert before, bottom
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
                        // Clear the indicator only if it still points at this row,
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
                    onclick: move || open_note_row(state, store, nid.get()),

                    span {
                        class: {move || {
                            let facet = note_by_id(store, &nid.get())
                                .map(|n| notes_filter::shown_facet(&n))
                                .unwrap_or(notes_filter::Facet::Lore);
                            format!("note-glyph {}", facet.css())
                        }},
                        title: {move || {
                            note_by_id(store, &nid.get())
                                .map(|n| notes_filter::shown_facet(&n).label().to_string())
                                .unwrap_or_default()
                        }},
                        {move || {
                            note_by_id(store, &nid.get())
                                .map(|n| notes_filter::shown_facet(&n).glyph().to_string())
                                .unwrap_or_default()
                        }}
                    }
                    span {
                        class: "note-card-title",
                        {move || {
                            note_by_id(store, &nid.get())
                                .map(|note| note.title.clone())
                                .unwrap_or_default()
                        }}
                    }
                    span {
                        class: "note-when",
                        {move || {
                            note_by_id(store, &nid.get())
                                .and_then(|n| notes_filter::span_label(&n, &store.notes.get()))
                                .unwrap_or_default()
                        }}
                    }
                }

                // Also outside the draggable, for the same reason as the twist.
                div { class: "note-row-actions",
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
                            api::delete_req::<serde_json::Value>(
                                &format!("/api/books/{}/notes/{}", bid, n),
                                move |result| {
                                    if result.is_ok() {
                                        refresh_notes(store, bid_refresh.clone());
                                    }
                                },
                            );
                        },
                        {render_tabler_icon(__scope, TablerIcon::Trash, TablerIconStyle::Outline)}
                    }
                }
            }

            if has_children(store, &nid.get()) && !is_collapsed(store, &nid.get()) {
                div { class: "note-children",
                    for row in visible_children(state, store, Some(&nid.get())) {
                        div { key: {row.id.clone()}, style: "display: contents;",
                            {render_drop_zone(__scope, Signal::new(Some(nid.get())), row.index, drop_target, dragging_note_id, bid_signal, store)}
                            {render_row(__scope, state, store, row.id.clone(), Some(nid.get()), row.index)}
                        }
                    }
                    {render_drop_zone(__scope, Signal::new(Some(nid.get())), sibling_count(store, Some(&nid.get())), drop_target, dragging_note_id, bid_signal, store)}
                }
            }
        }
    }
}

// ── The filter bar ───────────────────────────────────────────────────────────

/// One chip, flattened so the `rsx!` loop body is a single element — the macro only
/// carries attributes for a loop whose body is the element itself.
#[derive(Clone, PartialEq)]
struct ChipRow {
    key: String,
    label: String,
    op: &'static str,
    state: &'static str,
    title: String,
    term: Term,
}

fn chip_rows(state: BookState, store: AppStore) -> Vec<ChipRow> {
    let filter = state.notes_filter.get();
    notes_filter::chips_for(&store.notes.get())
        .into_iter()
        .map(|term| {
            let chip = filter.state_of(&term);
            ChipRow {
                key: term.key(),
                label: term.label(),
                op: chip.op(),
                state: chip.css(),
                title: match chip {
                    ChipState::Off => format!("{} — click to require it", term.label()),
                    other => format!("{} {}", other.label(), term.label()),
                },
                term,
            }
        })
        .collect()
}

/// What the filter currently says, in words.
///
/// The chips carry an operator glyph each, which is enough to read one chip and not
/// enough to read five. This is the sentence version — and it is where "any of" earns
/// its name, because it is the only state whose members have to be named together.
fn filter_summary(state: BookState, store: AppStore) -> String {
    let filter = state.notes_filter.get();
    let shown = tree_filter(state, store);
    let total = store.notes.get().len();
    let mut out = if filter.is_empty() {
        format!("{} note{}", total, if total == 1 { "" } else { "s" })
    } else {
        format!("{} of {} notes", shown.matched, total)
    };
    let clause = |label: &str, state: ChipState| {
        let terms = filter.terms_in(state);
        if terms.is_empty() {
            return String::new();
        }
        let names: Vec<String> = terms.iter().map(|t| t.label()).collect();
        format!(" \u{b7} {label} {}", names.join(", "))
    };
    out.push_str(&clause("must", ChipState::Must));
    out.push_str(&clause("any of", ChipState::AnyOf));
    out.push_str(&clause("without", ChipState::Without));
    out
}

fn render_filter_bar(__scope: &mut RenderScope, state: BookState, store: AppStore) -> NodeHandle {
    rsx! {
        div { class: "notes-filter",
            div { class: "notes-filter-chips",
                for row in chip_rows(state, store) {
                    div {
                        key: {row.key.clone()},
                        class: "fchip",
                        data-state: {row.state},
                        title: {row.title.clone()},
                        onclick: {
                            let term = row.term.clone();
                            move || {
                                let next = state.notes_filter.get().cycled(&term);
                                state.notes_filter.set(next);
                            }
                        },
                        span { class: "op", {row.op} }
                        span { class: "lbl", {row.label.clone()} }
                    }
                }
                if !state.notes_filter.get().is_empty() {
                    div {
                        class: "notes-filter-clear",
                        onclick: move || state.notes_filter.set(Filter::cleared()),
                        "clear"
                    }
                }
            }
            div { class: "notes-filter-status", {move || filter_summary(state, store)} }
        }
    }
}

// ── The surface ──────────────────────────────────────────────────────────────

/// Render the Notes surface (CSS toggle — every pane stays mounted, see `panes/mod.rs`).
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
        notes_view,
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

            // View switcher. Selection and filter state sit above it (on `BookState`),
            // so when the timeline lands, moving between views is a re-render of the
            // same narrowed, same selected set of notes — not a navigation.
            div { class: "notes-views",
                div {
                    class: {move || match notes_view.get() {
                        NotesView::Tree => "notes-viewtab is-on",
                    }},
                    onclick: move || {
                        // Changing view never leaves a note editor today (this surface
                        // is only on screen when Notes is the active pane), but every
                        // control that can move the pane calls the same one thing —
                        // that is the property that keeps the next editor from
                        // regressing save-on-leave. See `pages/book/flush.rs`.
                        super::super::flush::flush_pending_edits(state, store);
                        notes_view.set(NotesView::Tree);
                    },
                    "Tree"
                }
                div {
                    class: "notes-viewtab is-disabled",
                    title: "The timeline arrives with the book's calendar",
                    aria-disabled: "true",
                    "Timeline"
                }
            }

            {render_filter_bar(__scope, state, store)}

            if store.notes.get().is_empty() {
                div { class: "notes-empty",
                    Text { color: "dimmed", "No notes yet. Add one to start organizing your ideas!" }
                }
            }

            if !store.notes.get().is_empty() && tree_filter(state, store).shown.is_empty() {
                div { class: "notes-empty",
                    Text { color: "dimmed", "No note matches this filter." }
                }
            }

            if !store.notes.get().is_empty() {
                div { class: "notes-tree",
                    for row in visible_children(state, store, None) {
                        div { key: {row.id.clone()}, style: "display: contents;",
                            {render_drop_zone(__scope, Signal::new(None), row.index, drop_target, dragging_note_id, bid_signal, store)}
                            {render_row(__scope, state, store, row.id.clone(), None, row.index)}
                        }
                    }
                    {render_drop_zone(__scope, Signal::new(None), sibling_count(store, None), drop_target, dragging_note_id, bid_signal, store)}
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
