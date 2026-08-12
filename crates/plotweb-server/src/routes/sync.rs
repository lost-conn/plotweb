//! Automerge sync endpoints (Phase 2 · sync engine slice 1).
//!
//! Replaces the Phase-0 spike relay that used to live here — an unauthenticated,
//! process-global `HashMap` any caller could write any doc-id into. What it proved
//! (the C→S→C transport carrying opaque CRDT bytes) is recorded in
//! `docs/offline-first-rinch-plan.md` §"Spike ③ results"; the real endpoint below
//! keeps that transport shape and adds the two things the spike had no business
//! having: **authorization** and a **canonical, durable document**.
//!
//! One HTTP request carries one Automerge sync message each way, as raw bytes
//! (`application/octet-stream`). The protocol work is [`crate::sync`]; this module is
//! the authorization boundary and the per-doc lock.
//!
//! Routes are **book-scoped** (`/api/books/{book_id}/sync/{doc_id}`) so authorization
//! is the same ownership check every other book route already makes, rather than a new
//! global doc→owner index. The `user:` index doc, which has no book, is reached at
//! `/api/sync/user` and is *always* the session's own — the user id is never taken
//! from the request.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::auth::AuthSession;
use crate::sync::{self, SyncError};
use crate::AppState;

/// Body cap for one sync message. Generous: the first message for a fresh device is a
/// whole document, and a long chapter with history is comfortably under this.
pub const MAX_SYNC_BODY: usize = 32 * 1024 * 1024;

/// `POST /api/books/{book_id}/sync/{doc_id}` — one round of the sync protocol for a
/// document belonging to `book_id`.
///
/// Authorization is two checks, both required: the caller owns the book, **and**
/// `doc_id` is one of that book's documents. An id that isn't (a chapter of someone
/// else's book, a typo, a probe) is a 404 — never an implicit create.
pub async fn sync_book_doc(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, doc_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    if !super::verify_book_ownership(&state, &book_id, &user_id).await {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(doc_type) = doc_type_in_book(&state, &book_id, &doc_id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    run_round(&state, &doc_id, doc_type, body).await
}

/// `POST /api/books/{book_id}/sync/{doc_id}/adopt` — take ownership of a document
/// whose canonical copy is still the migration backfill's.
///
/// The body is a full Automerge document (`save()` bytes), not a sync message. See
/// [`crate::sync::adopt_doc`] for why bodies need this: the backfilled blob is frozen
/// at backfill time while git moved on, so it can be neither merged with (disjoint
/// histories concatenate) nor adopted from (stale text would overwrite current text).
///
/// Responds `{"adopted": bool}` — `false` means a client already owns the document and
/// the caller must use the sync protocol instead.
pub async fn adopt_book_doc(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, doc_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    if !super::verify_book_ownership(&state, &book_id, &user_id).await {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(doc_type) = doc_type_in_book(&state, &book_id, &doc_id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if body.len() > MAX_SYNC_BODY {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }

    let lock = state.doc_locks.for_doc(&doc_id);
    let _guard = lock.lock().await;

    let crdt_dir = state.crdt_dir.clone();
    let doc_id_owned = doc_id.clone();
    let doc_type = doc_type.to_string();
    let result = tokio::task::spawn_blocking(move || {
        sync::adopt_doc(&crdt_dir, &doc_id_owned, &doc_type, &body)
    })
    .await;

    match result {
        Ok(Ok(outcome)) => axum::Json(serde_json::json!({
            "adopted": outcome == sync::Adoption::Adopted,
        }))
        .into_response(),
        Ok(Err(SyncError::BadMessage(msg))) => (StatusCode::BAD_REQUEST, msg).into_response(),
        Ok(Err(e)) => {
            eprintln!("[sync] adopt {doc_id}: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            eprintln!("[sync] adopt worker panicked: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `GET /api/books/{book_id}/sync/{doc_id}` — the canonical document as a full
/// Automerge snapshot, or `204` when the server holds none.
///
/// The counterpart to adoption. A device whose local document was seeded
/// independently (from REST, pre-sync) shares no history with the canonical one, so
/// it must *replace* its copy rather than merge into it — merging disjoint histories
/// concatenates (`docs/sync-engine-design.md` §D8). Fetching the canonical bytes
/// outright is one request; reconstructing them through the sync protocol would take
/// several and buy nothing.
pub async fn get_canonical_doc(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, doc_id)): Path<(String, String)>,
) -> Response {
    if !super::verify_book_ownership(&state, &book_id, &user_id).await {
        return StatusCode::NOT_FOUND.into_response();
    }
    if doc_type_in_book(&state, &book_id, &doc_id).await.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }

    let crdt_dir = state.crdt_dir.clone();
    let doc_id_owned = doc_id.clone();
    let result =
        tokio::task::spawn_blocking(move || sync::canonical_snapshot(&crdt_dir, &doc_id_owned))
            .await;

    match result {
        Ok(Ok(Some(bytes))) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/octet-stream")],
            bytes,
        )
            .into_response(),
        Ok(Ok(None)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(e)) => {
            eprintln!("[sync] read {doc_id}: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            eprintln!("[sync] read worker panicked: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `GET /api/books/{book_id}/sync/heads` — the canonical heads of every document in
/// this book, as `{ "chapter:…": ["<hash>", …], … }`.
///
/// One request tells a client which documents have moved since it last looked, so a
/// background sweep can sync only those instead of polling each document in turn.
/// Documents the server has no canonical copy of are absent from the map.
///
/// Note this route is matched before `/sync/{doc_id}`: `heads` is a static segment,
/// and no document id can collide with it (ids always carry a `chapter:` / `note:` /
/// `book:` prefix).
pub async fn get_book_heads(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
) -> Response {
    if !super::verify_book_ownership(&state, &book_id, &user_id).await {
        return StatusCode::NOT_FOUND.into_response();
    }

    // Every document this book owns, from git — the same membership rule the sync
    // and adopt routes authorize against.
    let mut doc_ids = vec![format!("book:{book_id}")];
    if let Ok(chapters) = state.books.list_chapters(&book_id).await {
        doc_ids.extend(chapters.iter().map(|c| format!("chapter:{}", c.id)));
    }
    if let Ok((notes, _tree)) = state.books.list_notes(&book_id).await {
        doc_ids.extend(notes.iter().map(|n| format!("note:{}", n.id)));
    }

    let crdt_dir = state.crdt_dir.clone();
    let result =
        tokio::task::spawn_blocking(move || sync::canonical_heads(&crdt_dir, &doc_ids)).await;

    match result {
        Ok(Ok(heads)) => axum::Json(heads).into_response(),
        Ok(Err(e)) => {
            eprintln!("[sync] heads {book_id}: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            eprintln!("[sync] heads worker panicked: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /api/sync/user` — one round for the caller's own `user:` index doc.
pub async fn sync_user_doc(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    body: Bytes,
) -> Response {
    let doc_id = format!("user:{user_id}");
    run_round(&state, &doc_id, "user", body).await
}

/// Serialize on the doc, hand the protocol round to a blocking thread, and map the
/// result onto a binary response.
/// One yrs exchange for a body document: the client sends its state vector, and gets
/// back the update it lacks plus the server's own state vector, framed as
/// `[u32 LE length][diff][server state vector]`.
///
/// Two fixed steps replace Automerge's multi-round loop — the client applies the diff,
/// then posts what the server lacks to `.../update`. Framing beats a second request
/// for the state vector, and beats JSON+base64 for what is already binary.
async fn run_body_exchange(state: &AppState, doc_id: &str, body: Bytes) -> Response {
    let lock = state.doc_locks.for_doc(doc_id);
    let _guard = lock.lock().await;

    let crdt_dir = state.crdt_dir.clone();
    let doc_id = doc_id.to_string();
    let result =
        tokio::task::spawn_blocking(move || sync::body_exchange(&crdt_dir, &doc_id, &body)).await;

    match result {
        Ok(Ok(sync::BodyExchange::Diff { diff, state_vector })) => {
            let mut framed = Vec::with_capacity(4 + diff.len() + state_vector.len());
            framed.extend_from_slice(&(diff.len() as u32).to_le_bytes());
            framed.extend_from_slice(&diff);
            framed.extend_from_slice(&state_vector);
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/octet-stream")],
                framed,
            )
                .into_response()
        }
        // 409 rather than a flag in the frame: an older client reads it as an error and
        // backs off — which is the safe outcome — instead of misparsing a new field and
        // merging two unrelated documents. The client fetches the canonical copy and
        // replaces its own.
        Ok(Ok(sync::BodyExchange::Unrelated)) => (
            StatusCode::CONFLICT,
            "this document shares no history with the canonical one; fetch it and replace \
             your copy",
        )
            .into_response(),
        Ok(Err(SyncError::BadMessage(msg))) => (StatusCode::BAD_REQUEST, msg).into_response(),
        Ok(Err(e)) => {
            eprintln!("[sync] {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            eprintln!("[sync] worker panicked: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /api/books/{book_id}/sync/{doc_id}/update` — apply a client's yrs update to
/// a body document. The other half of [`run_body_exchange`].
pub async fn apply_body_update(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, doc_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    if !super::verify_book_ownership(&state, &book_id, &user_id).await {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(doc_type) = doc_type_in_book(&state, &book_id, &doc_id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !sync::is_body_doc(&doc_id) {
        return (StatusCode::BAD_REQUEST, "not a body document").into_response();
    }
    if body.len() > MAX_SYNC_BODY {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }

    let lock = state.doc_locks.for_doc(&doc_id);
    let _guard = lock.lock().await;

    let crdt_dir = state.crdt_dir.clone();
    let doc_id_owned = doc_id.clone();
    let doc_type = doc_type.to_string();
    let result = tokio::task::spawn_blocking(move || {
        sync::body_apply(&crdt_dir, &doc_id_owned, &doc_type, &body)
    })
    .await;

    match result {
        Ok(Ok(_changed)) => StatusCode::OK.into_response(),
        Ok(Err(SyncError::BadMessage(msg))) => (StatusCode::BAD_REQUEST, msg).into_response(),
        Ok(Err(e)) => {
            eprintln!("[sync] update {doc_id}: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            eprintln!("[sync] update worker panicked: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn run_round(state: &AppState, doc_id: &str, doc_type: &str, body: Bytes) -> Response {
    if body.len() > MAX_SYNC_BODY {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    // Bodies are the editor's CRDT, which is yrs since rinch #190; structure docs are
    // ours and stay Automerge.
    if sync::is_body_doc(doc_id) {
        return run_body_exchange(state, doc_id, body).await;
    }

    // Held across the whole read-modify-write: Automerge merges commute, but the
    // blob rewrite does not.
    let lock = state.doc_locks.for_doc(doc_id);
    let _guard = lock.lock().await;

    let crdt_dir = state.crdt_dir.clone();
    let doc_id = doc_id.to_string();
    let doc_type = doc_type.to_string();
    let result = tokio::task::spawn_blocking(move || {
        sync::sync_round(&crdt_dir, &doc_id, &doc_type, &body)
    })
    .await;

    match result {
        Ok(Ok(reply)) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/octet-stream")],
            reply,
        )
            .into_response(),
        // A message we can't decode is the client's fault and is not retryable.
        Ok(Err(SyncError::BadMessage(msg))) => {
            (StatusCode::BAD_REQUEST, msg).into_response()
        }
        Ok(Err(e)) => {
            eprintln!("[sync] {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            eprintln!("[sync] worker panicked: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// The doc type of `doc_id` **if** it is one of `book_id`'s documents, else `None`.
///
/// Membership comes from git, which is still authoritative pre-cutover: the book doc
/// itself, a chapter in its manuscript, or a note in its notes tree. After cutover
/// this check moves to the canonical `book:` document.
async fn doc_type_in_book(
    state: &AppState,
    book_id: &str,
    doc_id: &str,
) -> Option<&'static str> {
    if doc_id == format!("book:{book_id}") {
        return Some("book");
    }
    if let Some(chapter_id) = doc_id.strip_prefix("chapter:") {
        let chapters = state.books.list_chapters(book_id).await.ok()?;
        return chapters.iter().any(|c| c.id == chapter_id).then_some("chapter");
    }
    if let Some(note_id) = doc_id.strip_prefix("note:") {
        let (notes, _tree) = state.books.list_notes(book_id).await.ok()?;
        return notes.iter().any(|n| n.id == note_id).then_some("note");
    }
    None
}
