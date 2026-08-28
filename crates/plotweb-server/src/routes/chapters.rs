use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use plotweb_common::*;
use serde_json::json;
use uuid::Uuid;

use crate::auth::AuthSession;
use crate::routes::verify_book_ownership;
use crate::AppState;

pub async fn list(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    match state.books.list_chapters(&book_id).await {
        Ok(chapters) => {
            // Cut over: order and titles are the canonical document's — they are what it
            // holds. Everything else (content, word counts, timestamps) is git's, kept
            // current by the mirror; the CRDT structure has no record of them.
            //
            // A chapter the canonical copy knows and git does not is one created on a
            // device whose mirror write has not landed yet. It is listed rather than
            // hidden: the alternative is a chapter that vanishes from the sidebar for
            // half a minute after someone adds it on their phone.
            let chapters: Vec<Chapter> = match super::cutover_structure(&state, &book_id).await {
                Some(structure) => structure
                    .chapters
                    .iter()
                    .enumerate()
                    .map(|(i, (id, title))| {
                        let git = chapters.iter().find(|c| &c.id == id);
                        Chapter {
                            id: id.clone(),
                            book_id: book_id.clone(),
                            title: title.clone(),
                            content: git.map(|c| c.content.clone()).unwrap_or_default(),
                            sort_order: i as i64,
                            word_count: git.map(|c| c.word_count).unwrap_or(0),
                            created_at: git.map(|c| c.created_at.clone()).unwrap_or_default(),
                            updated_at: git.map(|c| c.updated_at.clone()).unwrap_or_default(),
                        }
                    })
                    .collect(),
                None => chapters
                    .into_iter()
                    .map(|ch| Chapter {
                        id: ch.id,
                        book_id: book_id.clone(),
                        title: ch.title,
                        content: ch.content,
                        sort_order: ch.sort_order,
                        word_count: ch.word_count,
                        created_at: ch.created_at,
                        updated_at: ch.updated_at,
                    })
                    .collect(),
            };
            (StatusCode::OK, Json(serde_json::to_value(chapters).unwrap()))
        }
        // Not an empty list. A failed read and a book with no chapters are different
        // answers, and collapsing them lets the client seed an authoritative local
        // document from a failure — after which the book really is empty until
        // something happens to refill it.
        Err(e) => {
            eprintln!("Failed to list chapters for {book_id}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "failed to list chapters" })),
            )
        }
    }
}

pub async fn get(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, chapter_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    match state.books.get_chapter(&book_id, &chapter_id).await {
        Ok(ch) => {
            // Cut-over books read their body from the canonical document; git still
            // holds everything else about the chapter (title, order, timestamps) and
            // remains the mirror. A missing or unreadable canonical copy degrades to
            // git (see routes::cutover_body).
            let content = match super::cutover_body(
                &state,
                &book_id,
                &format!("chapter:{}", ch.id),
                &ch.content,
                plotweb_crdt::BodyKind::Chapter,
            ) {
                super::CutoverRead::Git => ch.content,
                super::CutoverRead::Canonical(content) => content,
            };
            let chapter = Chapter {
                id: ch.id,
                book_id,
                title: ch.title,
                content,
                sort_order: ch.sort_order,
                word_count: ch.word_count,
                created_at: ch.created_at,
                updated_at: ch.updated_at,
            };
            (StatusCode::OK, Json(serde_json::to_value(chapter).unwrap()))
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "chapter not found" })),
        ),
    }
}

pub async fn create(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
    Json(req): Json<CreateChapterRequest>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    if req.title.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "title is required" })),
        );
    }

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    match state.books.create_chapter(&book_id, &id, &req.title, &now).await {
        Ok(ch) => {
            super::apply_cutover_structure(&state, &book_id, &[]).await;
            let chapter = Chapter {
                id: ch.id,
                book_id,
                title: ch.title,
                content: ch.content,
                sort_order: ch.sort_order,
                word_count: ch.word_count,
                created_at: ch.created_at,
                updated_at: ch.updated_at,
            };
            (StatusCode::CREATED, Json(serde_json::to_value(chapter).unwrap()))
        }
        Err(e) => {
            eprintln!("Failed to create chapter: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "failed to create chapter" })),
            )
        }
    }
}

pub async fn update(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, chapter_id)): Path<(String, String)>,
    Json(req): Json<UpdateChapterRequest>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    let doc_id = format!("chapter:{chapter_id}");

    // `sync_owned` only means anything for a cut-over book. There, the canonical
    // document is the source of truth and sync is already carrying this body's edits,
    // so `content` is a stale duplicate: applying it re-inserts text the client has
    // since deleted (the reappearing-sentence bug), and writing it to git would make
    // the two copies disagree and lock the document on the next read.
    //
    // Anywhere else git *is* the truth and this write is the only one that reaches it.
    // Honouring the flag without the cutover check is how an edit lands in the
    // canonical store, is never written to git, and then vanishes on the next read —
    // which is exactly what happened when this was decided client-side alone.
    //
    // The third condition is the one this cost two days of silently dropped writes to
    // learn: a declaration that *sync owns this body* is only true if the canonical
    // document exists and this build can read it. Trusting the client's half alone
    // leaves both writers deferring to each other — git stands down because sync has
    // it, sync cannot deliver because the document will not load — and the edit
    // survives nowhere but the author's browser.
    let claimed_by_sync = req.sync_owned && state.cutover.is_cut_over(&book_id);
    let sync_owns_body = claimed_by_sync
        && super::canonical_is_authoritative(&state, &book_id, &doc_id);
    let overrode_claim = claimed_by_sync && !sync_owns_body;

    let carries_content = req.content.is_some();
    let req = UpdateChapterRequest {
        title: req.title,
        content: if sync_owns_body { None } else { req.content },
        sync_owned: req.sync_owned,
    };
    let wrote_to_git = req.content.is_some() || req.title.is_some();

    if let Err(e) = state.books.update_chapter(&book_id, &chapter_id, &req).await {
        eprintln!("Failed to update chapter: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to save chapter" })),
        );
    }

    // The claim was overridden, so stop the document advertising itself as a syncing
    // client's responsibility. Until it is repaired, git is where this body lives; a
    // stale claim would make the next write stand down all over again.
    if overrode_claim {
        let crdt_dir = state.crdt_dir.clone();
        let doc = doc_id.clone();
        if let Ok(Err(e)) =
            tokio::task::spawn_blocking(move || crate::sync::disown_canonical(&crdt_dir, &doc))
                .await
        {
            eprintln!("[cutover] {doc_id}: could not clear a stale sync claim: {e}");
        }
    }

    // Cut over: the write also lands *in* the canonical document, as an edit. Git has
    // just taken the same content, which is what keeps it a live mirror — and what
    // makes the flag reversible to current content rather than to cutover-day content.
    // `req.content` is already `None` when sync owns this body, so the canonical
    // document is left to sync — which is the whole point.
    let applied_to_canonical = match req.content.as_deref() {
        Some(content) => {
            super::apply_cutover_body(
                &state,
                &book_id,
                &doc_id,
                "chapter",
                content,
                plotweb_crdt::BodyKind::Chapter,
            )
            .await
        }
        None => false,
    };
    // Only when a title came with the request. An autosave carries content alone, and
    // re-reading the whole book on every keystroke's worth of save would be a real cost
    // for a structure that did not move.
    if req.title.is_some() {
        super::apply_cutover_structure(&state, &book_id, &[]).await;
    }

    let mut receipt = SaveReceipt {
        git: wrote_to_git,
        canonical: applied_to_canonical,
        deferred_to_sync: sync_owns_body,
        warning: None,
    };
    receipt.warning = super::save_warning(overrode_claim, carries_content, receipt.is_durable());
    (StatusCode::OK, Json(serde_json::to_value(receipt).unwrap()))
}

pub async fn delete(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, chapter_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    if let Err(e) = state.books.delete_chapter(&book_id, &chapter_id).await {
        eprintln!("Failed to delete chapter: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to delete chapter" })),
        );
    }
    // Removal from the parent index *is* the deletion (§D7); the orphaned body document
    // is left in the store, and phase F decides when unreferenced blobs are collected.
    // Named explicitly, because absence from git alone means nothing — see there.
    super::apply_cutover_structure(&state, &book_id, &[chapter_id.clone()]).await;

    (StatusCode::OK, Json(json!({ "ok": true })))
}

pub async fn reorder(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
    Json(req): Json<ReorderChaptersRequest>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    if let Err(e) = state.books.reorder_chapters(&book_id, &req.chapter_ids).await {
        eprintln!("Failed to reorder chapters: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to reorder chapters" })),
        );
    }
    super::apply_cutover_structure(&state, &book_id, &[]).await;

    (StatusCode::OK, Json(json!({ "ok": true })))
}
