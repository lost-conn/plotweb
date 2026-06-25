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
