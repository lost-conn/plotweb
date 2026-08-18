pub mod auth;
pub mod beta;
pub mod books;
pub mod chapters;
pub mod export;
pub mod fonts;
pub mod history;
pub mod images;
pub mod import;
pub mod notes;
pub mod sync;

use crate::rhype::quote;
use crate::AppState;

/// Verify a book (by its UUID) belongs to the given user. Replaces the
/// `SELECT COUNT(*) FROM books WHERE id=? AND user_id=?` check that was
/// duplicated across the chapter/note/image/import/history routes.
pub async fn verify_book_ownership(state: &AppState, book_id: &str, user_id: &str) -> bool {
    let q = format!(
        "Book.filter(.uuid == {} && .user_id == {}).limit(1)",
        quote(book_id),
        quote(user_id)
    );
    state.rhype.exists(q).await.unwrap_or(false)
}

// ── Cutover (phase E) ──────────────────────────────────────────────
//
// For a book that has been cut over, the canonical document is the source of truth and
// git is the mirror. Both helpers are no-ops for every other book, so the paths below
// read exactly as they did before for anything not cut over.

/// What a cut-over book should do with a body read.
pub enum CutoverRead {
    /// Not cut over, or no usable canonical copy: serve git's content as before.
    Git,
    /// Serve the canonical document's content.
    Canonical(String),
    /// The two copies disagree. Refuse the read.
    Locked(String),
}

/// Decide how to serve a body for a cut-over book.
///
/// Falling back to git when the canonical copy is missing or unreadable is deliberate:
/// slightly older content is recoverable, while an error or an empty body looks to an
/// author exactly like losing a chapter.
///
/// **Disagreement is different from absence, and gets refused.** There is no safe side
/// to serve when the two copies differ: hand over git's and an edit overwrites whatever
/// the canonical copy held; hand over the canonical's and an edit overwrites git — which
/// is how a note lost a paragraph the first day this flag was on. Refusing means nobody
/// authors from an ambiguous base, and the shadow report already names the document so
/// it can be reconciled deliberately.
pub fn cutover_body(
    state: &AppState,
    book_id: &str,
    doc_id: &str,
    git_content: &str,
    kind: plotweb_crdt::BodyKind,
) -> CutoverRead {
    if !state.cutover.is_cut_over(book_id) {
        return CutoverRead::Git;
    }
    let bytes = match crate::sync::canonical_snapshot(&state.crdt_dir, doc_id) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            eprintln!("[cutover] {doc_id}: no canonical copy, serving git");
            return CutoverRead::Git;
        }
        Err(e) => {
            eprintln!("[cutover] {doc_id}: store read failed, serving git: {e}");
            return CutoverRead::Git;
        }
    };

    match plotweb_crdt::compare_body(git_content, &bytes, kind) {
        plotweb_crdt::Shadow::Match => match plotweb_crdt::materialize_body(&bytes) {
            Ok(content) => CutoverRead::Canonical(content),
            Err(e) => {
                eprintln!("[cutover] {doc_id}: canonical unreadable, serving git: {e}");
                CutoverRead::Git
            }
        },
        plotweb_crdt::Shadow::Diverged { detail } => {
            eprintln!("[cutover] {doc_id}: LOCKED — copies disagree: {detail}");
            CutoverRead::Locked(detail)
        }
        plotweb_crdt::Shadow::Unreadable { reason } => {
            eprintln!("[cutover] {doc_id}: canonical unreadable, serving git: {reason}");
            CutoverRead::Git
        }
    }
}

/// Apply a REST write into the canonical document of a cut-over book.
///
/// Serialized on the same per-document lock sync uses, so a save and an exchange cannot
/// interleave in the middle of a read-modify-write. Failure is logged, not surfaced:
/// git already has the content, so the author's save succeeded even if the mirror is
/// briefly ahead — and the shadow pass will report the difference.
pub async fn apply_cutover_body(
    state: &AppState,
    book_id: &str,
    doc_id: &str,
    doc_type: &str,
    content: &str,
    kind: plotweb_crdt::BodyKind,
) {
    if !state.cutover.is_cut_over(book_id) {
        return;
    }
    let lock = state.doc_locks.for_doc(doc_id);
    let _guard = lock.lock().await;

    let crdt_dir = state.crdt_dir.clone();
    let doc_id_owned = doc_id.to_string();
    let doc_type = doc_type.to_string();
    let content = content.to_string();
    let applied = tokio::task::spawn_blocking(move || {
        crate::sync::apply_body_content(&crdt_dir, &doc_id_owned, &doc_type, &content, kind)
    })
    .await;

    match applied {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => eprintln!("[cutover] {doc_id}: could not apply the write: {e}"),
        Err(e) => eprintln!("[cutover] {doc_id}: apply worker panicked: {e}"),
    }
}

/// Record a structure change — create, rename, reorder, delete, move, retitle — into
/// the canonical `book:` document of a cut-over book.
///
/// Callers pass no delta. The route has just written git, so git *is* the intended
/// structure; this re-reads it and lets `plotweb_crdt::apply_book_structure` work out
/// what differs. That is deliberate: a dozen routes computing their own deltas is a
/// dozen chances to describe a move as a delete-and-insert, and a wrong delta on a
/// structure document loses a chapter rather than a character.
///
/// Cost is a book read per structure change. Those are rare — creating, renaming,
/// reordering — and the frequent path, an autosave of a chapter body, does not come
/// through here.
pub async fn apply_cutover_structure(state: &AppState, book_id: &str) {
    if !state.cutover.is_cut_over(book_id) {
        return;
    }
    let Some(input) = crate::structure::read_structure_input(&state.books, book_id).await else {
        eprintln!("[cutover] book:{book_id}: no readable structure in git, nothing applied");
        return;
    };

    let doc_id = format!("book:{book_id}");
    let lock = state.doc_locks.for_doc(&doc_id);
    let _guard = lock.lock().await;

    let crdt_dir = state.crdt_dir.clone();
    let book = book_id.to_string();
    let applied =
        tokio::task::spawn_blocking(move || crate::sync::apply_structure(&crdt_dir, &book, &input))
            .await;

    match applied {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => eprintln!("[cutover] {doc_id}: could not apply the structure change: {e}"),
        Err(e) => eprintln!("[cutover] {doc_id}: structure apply worker panicked: {e}"),
    }
}

// ── Cascade deletes ────────────────────────────────────────────────
// SQLite enforced ON DELETE CASCADE (book → links → feedback → replies).
// rhypedb has no relations/cascade, so we delete children explicitly.

/// Delete all replies under a feedback, then the feedback rows for a link.
async fn delete_feedback_for_link(state: &AppState, link_id: &str) {
    let feedback = state
        .rhype
        .find(format!("BetaFeedback.filter(.link_id == {})", quote(link_id)))
        .await
        .unwrap_or_default();
    for fb in &feedback {
        if let Some(fb_id) = fb.str("uuid") {
            let _ = state
                .rhype
                .exec(format!(
                    "BetaReply.filter(.feedback_id == {}).delete()",
                    quote(fb_id)
                ))
                .await;
        }
    }
    let _ = state
        .rhype
        .exec(format!(
            "BetaFeedback.filter(.link_id == {}).delete()",
            quote(link_id)
        ))
        .await;
}

/// Delete a beta link and everything under it (feedback + replies).
pub async fn delete_link_cascade(state: &AppState, link_id: &str) {
    delete_feedback_for_link(state, link_id).await;
    let _ = state
        .rhype
        .exec(format!(
            "BetaBookmark.filter(.link_id == {}).delete()",
            quote(link_id)
        ))
        .await;
    let _ = state
        .rhype
        .exec(format!("BetaLink.filter(.uuid == {}).delete()", quote(link_id)))
        .await;
}

/// Delete a book's beta metadata (links → feedback → replies). The book row and
/// its git repo are deleted by the caller.
pub async fn delete_book_beta_metadata(state: &AppState, book_id: &str) {
    let links = state
        .rhype
        .find(format!("BetaLink.filter(.book_id == {})", quote(book_id)))
        .await
        .unwrap_or_default();
    for link in &links {
        if let Some(link_id) = link.str("uuid") {
            delete_feedback_for_link(state, link_id).await;
            let _ = state
                .rhype
                .exec(format!(
                    "BetaBookmark.filter(.link_id == {}).delete()",
                    quote(link_id)
                ))
                .await;
        }
    }
    let _ = state
        .rhype
        .exec(format!(
            "BetaLink.filter(.book_id == {}).delete()",
            quote(book_id)
        ))
        .await;
}
