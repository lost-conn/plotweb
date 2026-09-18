//! Note editor pane: the facet strip, the note's prose body with sigil autocomplete,
//! and the context rail that shows what the note is wired to.

use rinch::prelude::*;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{CreateNoteRequest, LinkTarget, Note, SaveReceipt, UpdateNoteRequest};

use crate::api;
use crate::pages::editor_utils;
use crate::rinch_backend::Editor;
use crate::store::AppStore;

use super::super::sigils::{self, EdgeKind, Offer, Sigil};
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

/// The id of the note currently open, if the note editor is the active pane.
fn open_note_id(state: BookState) -> Option<String> {
    match state.active_pane.get() {
        BookPane::NoteEditor(id) => Some(id),
        _ => None,
    }
}

/// The note currently open, as the store has it.
fn open_note(state: BookState, store: AppStore) -> Option<Note> {
    let id = open_note_id(state)?;
    store.notes.get().into_iter().find(|n| n.id == id)
}

// ── Sigil autocomplete ───────────────────────────────────────────────────────

/// Re-read the caret after an edit and decide whether a completion menu is owed.
///
/// Called from the editor's `on_change`, which is the only notification rinch gives —
/// there is no selection-change callback. That is enough here because a sigil only ever
/// appears by being typed, which is a document change; moving the caret *into* an
/// existing token deliberately does not reopen the menu, so clicking back into old
/// prose does not pop a list over it.
pub(in crate::pages::book) fn refresh_sigil_menu(state: BookState, store: AppStore) {
    let Some(doc) = state.sigil_doc.get() else {
        return;
    };
    let handle = state.note_handle.get();
    let Some(before) = editor_utils::text_before_caret(&handle) else {
        close_sigil_menu(state);
        return;
    };
    let Some(active) = sigils::active_sigil(&before) else {
        close_sigil_menu(state);
        return;
    };
    let self_id = open_note_id(state).unwrap_or_default();
    let rows = sigils::completions(
        active.sigil,
        &active.query,
        &self_id,
        &store.notes.get(),
        &store.chapters.get(),
    );
    if rows.is_empty() {
        close_sigil_menu(state);
        return;
    }
    state.sigil_caret.set(editor_utils::caret_anchor(&handle, &doc));
    state.sigil_rows.set(rows);
    state.sigil_highlight.set(0);
    state.sigil_active.set(Some(active));
}

pub(in crate::pages::book) fn close_sigil_menu(state: BookState) {
    if state.sigil_active.get().is_some() {
        state.sigil_active.set(None);
        state.sigil_rows.set(Vec::new());
        state.sigil_highlight.set(0);
    }
}

/// Take the completion at `index`: write the token into the body, and — for "create
/// new" — make the note it names first.
///
/// The edge itself is never written here. It is derived from the body on the next save
/// (`plotweb_common::extract_note_links_in`), which is what keeps the index and the
/// prose from ever disagreeing: this only types what the author would have typed.
fn take_completion(state: BookState, store: AppStore, book_id: String, index: usize) {
    let (Some(active), Some(row)) = (
        state.sigil_active.get(),
        state.sigil_rows.get().get(index).cloned(),
    ) else {
        return;
    };
    // "Create new" for `@`/`$` makes the note, so the token resolves on the very next
    // save rather than sitting unresolved until the author remembers to write it.
    if row.offer == Offer::Create && active.sigil != Sigil::Tag {
        create_linked_note(store, &book_id, &row.label, active.sigil == Sigil::Reference);
    }

    // `sigil + query` is what is on screen; a trailing space commits the token and
    // closes the menu in one go, which is what the author was about to type anyway.
    let typed = active.query.chars().count() + 1;
    let inserted = format!("{}{} ", active.sigil.char(), row.token);
    editor_utils::replace_before_caret(&state.note_handle.get(), typed, &inserted);
    close_sigil_menu(state);
}

/// Create the note a "create new" row names, and fold it into the local store so the
/// rail and the next completion list can see it without a round trip.
///
/// `entity` marks it as one, which is what makes it offerable under `$` — a `$` that
/// created a plain lore note would not be offered by the sigil that created it.
fn create_linked_note(store: AppStore, book_id: &str, title: &str, entity: bool) {
    let req = CreateNoteRequest {
        title: title.to_string(),
        parent_id: None,
        color: None,
    };
    let bid = book_id.to_string();
    api::post::<_, Note>(&format!("/api/books/{}/notes", bid), &req, move |result| {
        let Ok(note) = result else { return };
        let id = note.id.clone();
        let mut notes = store.notes.get();
        notes.push(note);
        store.notes.set(notes);
        if entity {
            let req = UpdateNoteRequest {
                is_entity: Some(true),
                ..Default::default()
            };
            crate::local_book::note_facets(&bid, &id, None, None, Some(true), None);
            api::put::<_, SaveReceipt>(
                &format!("/api/books/{}/notes/{}", bid, id),
                &req,
                move |_| {},
            );
            let mut notes = store.notes.get();
            if let Some(n) = notes.iter_mut().find(|n| n.id == id) {
                n.is_entity = true;
            }
            store.notes.set(notes);
        }
    });
}

/// Drive the open menu from the keyboard.
///
/// Returns whether the key was consumed. Split out from the listener so the decision is
/// plain logic rather than something only a browser can run.
// Only reachable through the web-only key listener below, which is compiled out on
// native — the menu is still fully usable there by clicking a row.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn handle_sigil_key(state: BookState, store: AppStore, book_id: &str, key: &str) -> bool {
    let rows = state.sigil_rows.get();
    if state.sigil_active.get().is_none() || rows.is_empty() {
        return false;
    }
    let at = state.sigil_highlight.get();
    match key {
        "ArrowDown" => state.sigil_highlight.set((at + 1) % rows.len()),
        "ArrowUp" => state.sigil_highlight.set((at + rows.len() - 1) % rows.len()),
        "Enter" | "Tab" => take_completion(state, store, book_id.to_string(), at),
        "Escape" => close_sigil_menu(state),
        _ => return false,
    }
    true
}

/// The completion list.
///
/// Positioned `fixed` at the caret, which rinch can report (see
/// [`editor_utils::caret_anchor`]). Rows are `onclick`, and rinch fires `onclick` on
/// **pointerdown** — which is what makes this work on touch, where a `click` would
/// arrive after the list had already moved under the finger.
fn sigil_menu(__scope: &mut RenderScope, state: BookState, store: AppStore, book_id: String) -> NodeHandle {
    let doc = state.sigil_doc;
    rsx! {
        div {
            class: "sigil-menu",
            id: "sigil-menu",
            style: {move || {
                let Some(caret) = state.sigil_caret.get() else {
                    return "display:none;".to_string();
                };
                if state.sigil_active.get().is_none() {
                    return "display:none;".to_string();
                }
                let Some(doc) = doc.get() else {
                    return "display:none;".to_string();
                };
                // Read inside the closure so the runtime's layout pass, which is what
                // fills the block's bounds in, re-runs this and snaps the menu onto the
                // caret rather than leaving it at the viewport origin.
                let (x, y) = editor_utils::caret_viewport_point(&caret, &doc);
                format!("left:{}px; top:{}px;", x.round(), (y + caret.height).round())
            }},
            for (i, row) in state.sigil_rows.get().into_iter().enumerate() {
                div {
                    key: {format!("{}-{}", i, row.token)},
                    class: {move || if state.sigil_highlight.get() == i {
                        "sigil-row selected"
                    } else {
                        "sigil-row"
                    }},
                    onclick: {
                        let book_id = book_id.clone();
                        move || take_completion(state, store, book_id.clone(), i)
                    },
                    span { class: "sigil-row-glyph", {offer_glyph(&row.offer)} }
                    span { class: "sigil-row-label", {row.label.clone()} }
                    span { class: "sigil-row-kind", {offer_hint(&row.offer)} }
                }
            }
        }
    }
}

/// A one-character gutter glyph, matching the facet vocabulary the tree will use.
fn offer_glyph(offer: &Offer) -> &'static str {
    match offer {
        Offer::Note { is_entity: true, .. } => "◆",
        Offer::Note { .. } => "○",
        Offer::Chapter { .. } => "¶",
        Offer::Tag => "#",
        Offer::Create => "+",
    }
}

fn offer_hint(offer: &Offer) -> &'static str {
    match offer {
        Offer::Note { is_entity: true, .. } => "entity",
        Offer::Note { .. } => "note",
        Offer::Chapter { .. } => "chapter",
        Offer::Tag => "tag",
        Offer::Create => "create",
    }
}

// ── Facet strip ──────────────────────────────────────────────────────────────

/// Toggle the entity facet through the patch fields, leaving every other facet absent.
///
/// Absent means "leave alone" (`plotweb_common::UpdateNoteRequest`), which is the whole
/// point of the patch shape: this must not clear a span as a side effect of marking a
/// character, any more than a keystroke may.
fn toggle_entity(state: BookState, store: AppStore, book_id: String, note_id: String, on: bool) {
    crate::local_book::note_facets(&book_id, &note_id, None, None, Some(on), None);
    let mut notes = store.notes.get();
    if let Some(n) = notes.iter_mut().find(|n| n.id == note_id) {
        n.is_entity = on;
    }
    store.notes.set(notes);
    let req = UpdateNoteRequest {
        is_entity: Some(on),
        ..Default::default()
    };
    api::put::<_, SaveReceipt>(
        &format!("/api/books/{}/notes/{}", book_id, note_id),
        &req,
        move |result| apply_note_save_receipt(result, state.note_save_status, state.save_alert),
    );
}

/// What the span reads as in the strip. Card 4 brings the calendar that turns a tick
/// into a date; until then the presence of a span is the whole of what can be said, and
/// saying it is still worth more than an empty row.
fn span_summary(note: &Note) -> String {
    match (&note.span, &note.relative) {
        (Some(span), _) if span.open_ended => "from a point in time".to_string(),
        (Some(span), _) if span.end.is_some() => "over a span".to_string(),
        (Some(_), _) => "at a point in time".to_string(),
        (None, Some(_)) => "placed against another note".to_string(),
        (None, None) => "undated".to_string(),
    }
}

fn facet_strip(__scope: &mut RenderScope, state: BookState, store: AppStore, book_id: String) -> NodeHandle {
    rsx! {
        div { class: "note-facets",
            div {
                class: {move || {
                    let on = open_note(state, store).map(|n| n.is_event()).unwrap_or(false);
                    if on { "note-facet is-on" } else { "note-facet" }
                }},
                // Event is not a toggle: a note becomes an event by being given a time,
                // and card 4 is what gives it one. Flipping a flag here would leave a
                // note claiming to be an event with nothing to place it by.
                title: "A note becomes an event when it is given a time",
                span { class: "note-facet-glyph", "◇" }
                "Event"
            }
            div {
                class: {move || {
                    let on = open_note(state, store).map(|n| n.is_entity).unwrap_or(false);
                    if on { "note-facet is-on is-button" } else { "note-facet is-button" }
                }},
                id: "note-facet-entity",
                onclick: {
                    let book_id = book_id.clone();
                    move || {
                        let Some(note) = open_note(state, store) else { return };
                        toggle_entity(state, store, book_id.clone(), note.id, !note.is_entity);
                    }
                },
                span { class: "note-facet-glyph", "◆" }
                "Entity"
            }
            div { class: "note-facet-span",
                {move || open_note(state, store).map(|n| span_summary(&n)).unwrap_or_default()}
            }
        }
    }
}

// ── Context rail ─────────────────────────────────────────────────────────────

fn rail_group(
    __scope: &mut RenderScope,
    state: BookState,
    store: AppStore,
    heading: &'static str,
    class: &'static str,
    rows: impl Fn(&Note, &[Note], &[plotweb_common::Chapter]) -> Vec<sigils::RailLink> + 'static + Copy,
) -> NodeHandle {
    rsx! {
        div { class: {format!("note-rail-group {class}")},
            div { class: "note-rail-heading", {heading} }
            if rail_rows(state, store, rows).is_empty() {
                div { class: "note-rail-empty", "Nothing yet" }
            }
            for row in rail_rows(state, store, rows).into_iter().enumerate().map(rail_row) {
                div {
                    key: {row.key.clone()},
                    class: {if row.openable { "note-rail-row is-openable" } else { "note-rail-row" }},
                    onclick: {
                        let open = row.open.clone();
                        move || {
                            // Only a note can be opened from here — a chapter lives in
                            // the other half of the workspace, and jumping there would
                            // abandon the note mid-edit.
                            if let Some(id) = open.clone() {
                                open_note_by_id(state, store, id);
                            }
                        }
                    },
                    span { class: "note-rail-glyph", {row.glyph} }
                    span { class: "note-rail-label", {row.label.clone()} }
                    if row.missing {
                        span { class: "note-rail-missing", "not written" }
                    }
                }
            }
        }
    }
}

/// One rail row, flattened so the `rsx!` loop body is a single element — the macro
/// only carries attributes for a loop whose body is the element itself.
#[derive(Clone, PartialEq)]
struct RailRow {
    key: String,
    label: String,
    glyph: &'static str,
    openable: bool,
    missing: bool,
    /// The note id to open, or `None` for a chapter or an unwritten target.
    open: Option<String>,
}

fn rail_row((i, link): (usize, sigils::RailLink)) -> RailRow {
    let openable = link.id.is_some() && link.target == LinkTarget::Note;
    RailRow {
        key: format!("{i}-{}", link.label),
        glyph: edge_glyph(link.edge, link.target),
        openable,
        missing: link.id.is_none(),
        open: openable.then(|| link.id.clone()).flatten(),
        label: link.label,
    }
}

fn rail_rows(
    state: BookState,
    store: AppStore,
    rows: impl Fn(&Note, &[Note], &[plotweb_common::Chapter]) -> Vec<sigils::RailLink>,
) -> Vec<sigils::RailLink> {
    let Some(note) = open_note(state, store) else {
        return Vec::new();
    };
    rows(&note, &store.notes.get(), &store.chapters.get())
}

/// `$` and `@` get different glyphs because they mean different things — only `$` will
/// draw a thread through an event — and a chapter gets the paragraph mark it has in the
/// completion list, so the two surfaces read as one vocabulary.
fn edge_glyph(edge: EdgeKind, target: LinkTarget) -> &'static str {
    match (edge, target) {
        (_, LinkTarget::Chapter) => "¶",
        (EdgeKind::Reference, _) => "◆",
        (EdgeKind::Mention, _) => "→",
    }
}

/// Open another note by id: the same load the tree's card click does.
///
/// Shared with the chapter editor's "Notes here" strip, which reaches the same notes
/// from the other end of an `@` edge.
pub(in crate::pages::book) fn open_note_by_id(
    state: BookState,
    store: AppStore,
    note_id: String,
) {
    let book_id = store
        .current_book
        .get()
        .map(|b| b.id)
        .unwrap_or_default();
    let url = format!("/api/books/{}/notes/{}", book_id, note_id);
    api::get::<Note>(&url, move |result| {
        let Ok(note) = result else { return };
        state.note_editor_title.set(note.title.clone());
        state.note_editor_color.set(note.color.clone());
        close_sigil_menu(state);
        state.active_pane.set(BookPane::NoteEditor(note.id.clone()));
        let handle = state.note_handle.get();
        crate::pages::editor_utils::load_note_content(&handle, &note.content);
        crate::local_store::attach_note(handle, book_id.clone(), note.id.clone(), note.content.clone());
    });
}

fn context_rail(__scope: &mut RenderScope, state: BookState, store: AppStore) -> NodeHandle {
    rsx! {
        aside { class: "note-rail",
            {rail_group(__scope, state, store, "References", "is-out", sigils::outbound)}
            {rail_group(__scope, state, store, "Mentioned by", "is-in", |n, notes, _| {
                sigils::inbound(n, notes)
            })}
            div { class: "note-rail-group",
                div { class: "note-rail-heading", "Tags" }
                if open_note(state, store).map(|n| n.links.tags.is_empty()).unwrap_or(true) {
                    div { class: "note-rail-empty", "Nothing yet" }
                }
                div { class: "note-rail-tags",
                    for tag in open_note(state, store).map(|n| n.links.tags.clone()).unwrap_or_default() {
                        span { key: {tag.clone()}, class: "note-rail-tag", {format!("#{tag}")} }
                    }
                }
            }
            div { class: "note-rail-group",
                div { class: "note-rail-heading", "In time" }
                div { class: "note-rail-empty",
                    {move || open_note(state, store).map(|n| span_summary(&n)).unwrap_or_default()}
                }
            }
        }
    }
}

/// Wire Up/Down/Enter/Tab/Escape to the menu.
///
/// A **capture-phase window listener**, because the editor's own key handling is on the
/// document and would otherwise also see the key: Enter would insert a paragraph as
/// well as taking the completion, and the arrows would move the caret out from under
/// the very token the menu is completing. Capture on `window` runs before any document
/// listener, so `stop_propagation` is what makes the menu's claim on those five keys
/// exclusive — and it only claims them while the menu is open, so ordinary typing is
/// untouched.
///
/// Web-only and compiled out elsewhere: constructing a `Closure` *aborts* off-wasm (see
/// `crate::platform`), so this cannot be a runtime guard. On desktop the menu is still
/// fully usable by clicking a row.
fn install_sigil_keys(state: BookState, store: AppStore, book_id: String) {
    // `forget()`-ed, like the book page's own window listeners: it stays bound for the
    // life of the tab, including after this page's scope is gone. Every signal it
    // touches is freed at that point and `Signal::get()` panics on a freed slot, so it
    // checks `is_alive` first — the same guard, and the same reason.
    let _ = (&state, &store, &book_id);
    crate::web_only! {
        use wasm_bindgen::JsCast;
        let cb = wasm_bindgen::closure::Closure::wrap(Box::new(move |e: web_sys::Event| {
            if !state.sigil_active.is_alive() {
                return;
            }
            let Ok(ev) = e.clone().dyn_into::<web_sys::KeyboardEvent>() else {
                return;
            };
            if handle_sigil_key(state, store, &book_id, &ev.key()) {
                ev.prevent_default();
                ev.stop_propagation();
                ev.stop_immediate_propagation();
            }
        }) as Box<dyn FnMut(web_sys::Event)>);
        if let Some(win) = crate::platform::window() {
            let opts = web_sys::AddEventListenerOptions::new();
            opts.set_capture(true);
            win.add_event_listener_with_callback_and_add_event_listener_options(
                "keydown",
                cb.as_ref().unchecked_ref(),
                &opts,
            )
            .ok();
        }
        cb.forget();
    }
}

/// Render the Note Editor pane (CSS toggle).
pub(in crate::pages::book) fn render<GB, SN>(
    __scope: &mut RenderScope,
    state: BookState,
    store: AppStore,
    book_id: String,
    saved_here_only: impl Fn() -> bool + Copy + 'static,
    go_back_to_notes: GB,
    schedule_note_save: SN,
) -> NodeHandle
where
    GB: Fn() + 'static + Copy,
    SN: Fn() + 'static + Copy,
{
    // The editor's change callback runs outside any render scope, so the document
    // handle the caret geometry needs is captured here, once, while there is one.
    state.sigil_doc.set(Some(__scope.doc_weak()));
    install_sigil_keys(state, store, book_id.clone());

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

            div { class: "note-editor-split",
                div { class: "note-editor-body",
                    {facet_strip(__scope, state, store, book_id.clone())}

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

                {context_rail(__scope, state, store)}
            }

            {sigil_menu(__scope, state, store, book_id)}
        }
    }
}
