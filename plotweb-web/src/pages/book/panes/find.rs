//! The find-and-replace panel: a floating surface over the book workspace.
//!
//! Not one of the eight `display:none` panes — a find that closed the chapter
//! it is searching would be useless, so this is a fixed overlay that can be open
//! on top of whatever is. Ctrl+F opens it scoped to the chapter in the editor,
//! Ctrl+Shift+F to the whole book; the scope switch in the panel is the same
//! choice, so neither shortcut is a mode the author is stuck in.
//!
//! # Where the hits come from
//!
//! Two sources, and they are not interchangeable:
//!
//! - **The open chapter** is searched from `EditorHandle::doc()` — the live
//!   document, including everything typed since the last save. Anything else
//!   would count a chapter the author is looking at wrongly.
//! - **Every other chapter** is searched from `AppStore::chapters`' stored
//!   `content`, which the book load fetched for all of them. That is what makes
//!   a whole-book search instant instead of forty round trips.
//!
//! Both go through [`crate::find::doc`], which builds the second the way the
//! editor would build it — so a result row for an unopened chapter can be
//! clicked, the chapter opened, and the *n*th hit found again in the live
//! document and selected. Navigation carries the hit's **index**, never its
//! position: a position taken from a stored copy would be a guess about a
//! document that had not been loaded yet.
//!
//! # Replacing, and the one writer rule
//!
//! Every book is cut over, so the CRDT sync engine is the only writer of chapter
//! bodies and it runs only through a live `EditorHandle` (see
//! `local_store::attach_chapter` and `crate::sync`'s header). There is therefore
//! no such thing here as replacing in a chapter that is not open: "Replace all
//! in book" walks the chapters, opening each one through the ordinary
//! chapter-switch path, **waits for its body document to be attached**, and runs
//! the replacement as a transaction on the mounted editor.
//!
//! Waiting is the part that is easy to get wrong. `loaded_chapter_id` is set the
//! moment the REST body is in the editor model, but `attach_chapter` is still
//! in flight at that point — and when it lands it either adopts this device's
//! stored document (replacing the model) or seeds a fresh one from the REST
//! content. A replacement made in that window is discarded by whichever of those
//! happens, silently, with the editor showing the right text for a fraction of a
//! second first. So readiness is `loaded_chapter_id` **and**
//! `local_store::body_is_open`, polled; see [`when_chapter_ready`].

use rinch::prelude::*;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};

use crate::components::dialog::Dialog;
use crate::find::{self, FindOptions, Hit};
use crate::pages::editor_utils;
use crate::store::AppStore;

use super::super::BookPane;
use super::super::state::BookState;

/// How long the panel waits after a keystroke before searching the book.
const DEBOUNCE_MS: u32 = 150;

/// How long [`when_chapter_ready`] waits for a chapter's body to attach before
/// giving up and going ahead anyway (`ATTACH_POLL_MS` × this).
const ATTACH_MAX_POLLS: u32 = 120;
const ATTACH_POLL_MS: u32 = 50;

/// One chapter's worth of results.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::pages::book) struct ChapterHits {
    pub chapter_id: String,
    pub title: String,
    pub hits: Vec<Hit>,
    /// Whether this chapter's stored body could be read at all. A chapter that
    /// could not be is listed with no hits and said so, rather than counted as
    /// zero — "no matches" and "not searched" are different facts, and only one
    /// of them makes a replace-all count trustworthy.
    pub readable: bool,
}

// ── Reading the panel's state ────────────────────────────────────────────────

fn options(state: BookState) -> FindOptions {
    FindOptions {
        match_case: state.find_match_case.get(),
        whole_word: state.find_whole_word.get(),
    }
}

/// The chapter whose *live* document is the one to search — open in the editor
/// **and** finished loading.
fn open_chapter_id(state: BookState) -> Option<String> {
    match state.active_pane.get() {
        BookPane::Editor(id) if state.loaded_chapter_id.get().as_deref() == Some(id.as_str()) => {
            Some(id)
        }
        _ => None,
    }
}

/// Every hit in the results, flattened to `(chapter_id, index within chapter)`
/// in book order — the order Prev/Next cycles.
fn flat_hits(state: BookState) -> Vec<(String, usize)> {
    state
        .find_results
        .get()
        .iter()
        .flat_map(|g| (0..g.hits.len()).map(|i| (g.chapter_id.clone(), i)))
        .collect()
}

/// Whether "Replace all in book" is walking the chapters.
///
/// Every control that would start a second walk, change what the running one is
/// replacing, or move the editor out from under it is refused while this holds
/// — the walk switches chapters itself, and it reads the query and the switches
/// once per chapter. Flipping Match case halfway would leave the first half of
/// the book replaced under one rule and the second half under another.
pub(in crate::pages::book) fn busy(state: BookState) -> bool {
    state.find_busy.get().is_some()
}

/// How many hits there are in total.
pub(in crate::pages::book) fn total_hits(state: BookState) -> usize {
    state.find_results.get().iter().map(|g| g.hits.len()).sum()
}

// ── Searching ────────────────────────────────────────────────────────────────

/// Run the search now and publish the results.
pub(in crate::pages::book) fn run_search(state: BookState, store: AppStore) {
    let query = state.find_query.get();
    let opts = options(state);

    // Tell the highlighter what to paint before anything else: the selection
    // set below is applied through a transaction, and the decorations are
    // pulled during it.
    let shared = find::plugin::shared();
    shared.set_search(&query, opts);
    shared.set_active(state.find_open.get() && !query.is_empty());

    if query.is_empty() {
        state.find_results.set(Vec::new());
        state.find_current.set(None);
        shared.set_current(None);
        find::plugin::force_redraw(&state.chapter_handle.get());
        return;
    }

    let open_id = open_chapter_id(state);
    let book_scope = state.find_book_scope.get();
    let mut groups: Vec<ChapterHits> = Vec::new();
    for chapter in store.chapters.get() {
        let is_open = open_id.as_deref() == Some(chapter.id.as_str());
        if !book_scope && !is_open {
            continue;
        }
        let (hits, readable) = if is_open {
            (
                find::find_in_doc(&state.chapter_handle.get().doc(), &query, opts),
                true,
            )
        } else {
            match find::find_in_chapter_content(&chapter.content, &query, opts) {
                Some(hits) => (hits, true),
                None => (Vec::new(), false),
            }
        };
        if hits.is_empty() && readable {
            continue;
        }
        groups.push(ChapterHits {
            chapter_id: chapter.id,
            title: chapter.title,
            hits,
            readable,
        });
    }

    // A current hit that the new results no longer contain is not current any
    // more — keeping it would light up a row that has moved or gone.
    let still_valid = state.find_current.get().is_some_and(|(cid, idx)| {
        groups
            .iter()
            .any(|g| g.chapter_id == cid && idx < g.hits.len())
    });
    state.find_results.set(groups);
    if !still_valid {
        state.find_current.set(None);
    }
    sync_current_highlight(state);
}

/// Search after a short pause, so typing a word does not search the book once
/// per letter.
pub(in crate::pages::book) fn schedule_search(state: BookState, store: AppStore) {
    if let Some(handle) = state.find_debounce_timer_id.get() {
        rinch_core::clear_timeout(handle);
    }
    // `unowned`: the timer must outlive the element that scheduled it (see
    // `panes::typography`'s note — a callback parked during a render is dropped
    // when that scope is disposed, which is exactly when a pane switches).
    state
        .find_debounce_timer_id
        .set(Some(rinch_core::reactive::unowned(|| {
            rinch_core::set_timeout(DEBOUNCE_MS, move || {
                if !state.find_open.is_alive() {
                    return;
                }
                state.find_debounce_timer_id.set(None);
                run_search(state, store);
            })
        })));
}

/// Point the highlighter at the current hit — but only when that hit is in the
/// chapter the editor is actually showing.
fn sync_current_highlight(state: BookState) {
    let index = match (open_chapter_id(state), state.find_current.get()) {
        (Some(open), Some((cid, idx))) if open == cid => Some(idx),
        _ => None,
    };
    find::plugin::shared().set_current(index);
    find::plugin::force_redraw(&state.chapter_handle.get());
}

// ── Opening and closing ──────────────────────────────────────────────────────

/// Open the panel. `book_scope` is Ctrl+Shift+F; without it the search starts
/// scoped to the open chapter.
pub(in crate::pages::book) fn open(state: BookState, store: AppStore, book_scope: bool) {
    state
        .find_book_scope
        .set(book_scope || open_chapter_id(state).is_none());
    state.find_open.set(true);
    run_search(state, store);
    focus_query_field();
}

/// Close the panel, clear the highlights, and forget which hit was current.
pub(in crate::pages::book) fn close(state: BookState) {
    state.find_open.set(false);
    state.find_current.set(None);
    state.find_results.set(Vec::new());
    state.find_confirm.set(false);
    if let Some(handle) = state.find_debounce_timer_id.get() {
        rinch_core::clear_timeout(handle);
        state.find_debounce_timer_id.set(None);
    }
    let shared = find::plugin::shared();
    shared.set_active(false);
    shared.set_current(None);
    find::plugin::force_redraw(&state.chapter_handle.get());
}

/// Whether the panel is open — what the Escape handler asks before deciding
/// Escape belongs to it.
pub(in crate::pages::book) fn is_open(state: BookState) -> bool {
    state.find_open.get()
}

/// The id of the panel's query field, so the global key handler can tell a
/// keystroke aimed at the panel from one aimed at the prose.
pub(in crate::pages::book) const QUERY_INPUT_ID: &str = "find-query";
/// The replace field's id, for the same reason.
pub(in crate::pages::book) const REPLACE_INPUT_ID: &str = "find-replace";

/// Put the caret in the query field, with whatever is in it selected so the
/// next keystroke replaces the last search rather than extending it.
///
/// Tried twice, and both attempts are load-bearing. The `if find_open.get()`
/// that mounts the panel is a reactive effect, so by the time `open` returns the
/// input usually exists and can be focused *synchronously* — which matters,
/// because a keystroke arriving in the same task as the shortcut would otherwise
/// land nowhere. "Usually" is not "always" (nothing here promises the mount ran
/// inline), so a `setTimeout(0)` follows as the fallback. Focusing an
/// already-focused field is a no-op, so running both costs nothing.
fn focus_query_field() {
    crate::web_only! {
        use wasm_bindgen::JsCast;
        fn focus_now() {
            if let Some(doc) = crate::platform::document()
                && let Some(el) = doc.get_element_by_id(QUERY_INPUT_ID)
                && let Ok(input) = el.dyn_into::<web_sys::HtmlInputElement>()
            {
                input.focus().ok();
                input.select();
            }
        }
        focus_now();
        let closure = wasm_bindgen::closure::Closure::once(focus_now);
        if let Some(window) = crate::platform::window() {
            window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    closure.as_ref().unchecked_ref(),
                    0,
                )
                .ok();
        }
        closure.forget();
    }
}

// ── Navigating ───────────────────────────────────────────────────────────────

/// Move to the next (`+1`) or previous (`-1`) hit, wrapping at either end.
pub(in crate::pages::book) fn step(state: BookState, store: AppStore, delta: isize) {
    if busy(state) {
        return;
    }
    let flat = flat_hits(state);
    if flat.is_empty() {
        return;
    }
    let at = state
        .find_current
        .get()
        .and_then(|(cid, idx)| flat.iter().position(|h| h.0 == cid && h.1 == idx));
    let next = match at {
        Some(pos) => (pos as isize + delta).rem_euclid(flat.len() as isize) as usize,
        // Nothing current yet: Next starts at the first hit, Prev at the last.
        None if delta >= 0 => 0,
        None => flat.len() - 1,
    };
    let (cid, idx) = flat[next].clone();
    go_to(state, store, cid, idx);
}

/// Make hit `idx` of chapter `cid` the current one, opening that chapter first
/// if it is not the one on screen.
pub(in crate::pages::book) fn go_to(
    state: BookState,
    store: AppStore,
    chapter_id: String,
    index: usize,
) {
    if busy(state) {
        return;
    }
    state.find_current.set(Some((chapter_id.clone(), index)));
    if open_chapter_id(state).as_deref() == Some(chapter_id.as_str()) {
        select_current(state);
        return;
    }
    switch_to_chapter(state, store, &chapter_id);
    when_chapter_ready(
        state,
        chapter_id,
        0,
        Box::new(move || {
            // The chapter's live document may differ from the stored copy the
            // count came from (another device, or an unsaved edit), so the hits
            // are found again here rather than trusted from the results list.
            run_search(state, store);
            select_current(state);
        }),
    );
}

/// Select the current hit in the editor and scroll it into view.
fn select_current(state: BookState) {
    let Some((cid, index)) = state.find_current.get() else {
        return;
    };
    if open_chapter_id(state).as_deref() != Some(cid.as_str()) {
        return;
    }
    let handle = state.chapter_handle.get();
    let hits = find::find_in_doc(&handle.doc(), &state.find_query.get(), options(state));
    let Some(hit) = hits.get(index) else {
        return;
    };
    // Before the selection, not after: `set_selection` dispatches a transaction
    // and the view pulls its decorations during it, so the current-hit class
    // has to be set by then or the highlight lags one step behind.
    find::plugin::shared().set_current(Some(index));
    handle.set_selection(rinch_editor_core::Selection::text(hit.from, hit.to));
    scroll_current_hit_into_view();
}

/// Scroll the editor column so the current hit is on screen.
///
/// Web-only, and deliberately scrolls `.editor-scroll` rather than calling
/// `Element::scroll_into_view`: the latter walks every scrollable ancestor and
/// would move the workspace itself, which shifts the find panel out from under
/// the author's hand. rinch has no "reveal the selection" call of its own —
/// `scroll_to_text_in_editor` (this file's neighbour in `panes/editor.rs`) is
/// the existing precedent for reaching into the DOM for exactly this.
fn scroll_current_hit_into_view() {
    crate::web_only! {
        use wasm_bindgen::JsCast;
        let closure = wasm_bindgen::closure::Closure::once(move || {
            let Some(doc) = crate::platform::document() else { return };
            let Ok(Some(container)) = doc.query_selector(".editor-layout .editor-scroll") else {
                return;
            };
            let Ok(Some(hit)) = doc.query_selector("#editor-main .pm-search-hit-current") else {
                return;
            };
            let view = container.get_bounding_client_rect();
            let target = hit.get_bounding_client_rect();
            if target.top() >= view.top() && target.bottom() <= view.bottom() {
                return;
            }
            // Centre it in the column.
            let delta = target.top() - view.top() - (view.height() - target.height()) / 2.0;
            container.set_scroll_top(container.scroll_top() + delta as i32);
        });
        if let Some(window) = crate::platform::window() {
            window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    closure.as_ref().unchecked_ref(),
                    0,
                )
                .ok();
        }
        closure.forget();
    }
}

// ── Opening a chapter, and knowing when it is really open ────────────────────

/// Open `chapter_id` in the editor through the same path the chapters pane uses
/// — flush whatever prose surface is active, then switch.
fn switch_to_chapter(state: BookState, store: AppStore, chapter_id: &str) {
    super::super::flush::flush_pending_edits(state, store);
    super::chapters::do_switch_chapter(
        state.active_pane,
        state.save_alert,
        state.auto_save_timer_id,
        state.chapter_title_save_timer_id,
        state.save_status,
        state.editor_word_count,
        state.loaded_chapter_id,
        state.chapter_dirty,
        state.chapter_handle,
        state.chapter_title,
        store,
        &state.bid_signal.get(),
        chapter_id,
    );
}

/// Call `then` once `chapter_id`'s body is genuinely the editor's — see the
/// module header for why `loaded_chapter_id` alone is not that moment.
///
/// `then` is boxed rather than generic because this function calls itself: a
/// generic parameter would make every poll a fresh closure type and the
/// recursion would never finish monomorphising.
fn when_chapter_ready(state: BookState, chapter_id: String, attempt: u32, then: Box<dyn FnOnce()>) {
    if !state.find_open.is_alive() {
        return;
    }
    let ready = state.loaded_chapter_id.get().as_deref() == Some(chapter_id.as_str())
        && crate::local_store::body_is_open(&format!("chapter:{chapter_id}"));
    if ready {
        then();
        return;
    }
    if attempt >= ATTACH_MAX_POLLS {
        // The local document store is unavailable (private browsing with
        // storage blocked, say). Going ahead is still better than stalling: the
        // edit reaches the editor and the REST save, it just is not backed by a
        // local CRDT — which is the same position every edit in that browser is
        // in already.
        log::warn!("find: chapter {chapter_id} never attached its body; replacing anyway");
        then();
        return;
    }
    rinch_core::reactive::unowned(|| {
        rinch_core::set_timeout(ATTACH_POLL_MS, move || {
            when_chapter_ready(state, chapter_id, attempt + 1, then);
        })
    });
}

// ── Replacing ────────────────────────────────────────────────────────────────

/// Mark the open chapter edited, so the autosave and every save-on-leave path
/// treats the replacement the way they treat typing, and refresh the stored copy
/// the whole-book search reads.
fn after_replacing(state: BookState, store: AppStore, chapter_id: &str) {
    state.chapter_dirty.set(true);
    state.save_status.set("unsaved");
    state
        .editor_word_count
        .set(editor_utils::editor_word_count(&state.chapter_handle.get()));

    // `store.chapters` carries the body the *book load* fetched, which is what
    // every unopened chapter is searched from. Leaving it behind would have the
    // panel go on reporting hits in a chapter it had just cleaned.
    let Some(content) = editor_utils::editor_content_json(&state.chapter_handle.get()) else {
        return;
    };
    let id = chapter_id.to_string();
    let body = content.clone();
    store.chapters.update(|chapters| {
        if let Some(ch) = chapters.iter_mut().find(|c| c.id == id) {
            ch.content = body.clone();
        }
    });
    // And the snapshot `local_book`'s projection re-paints `store.chapters`
    // from, or the next projection would put the old body back.
    crate::local_book::rest_chapters(&state.bid_signal.get(), |chapters| {
        if let Some(ch) = chapters.iter_mut().find(|c| c.id == id) {
            ch.content = content.clone();
        }
    });
}

/// Replace the current hit, then move to the next one.
pub(in crate::pages::book) fn replace_current(state: BookState, store: AppStore) {
    if busy(state) {
        return;
    }
    let Some((cid, index)) = state.find_current.get() else {
        // Nothing selected yet: Replace means "the first one".
        step(state, store, 1);
        return;
    };
    if open_chapter_id(state).as_deref() != Some(cid.as_str()) {
        return;
    }
    let handle = state.chapter_handle.get();
    let hits = find::find_in_doc(&handle.doc(), &state.find_query.get(), options(state));
    let Some(hit) = hits.get(index) else {
        return;
    };
    if !find::replace_one(&handle, hit.from, hit.to, &state.find_replace.get()) {
        return;
    }
    after_replacing(state, store, &cid);
    // The hit that was at `index` is gone, so what was `index + 1` is now
    // `index` — leaving the pointer where it is lands on the next occurrence.
    state.find_current.set(Some((cid, index)));
    run_search(state, store);
    select_current(state);
}

/// Replace every hit in the open chapter, in one undo step.
pub(in crate::pages::book) fn replace_all_in_chapter(state: BookState, store: AppStore) -> usize {
    if busy(state) {
        return 0;
    }
    let Some(cid) = open_chapter_id(state) else {
        return 0;
    };
    let replaced = find::replace_all(
        &state.chapter_handle.get(),
        &state.find_query.get(),
        options(state),
        &state.find_replace.get(),
    );
    if replaced == 0 {
        return 0;
    }
    after_replacing(state, store, &cid);
    state.find_current.set(None);
    run_search(state, store);
    replaced
}

/// Replace every hit in every chapter, one chapter at a time.
///
/// Each chapter is opened, waited for, replaced, and left — and *leaving* is
/// what persists it, because every exit flushes the prose surface it is leaving
/// (`super::super::flush`). The last chapter has no exit, so the walk flushes
/// explicitly when it finishes rather than trusting the 3s autosave.
pub(in crate::pages::book) fn replace_all_in_book(state: BookState, store: AppStore) {
    if busy(state) {
        return;
    }
    let targets: Vec<String> = state
        .find_results
        .get()
        .iter()
        .filter(|g| !g.hits.is_empty())
        .map(|g| g.chapter_id.clone())
        .collect();
    if targets.is_empty() {
        return;
    }
    // Where the author was, so they are put back rather than dumped in whatever
    // chapter happened to be last.
    let origin = match state.active_pane.get() {
        BookPane::Editor(id) => Some(id),
        _ => None,
    };
    replace_step(state, store, targets, 0, origin);
}

fn replace_step(
    state: BookState,
    store: AppStore,
    targets: Vec<String>,
    at: usize,
    origin: Option<String>,
) {
    if !state.find_open.is_alive() {
        return;
    }
    if at >= targets.len() {
        finish_replace_all(state, store, origin);
        return;
    }
    state.find_busy.set(Some(format!(
        "Replacing… {}/{} chapters",
        at + 1,
        targets.len()
    )));

    let chapter_id = targets[at].clone();
    if open_chapter_id(state).as_deref() != Some(chapter_id.as_str()) {
        switch_to_chapter(state, store, &chapter_id);
    }
    let next_id = chapter_id.clone();
    when_chapter_ready(
        state,
        chapter_id,
        0,
        Box::new(move || {
            let replaced = find::replace_all(
                &state.chapter_handle.get(),
                &state.find_query.get(),
                options(state),
                &state.find_replace.get(),
            );
            if replaced > 0 {
                after_replacing(state, store, &next_id);
            }
            replace_step(state, store, targets, at + 1, origin);
        }),
    );
}

fn finish_replace_all(state: BookState, store: AppStore, origin: Option<String>) {
    // The last chapter was never left, so nothing has flushed it.
    super::super::flush::flush_pending_edits(state, store);
    if let Some(origin) = origin
        && open_chapter_id(state).as_deref() != Some(origin.as_str())
    {
        switch_to_chapter(state, store, &origin);
    }
    state.find_busy.set(None);
    state.find_current.set(None);
    // Re-ask the question. A replace-all that worked answers zero.
    run_search(state, store);
}

// ── Rendering ────────────────────────────────────────────────────────────────

/// One result row: the chapter's snippet with the match emboldened.
fn hit_row(
    __scope: &mut RenderScope,
    state: BookState,
    store: AppStore,
    chapter_id: String,
    index: usize,
    hit: Hit,
) -> NodeHandle {
    let (before, matched, after) = hit.snippet_parts();
    let (before, matched, after) = (before.to_string(), matched.to_string(), after.to_string());
    // One clone per closure: both are `move`, and a shared non-`Copy` capture
    // would be moved into whichever the macro expands first.
    let for_class = chapter_id.clone();
    let for_click = chapter_id;
    rsx! {
        button {
            class: {move || {
                let current = state
                    .find_current
                    .get()
                    .is_some_and(|(cid, idx)| cid == for_class && idx == index);
                if current { "find-hit is-current" } else { "find-hit" }
            }},
            onclick: move || go_to(state, store, for_click.clone(), index),
            span { class: "find-hit-before", {before.clone()} }
            span { class: "find-hit-match", {matched.clone()} }
            span { class: "find-hit-after", {after.clone()} }
        }
    }
}

/// What the panel says next to the query field: "3 of 12", "12 matches" when
/// nothing is current yet, or "No matches".
///
/// A free function rather than a closure in `render`, so the rsx node can be
/// `{move || position_label(state)}` — a closure that merely forwards to
/// another closure is one clippy (rightly) objects to.
fn position_label(state: BookState) -> String {
    if state.find_query.get().is_empty() {
        return String::new();
    }
    let total = total_hits(state);
    if total == 0 {
        return "No matches".to_string();
    }
    let flat = flat_hits(state);
    match state
        .find_current
        .get()
        .and_then(|(cid, idx)| flat.iter().position(|h| h.0 == cid && h.1 == idx))
    {
        Some(pos) => format!("{} of {}", pos + 1, total),
        None if total == 1 => "1 match".to_string(),
        None => format!("{total} matches"),
    }
}

/// One chapter's group of results: its title, its count, and a row per hit.
///
/// A function rather than markup inline in the `for` body, because both header
/// labels are values computed from the group — and a `{if …}` in node position
/// is reactive *markup* to rsx, not a reactive value. Computing them here, once
/// per group, is both what the macro wants and what the list needs: a group's
/// title does not change while it is on screen.
fn group_block(
    __scope: &mut RenderScope,
    state: BookState,
    store: AppStore,
    group: ChapterHits,
) -> NodeHandle {
    let title = if group.title.trim().is_empty() {
        "Untitled".to_string()
    } else {
        group.title.clone()
    };
    let count = if group.readable {
        group.hits.len().to_string()
    } else {
        "legacy format — not searched".to_string()
    };
    let chapter_id = group.chapter_id.clone();
    rsx! {
        div { key: {group.chapter_id.clone()}, class: "find-group",
            div { class: "find-group-head",
                span { class: "find-group-title", {title.clone()} }
                span { class: "find-group-count", {count.clone()} }
            }
            for (index, hit) in group.hits.iter().cloned().enumerate() {
                {hit_row(__scope, state, store, chapter_id.clone(), index, hit)}
            }
        }
    }
}

/// One toggle in the options row.
fn toggle(
    __scope: &mut RenderScope,
    state: BookState,
    label: &'static str,
    hint: &'static str,
    signal: Signal<bool>,
    on_change: impl Fn() + 'static + Copy,
) -> NodeHandle {
    rsx! {
        button {
            class: {move || if signal.get() { "find-toggle is-on" } else { "find-toggle" }},
            title: hint,
            onclick: move || {
                if busy(state) {
                    return;
                }
                signal.update(|v| *v = !*v);
                on_change();
            },
            {label}
        }
    }
}

/// Render the find panel (a fixed overlay; present in the DOM only while open).
pub(in crate::pages::book) fn render(
    __scope: &mut RenderScope,
    state: BookState,
    store: AppStore,
) -> NodeHandle {
    let BookState {
        find_open,
        find_query,
        find_replace,
        find_match_case,
        find_whole_word,
        find_book_scope,
        find_results,
        find_busy,
        find_confirm,
        ..
    } = state;

    let rerun = move || schedule_search(state, store);
    // Every re-search the *author* asks for is refused mid-walk, for the reason
    // on `busy`. The walk's own final re-search calls `run_search` directly.
    let rerun_now = move || {
        if !busy(state) {
            run_search(state, store)
        }
    };

    rsx! {
        Fragment {
            if find_open.get() {
                div {
                    // `is-busy` is the *visible* half of the guard every button
                    // already applies: a whole-book replace switches chapters
                    // underneath the panel, so its controls stop working while
                    // one runs, and a control that silently stops working is
                    // worse than one that says so.
                    class: {move || if find_busy.get().is_some() { "find-panel is-busy" } else { "find-panel" }},
                    id: "find-panel",

                    div { class: "find-row find-row-query",
                        {render_tabler_icon(__scope, TablerIcon::Search, TablerIconStyle::Outline)}
                        input {
                            id: QUERY_INPUT_ID,
                            class: "find-input",
                            r#type: "text",
                            autocomplete: "off",
                            placeholder: "Find in book",
                            value: {move || find_query.get()},
                            oninput: move |v: String| {
                                if busy(state) {
                                    return;
                                }
                                find_query.set(v);
                                rerun();
                            },
                        }
                        span { class: "find-count", {move || position_label(state)} }
                        button {
                            class: "find-step",
                            title: "Previous match (Shift+Enter)",
                            onclick: move || step(state, store, -1),
                            {render_tabler_icon(__scope, TablerIcon::ChevronUp, TablerIconStyle::Outline)}
                        }
                        button {
                            class: "find-step",
                            title: "Next match (Enter)",
                            onclick: move || step(state, store, 1),
                            {render_tabler_icon(__scope, TablerIcon::ChevronDown, TablerIconStyle::Outline)}
                        }
                        button {
                            class: "find-step",
                            title: "Close (Esc)",
                            onclick: move || close(state),
                            {render_tabler_icon(__scope, TablerIcon::X, TablerIconStyle::Outline)}
                        }
                    }

                    div { class: "find-row find-row-replace",
                        {render_tabler_icon(__scope, TablerIcon::Replace, TablerIconStyle::Outline)}
                        input {
                            id: REPLACE_INPUT_ID,
                            class: "find-input",
                            r#type: "text",
                            autocomplete: "off",
                            placeholder: "Replace with",
                            value: {move || find_replace.get()},
                            oninput: move |v: String| find_replace.set(v),
                        }
                        button {
                            class: "find-action",
                            onclick: move || replace_current(state, store),
                            "Replace"
                        }
                        button {
                            class: "find-action",
                            onclick: move || {
                                replace_all_in_chapter(state, store);
                            },
                            "All in chapter"
                        }
                    }

                    div { class: "find-row find-row-options",
                        {toggle(__scope, state, "Aa", "Match case", find_match_case, rerun_now)}
                        {toggle(__scope, state, "Ab|", "Whole word", find_whole_word, rerun_now)}
                        div { class: "find-scope",
                            button {
                                class: {move || if find_book_scope.get() { "find-scope-option" } else { "find-scope-option is-on" }},
                                onclick: move || {
                                    if busy(state) {
                                        return;
                                    }
                                    find_book_scope.set(false);
                                    rerun_now();
                                },
                                "This chapter"
                            }
                            button {
                                class: {move || if find_book_scope.get() { "find-scope-option is-on" } else { "find-scope-option" }},
                                onclick: move || {
                                    if busy(state) {
                                        return;
                                    }
                                    find_book_scope.set(true);
                                    rerun_now();
                                },
                                "Whole book"
                            }
                        }
                    }

                    if find_busy.get().is_some() {
                        div { class: "find-progress", {move || find_busy.get().unwrap_or_default()} }
                    }

                    div { class: "find-results",
                        for group in find_results.get() {
                            {group_block(__scope, state, store, group)}
                        }
                    }

                    if find_book_scope.get() {
                        div { class: "find-footer",
                            button {
                                class: "find-action find-action-wide",
                                onclick: move || {
                                    if !busy(state) && total_hits(state) > 0 {
                                        find_confirm.set(true);
                                    }
                                },
                                "Replace all in book"
                            }
                        }
                    }
                }
            }

            // Tier 2 — the destructive confirm. A whole-book replace touches
            // chapters the author is not looking at, so it says how many before
            // it does.
            Dialog {
                opened_fn: move || find_confirm.get(),
                onclose: move || find_confirm.set(false),
                title: "Replace all in book?",
                danger: true,
                confirm_label: "Replace all",
                onconfirm: move || {
                    find_confirm.set(false);
                    replace_all_in_book(state, store);
                },

                div {
                    {move || {
                        let hits = total_hits(state);
                        let chapters = find_results.get().iter().filter(|g| !g.hits.is_empty()).count();
                        let replacement = find_replace.get();
                        let with = if replacement.is_empty() {
                            "delete them".to_string()
                        } else {
                            format!("replace them with \"{replacement}\"")
                        };
                        format!(
                            "{hits} {} in {chapters} {} — {with}.",
                            if hits == 1 { "match" } else { "matches" },
                            if chapters == 1 { "chapter" } else { "chapters" },
                        )
                    }}
                }
                span { class: "pw-dialog-warning",
                    "Each chapter is opened and saved as it goes. One undo per chapter."
                }
            }
        }
    }
}
