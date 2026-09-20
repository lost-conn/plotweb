//! One question, asked in one place: **is this body actually going anywhere?**
//!
//! ## The invisible-write bug class
//!
//! In a cut-over book the canonical document is the source of truth and the sync
//! engine is the *only* writer of the body — the REST save deliberately withholds
//! `content` (see `book_page`'s `sends_body_content`), because two writers on one
//! body is how deleted text comes back. That makes the CRDT the single seam every
//! keystroke must pass through, and it makes any refusal at that seam total.
//!
//! rinch's collab projection is staged: some content (a table, a blockquote wrap)
//! cannot be projected onto the CRDT. When it can't, outbound **stalls** — that edit
//! and every edit after it stays in this tab until the offending content is removed.
//! Nothing errors: the REST PUT still goes out, still carries no body, still comes
//! back durable, and the footer still says "Saved". The author writes for an hour
//! into a document that only exists in one browser tab, watching an indicator that
//! agrees with them the whole time.
//!
//! rinch PR #838 shrank the reachable set — inline atoms (`hard_break` from
//! Shift+Enter, `image`) now project — but blockquote and tables remain out of
//! scope, so the failure mode is still reachable. It cannot be made impossible here;
//! it can be made **honest**. That is all this module does.
//!
//! ## Two ways a body stops syncing
//!
//! 1. **Mid-session**: an edit could not be projected. `EditorHandle` reports it as
//!    a *state* — [`EditorHandle::collab_outbound_stall`] stays `Some` for as long as
//!    the content is in the document and clears itself the moment an edit projects
//!    again (rinch then broadcasts the whole backlog at once). Consulted after every
//!    edit; it is a `try_borrow` and a clone, cheap enough for a per-keystroke path.
//! 2. **At seed time**: the document could not start a collaboration session at all,
//!    so `local_store::seed_and_host_kind` fell back to REST-only. Recorded per
//!    surface — see [`crate::local_store::seed_stall`].
//!
//! Either way the answer is the same and the footer says the same thing:
//! **"Unsaved — unsupported content"**.

use rinch_core::Signal;

use crate::local_store::BodyKind;
use crate::rinch_backend::EditorHandle;

use std::cell::RefCell;

thread_local! {
    /// The stall text most recently logged for each surface, so a stall is logged
    /// once rather than once per keystroke — the condition can persist for the whole
    /// time the author keeps typing into the unsupported block.
    static LAST_LOGGED: RefCell<[Option<String>; 2]> = const { RefCell::new([None, None]) };
}

fn slot(kind: BodyKind) -> usize {
    match kind {
        BodyKind::Chapter => 0,
        BodyKind::Note => 1,
    }
}

/// Why this surface's body is not reaching the server, if it isn't — mid-session
/// stall first, then the seed-time refusal.
///
/// `None` for a book where sync does not carry the body: there the REST PUT sends
/// `content` and a collab refusal costs nothing but the local-first cache.
fn reason(kind: BodyKind, handle: &EditorHandle, book_id: &str) -> Option<String> {
    if !crate::sync::enabled_for_book(book_id) {
        return None;
    }
    handle
        .collab_outbound_stall()
        .map(|e| e.to_string())
        .or_else(|| crate::local_store::seed_stall(kind))
}

/// Consult the surface, and if the body is stalled put the save indicator in the one
/// state that is true — `"unsupported"`, rendered as "Unsaved — unsupported content".
///
/// Returns `true` when stalled, which every save path takes as a veto: letting the
/// REST round-trip complete would flip the indicator to `"saved"`, and for a cut-over
/// book that write carried no body at all. A refused save deliberately leaves the
/// surface *dirty*, so the text is still pending for whichever flush runs after the
/// author removes the unsupported content.
pub(in crate::pages::book) fn veto_if_stalled(
    kind: BodyKind,
    handle: &EditorHandle,
    book_id: &str,
    save_status: Signal<&'static str>,
) -> bool {
    let i = slot(kind);
    match reason(kind, handle, book_id) {
        Some(text) => {
            // The error names the node type, which is the only actionable part of
            // this for whoever reads the console.
            let fresh = LAST_LOGGED.with(|l| {
                let mut l = l.borrow_mut();
                if l[i].as_deref() == Some(text.as_str()) {
                    false
                } else {
                    l[i] = Some(text.clone());
                    true
                }
            });
            if fresh {
                log::warn!(
                    "body sync stalled — this edit is not reaching the server and will \
                     not until the content is removed: {text}"
                );
            }
            save_status.set("unsupported");
            true
        }
        None => {
            // Cleared, so a later recurrence logs again rather than being swallowed
            // as a duplicate of a stall the author already resolved.
            LAST_LOGGED.with(|l| l.borrow_mut()[i] = None);
            false
        }
    }
}
