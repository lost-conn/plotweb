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

/// The body a cut-over book should serve, or `None` to fall back to git.
///
/// Falling back matters: a canonical copy that is missing or unreadable means the
/// server serves slightly older content, which is recoverable. Serving an error, or an
/// empty body, is what would look like data loss to the author.
pub fn cutover_body(state: &AppState, book_id: &str, doc_id: &str) -> Option<String> {
    if !state.cutover.is_cut_over(book_id) {
        return None;
    }
    match crate::sync::canonical_snapshot(&state.crdt_dir, doc_id) {
        Ok(Some(bytes)) => match plotweb_crdt::materialize_body(&bytes) {
            Ok(content) => Some(content),
            Err(e) => {
                eprintln!("[cutover] {doc_id}: canonical unreadable, serving git: {e}");
                None
            }
        },
        Ok(None) => {
            eprintln!("[cutover] {doc_id}: no canonical copy, serving git");
            None
        }
        Err(e) => {
            eprintln!("[cutover] {doc_id}: store read failed, serving git: {e}");
            None
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
