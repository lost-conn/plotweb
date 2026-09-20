//! Save-on-leave for every prose surface in the book workspace.
//!
//! Both editors autosave on a debounce (3s for a chapter, 800ms for a note), so at any
//! moment up to that much typing exists only in the editor model. Whatever ends the
//! editing session — a sidebar entry, a back arrow, opening another document, signing
//! out — has to write that pending text out *before* it changes `active_pane`, because
//! both debounce callbacks check the pane and skip the save once it has moved on. Miss
//! one exit and the author loses their last few seconds of writing with no indication
//! that anything happened.
//!
//! PR #62 fixed this for the chapter editor with a `flush_editor_if_active` closure
//! called from each exit, and the note editor — added later, with its own timer and its
//! own dirty flag — was never added to it. The fix is the same one generalised: a single
//! [`flush_pending_edits`] that dispatches on the active pane, so an exit calls *one*
//! thing and a new prose surface is a new match arm here rather than a new call to add
//! at every exit. That is the property that keeps a third editor from regressing this
//! the same way: exits no longer name a specific editor.
//!
//! Every arm has the same four-step shape, and each step is load-bearing:
//!
//! 1. **Cancel the pending debounce timer** — it would otherwise fire against a pane
//!    that has already moved on and, worse, re-save stale model content to whatever
//!    document is open by then.
//! 2. **Only save if the model holds *this* document** (`loaded_*_id` == the pane's id).
//!    During the fetch window of a switch the model still holds the previous document;
//!    writing it under the new id is the document-crosstalk bug.
//! 3. **Only save if it was actually edited** (`*_dirty`). Saving a document that was
//!    merely opened rewrites it with whatever the editor was handed — which is exactly
//!    how a note lost a paragraph in production (see `state.rs`'s `chapter_dirty` docs):
//!    it was opened, showed a blank canonical copy of a diverged document, and the walk
//!    away wrote that blank over git.
//! 4. **Clear the dirty flag only once the write is actually issued** — never before.
//!    The old `save_note_content` cleared it at the top and *then* checked the pane, so
//!    a save that never happened still left the surface looking clean: the edit was
//!    unrecoverable even though a later exit would have flushed it.

use rinch_core::Signal;

use crate::store::AppStore;

use super::panes;
use super::stall;
use super::state::BookState;
use super::BookPane;

/// Whether this book's bodies travel by sync rather than by this PUT.
///
/// Mirrors `book_page`'s `sends_body_content`, inverted: a cut-over book's canonical
/// document is the source of truth and a whole-state PUT could only be a stale duplicate
/// of the ops sync already sent.
fn cut_over(store: AppStore) -> bool {
    store.current_book.get().map(|b| b.cutover).unwrap_or(false)
}

/// Decide whether `target` may be written from the shared editor model, and if so mark
/// the surface clean. Returns `true` exactly when the caller should go on to write.
///
/// Both arms share this because both need the identical two guards in the identical
/// order, and getting that order wrong is the whole bug: the old note path cleared
/// `dirty` *before* deciding, so a skipped write still marked the note clean and the
/// author's text became unrecoverable. Here `dirty` is cleared only on the `true` path —
/// a skip always leaves the edit pending so a later, better-placed flush can still save
/// it.
///
/// Kept signal-only (no editor handle, no network) so this — the part that regressed —
/// is unit-testable on the host target.
fn claim_write(loaded: Signal<Option<String>>, dirty: Signal<bool>, target: &str) -> bool {
    // The model is shared across documents of this kind. Mid-switch it still holds the
    // one we are leaving, so writing it under `target` would overwrite a different
    // document with this one's text.
    if loaded.get().as_deref() != Some(target) {
        return false;
    }
    // Never rewrite a document that was only looked at: that turns a failed or diverged
    // load into a destructive save (see the module header).
    if !dirty.get() {
        return false;
    }
    dirty.set(false);
    true
}

/// Write out any pending debounced edit on whichever prose surface is active.
///
/// Call this from *every* exit, before the pane changes. Safe to call when nothing is
/// pending or no editor is open — it no-ops.
pub(in crate::pages::book) fn flush_pending_edits(state: BookState, store: AppStore) {
    match state.active_pane.get() {
        BookPane::Editor(id) => flush_chapter(state, store, id),
        BookPane::NoteEditor(id) => flush_note(state, store, id),
        _ => {}
    }
}

/// The chapter half — PR #62's `flush_editor_if_active`, plus the dirty guard that
/// `do_switch_chapter_inner` already applied to the very same save-on-leave write.
fn flush_chapter(state: BookState, store: AppStore, chapter_id: String) {
    if let Some(h) = state.auto_save_timer_id.get() {
        rinch_core::clear_timeout(h);
        state.auto_save_timer_id.set(None);
    }
    // Step 2.5, between "may this be written" and "write it": *can* it be written?
    // In a cut-over book the PUT below carries no body, so with sync stalled this
    // would report a save that persisted nothing. Asked before `claim_write` so the
    // surface stays dirty — this is exactly the "a refused write never marks the
    // surface clean" rule the module header is about, applied to a new refusal.
    if stall::veto_if_stalled(
        crate::local_store::BodyKind::Chapter,
        &state.chapter_handle.get(),
        &state.bid_signal.get(),
        state.save_status,
    ) {
        return;
    }
    if !claim_write(state.loaded_chapter_id, state.chapter_dirty, &chapter_id) {
        return;
    }
    let Some(content) = crate::pages::editor_utils::editor_content_json(&state.chapter_handle.get())
    else {
        return;
    };
    state.save_status.set("saving");
    panes::chapters::save_chapter_body(
        format!(
            "/api/books/{}/chapters/{}",
            state.bid_signal.get(),
            chapter_id
        ),
        content,
        cut_over(store),
        state.save_status,
        state.save_alert,
    );
}

/// The note half — the arm that did not exist, and whose absence meant every exit
/// except the back arrow discarded the last ~800ms of a note.
///
/// This is also the body of the debounced save itself (`save_note_content` calls
/// straight through), so the timer path and the leave path cannot drift apart: there is
/// one place that decides whether a note may be written, and one place that clears
/// `note_dirty`.
fn flush_note(state: BookState, store: AppStore, note_id: String) {
    if let Some(h) = state.note_save_timer_id.get() {
        rinch_core::clear_timeout(h);
        state.note_save_timer_id.set(None);
    }
    // The note half of the same veto — see `flush_chapter`.
    if stall::veto_if_stalled(
        crate::local_store::BodyKind::Note,
        &state.note_handle.get(),
        &state.bid_signal.get(),
        state.note_save_status,
    ) {
        return;
    }
    if !claim_write(state.loaded_note_id, state.note_dirty, &note_id) {
        return;
    }
    let content =
        crate::pages::editor_utils::editor_content_json(&state.note_handle.get()).unwrap_or_default();
    let bid = state.bid_signal.get();
    let title_val = state.note_editor_title.get();
    let color_val = state.note_editor_color.get();

    // Local-first: mirror the rename/recolor into the `book:` doc's note titles/colors
    // Maps (structure decoupled), beside the REST PUT below.
    crate::local_book::note_meta(&bid, &note_id, Some(&title_val), color_val.as_deref());
    // And the link index derived from the body just serialized. It lives in the `book:`
    // doc so the timeline can be drawn without opening every note, and it is refreshed
    // here because this device is the only one holding the new text until sync carries it.
    crate::local_book::note_links(&bid, &note_id, &content);

    state.note_save_status.set("saving");
    panes::note_editor::save_note_body(
        format!("/api/books/{}/notes/{}", bid, note_id),
        title_val,
        content,
        color_val,
        cut_over(store),
        state.note_save_status,
        state.save_alert,
    );
}

/// `claim_write` is the whole of the fix that can be tested without a browser: the two
/// guards, their order, and — the part that lost a writing session — the rule that a
/// refused write never marks the surface clean.
///
/// These run against the note signals by name, but `claim_write` is the same code the
/// chapter arm calls, so they pin the behaviour for every prose surface at once.
#[cfg(test)]
mod claim_write_tests {
    use super::claim_write;
    use rinch_core::Signal;

    fn open(id: &str, dirty: bool) -> (Signal<Option<String>>, Signal<bool>) {
        (Signal::new(Some(id.to_string())), Signal::new(dirty))
    }

    /// The ordinary case every exit depends on: the note the model holds was edited, so
    /// it is written and only then marked clean.
    #[test]
    fn an_edited_note_is_written_and_then_marked_clean() {
        let (loaded, dirty) = open("note-a", true);
        assert!(claim_write(loaded, dirty, "note-a"));
        assert!(!dirty.get(), "dirty is cleared once the write is committed to");
    }

    /// The regression. The old `save_note_content` cleared `note_dirty` at the top and
    /// *then* checked the pane, so leaving via the sidebar left the note clean with the
    /// edit never sent — and because it looked clean, no later flush could rescue it.
    /// A refused write must leave the edit pending.
    #[test]
    fn a_refused_write_leaves_the_edit_pending() {
        // Refused because the model holds a different note (mid-switch).
        let (loaded, dirty) = open("note-a", true);
        assert!(!claim_write(loaded, dirty, "note-b"));
        assert!(
            dirty.get(),
            "a skipped write must not mark the surface clean — that is how the edit \
             became unrecoverable"
        );

        // Still pending, so the correctly-targeted flush that follows saves it.
        assert!(claim_write(loaded, dirty, "note-a"));
    }

    /// The crosstalk guard: during the fetch window of a note switch the model still
    /// holds the outgoing note, and writing it under the incoming id would overwrite a
    /// note the author never touched.
    #[test]
    fn a_note_still_loading_is_never_written() {
        let loaded: Signal<Option<String>> = Signal::new(None);
        let dirty = Signal::new(true);
        assert!(!claim_write(loaded, dirty, "note-b"));
        assert!(dirty.get());
    }

    /// Opening a note and walking away must not rewrite it. This is the production
    /// note-loss guard documented on `state.rs`'s dirty flags: a blank canonical copy of
    /// a diverged document was written over git by a save-on-leave that never asked
    /// whether anything had been typed.
    #[test]
    fn a_note_merely_opened_is_never_written() {
        let (loaded, dirty) = open("note-a", false);
        assert!(!claim_write(loaded, dirty, "note-a"));
    }

    /// Flushing is idempotent, which is what lets exits layer safely — `open_chapter`
    /// flushes and then `do_switch_chapter` runs its own save-on-leave, and exactly one
    /// write goes out.
    #[test]
    fn a_second_flush_with_no_new_edits_writes_nothing() {
        let (loaded, dirty) = open("note-a", true);
        assert!(claim_write(loaded, dirty, "note-a"));
        assert!(
            !claim_write(loaded, dirty, "note-a"),
            "nothing was typed in between, so there is nothing to write"
        );
    }
}
