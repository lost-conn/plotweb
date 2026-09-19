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
use super::super::time_entry;
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

/// What the note's time reads as, in the book's calendar: exactly what the time field
/// would hold for it, or "Undated".
fn time_summary(state: BookState, store: AppStore) -> String {
    let Some(note) = open_note(state, store) else {
        return String::new();
    };
    let text = time_entry::format_entry(
        note.span.as_ref(),
        note.relative.as_ref(),
        &super::calendar::book_calendar(store),
        &store.notes.get(),
    );
    if text.is_empty() { "Undated".to_string() } else { text }
}

/// Open the time field for the note on screen, prefilled with its time as it would be
/// typed — so an unchanged Enter writes back exactly what was there.
fn open_time_editor(state: BookState, store: AppStore) {
    let Some(note) = open_note(state, store) else { return };
    state.time_draft.set(time_entry::format_entry(
        note.span.as_ref(),
        note.relative.as_ref(),
        &super::calendar::book_calendar(store),
        &store.notes.get(),
    ));
    state.time_error.set(None);
    state.time_editing.set(Some(note.id));
}

fn time_editor_open(state: BookState) -> bool {
    let editing = state.time_editing.get();
    editing.is_some() && editing == open_note_id(state)
}

/// Commit the time field: the note's whole place in time, both halves at once.
///
/// Stored exactly as typed. There is deliberately **no** check against the note's event
/// parent — whether a child may fall outside its parent's span is card 6's open
/// question, so nothing here clamps, stretches or warns.
fn commit_time(state: BookState, store: AppStore, book_id: String, text: String) {
    let Some(note_id) = open_note_id(state) else { return };
    let entry = match time_entry::parse_entry(
        &text,
        &super::calendar::book_calendar(store),
        &store.notes.get(),
        &note_id,
    ) {
        Ok(entry) => entry,
        Err(e) => {
            state.time_error.set(Some(e));
            return;
        }
    };
    write_note_time(state, store, &book_id, &note_id, Some(entry.span), Some(entry.relative));
    state.time_error.set(None);
    state.time_editing.set(None);
}

/// Write a note's place in time — **the** write path for it, shared by the facet
/// strip's time field and the timeline's holding rail (a drop onto the line), so there
/// is one place that knows the order and the storage rules.
///
/// Both halves are patches, exactly as on `UpdateNoteRequest`: `None` leaves that facet
/// alone, `Some(None)` clears it. The local `book:` document goes first, through
/// `local_book::note_facets`, which writes a cleared facet as a **tombstone** rather
/// than deleting the key — the rule card 4 found a refetch needs, or a clear made here
/// is resurrected by the next note list. Then the projected list, so every view redraws
/// at once, then the server.
pub(in crate::pages::book) fn write_note_time(
    state: BookState,
    store: AppStore,
    book_id: &str,
    note_id: &str,
    span: Option<Option<plotweb_common::TimeSpan>>,
    relative: Option<Option<plotweb_common::RelativeTime>>,
) {
    crate::local_book::note_facets(book_id, note_id, span.clone(), relative.clone(), None, None);
    let mut notes = store.notes.get();
    if let Some(n) = notes.iter_mut().find(|n| n.id == note_id) {
        if let Some(span) = &span {
            n.span = span.clone();
        }
        if let Some(relative) = &relative {
            n.relative = relative.clone();
        }
    }
    store.notes.set(notes);
    let req = UpdateNoteRequest {
        span,
        relative,
        ..Default::default()
    };
    api::put::<_, SaveReceipt>(
        &format!("/api/books/{}/notes/{}", book_id, note_id),
        &req,
        move |result| apply_note_save_receipt(result, state.note_save_status, state.save_alert),
    );
}

/// The line under the field: the state and depth the text reads as, or why it cannot be
/// read. Live, so the author sees "Approximate · known to the Season" before committing.
fn time_preview(state: BookState, store: AppStore) -> (bool, String) {
    if let Some(e) = state.time_error.get() {
        return (true, e);
    }
    let cal = super::calendar::book_calendar(store);
    let self_id = open_note_id(state).unwrap_or_default();
    match time_entry::parse_entry(&state.time_draft.get(), &cal, &store.notes.get(), &self_id) {
        Ok(entry) => (false, time_entry::describe(&entry, &cal)),
        Err(e) => (true, e),
    }
}

/// The book's units, coarsest first — the hint under the field, so an author on an
/// invented calendar can see what the parts of a date are called here.
fn unit_hint(store: AppStore) -> String {
    let cal = super::calendar::book_calendar(store);
    let units: Vec<&str> = cal.units.iter().map(|u| u.name.as_str()).collect();
    format!("Parts, largest first: {}", units.join(" · "))
}

fn time_editor(__scope: &mut RenderScope, state: BookState, store: AppStore, book_id: String) -> NodeHandle {
    let BookState { time_draft, time_error, time_editing, .. } = state;
    let submit_id = book_id.clone();
    let set_id = book_id.clone();
    let clear_id = book_id;
    rsx! {
        div { class: "note-time-editor", id: "note-time-editor",
            div { class: "note-time-row",
                div { class: "note-time-input",
                    TextInput {
                        placeholder: "1206 · ~1206 · 1181 – 1211 · 1198 – · after The Siege",
                        value_fn: move || time_draft.get(),
                        oninput: move |v: String| {
                            time_draft.set(v);
                            time_error.set(None);
                        },
                        onsubmit: move || commit_time(state, store, submit_id.clone(), time_draft.get()),
                    }
                }
                div { id: "note-time-set", style: "display: contents;",
                    Button {
                        size: "xs",
                        onclick: move || commit_time(state, store, set_id.clone(), time_draft.get()),
                        "Set"
                    }
                }
                div { id: "note-time-clear", style: "display: contents;",
                    Button {
                        size: "xs",
                        variant: "subtle",
                        color: "gray",
                        onclick: move || {
                            time_draft.set(String::new());
                            commit_time(state, store, clear_id.clone(), String::new());
                        },
                        "Undated"
                    }
                }
                div { id: "note-time-cancel", style: "display: contents;",
                    Button {
                        size: "xs",
                        variant: "subtle",
                        color: "gray",
                        onclick: move || {
                            time_error.set(None);
                            time_editing.set(None);
                        },
                        "Cancel"
                    }
                }
            }
            div {
                class: {move || if time_preview(state, store).0 { "note-time-preview is-error" } else { "note-time-preview" }},
                {move || time_preview(state, store).1}
            }
            div { class: "note-time-help",
                span { {move || unit_hint(store)} }
                span {
                    class: "note-time-calendar",
                    id: "note-time-calendar",
                    // Leaves the note: `open_calendar` flushes its pending edit first.
                    onclick: move || super::calendar::open_calendar(state, store),
                    "Calendar…"
                }
            }
        }
    }
}

fn facet_strip(__scope: &mut RenderScope, state: BookState, store: AppStore, book_id: String) -> NodeHandle {
    rsx! {
        div { class: "note-facets-block",
        div { class: "note-facets",
            div {
                class: {move || {
                    let on = open_note(state, store).map(|n| n.is_event()).unwrap_or(false);
                    if on { "note-facet is-on is-button" } else { "note-facet is-button" }
                }},
                id: "note-facet-event",
                // Event is not a toggle: a note becomes an event by being given a time,
                // so this opens the time field rather than flipping a flag that would
                // leave a note claiming to be an event with nothing to place it by.
                title: "A note becomes an event when it is given a time",
                onclick: move || open_time_editor(state, store),
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
            div {
                class: "note-facet-span",
                id: "note-facet-when",
                title: "When this happens — click to change",
                onclick: move || open_time_editor(state, store),
                {move || time_summary(state, store)}
            }
        }
        if time_editor_open(state) {
            {time_editor(__scope, state, store, book_id.clone())}
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
    // Following a rail link or a "Notes here" chip leaves the note currently open, so it
    // flushes like every other exit — before the fetch, while the pane still names the
    // note whose text the model holds.
    super::super::flush::flush_pending_edits(state, store);
    // A load is in flight; the model still holds the outgoing note until it lands.
    state.loaded_note_id.set(None);
    api::get::<Note>(&url, move |result| {
        let Ok(note) = result else { return };
        state.note_editor_title.set(note.title.clone());
        state.note_editor_color.set(note.color.clone());
        close_sigil_menu(state);
        state.active_pane.set(BookPane::NoteEditor(note.id.clone()));
        // Opening a note is the one moment it is known-clean: the model now holds
        // exactly what the server sent.
        state.note_dirty.set(false);
        state.loaded_note_id.set(Some(note.id.clone()));
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
                div { class: "note-rail-empty note-rail-when",
                    {move || time_summary(state, store)}
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
