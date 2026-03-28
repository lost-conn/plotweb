use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use plotweb_common::*;
use serde_json::json;
use uuid::Uuid;

use tower_sessions::Session;

use crate::auth::AuthSession;
use crate::ws::WsMessage;
use crate::AppState;

/// Verify the book belongs to the user.
async fn verify_book_ownership(state: &AppState, book_id: &str, user_id: &str) -> bool {
    sqlx::query_as::<_, (i64,)>(
        "SELECT COUNT(*) FROM books WHERE id = ? AND user_id = ?",
    )
    .bind(book_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .map(|r| r.0 > 0)
    .unwrap_or(false)
}

// ── Beta Link CRUD (authenticated, book owner) ──────────────────────────────

pub async fn list_links(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "book not found" })));
    }

    let rows = sqlx::query_as::<_, (String, String, String, String, Option<i64>, i64, String, Option<String>, Option<String>, Option<String>)>(
        "SELECT l.id, l.book_id, l.token, l.reader_name, l.max_chapter_index, l.active, l.created_at, l.pinned_commit, l.user_id, u.username
         FROM beta_reader_links l
         LEFT JOIN users u ON l.user_id = u.id
         WHERE l.book_id = ? ORDER BY l.created_at DESC",
    )
    .bind(&book_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let links: Vec<BetaReaderLink> = rows
        .into_iter()
        .map(|(id, book_id, token, reader_name, max_chapter_index, active, created_at, pinned_commit, user_id, username)| {
            BetaReaderLink {
                id,
                book_id,
                token,
                reader_name,
                max_chapter_index,
                active: active != 0,
                created_at,
                pinned_commit,
                user_id,
                username,
            }
        })
        .collect();

    (StatusCode::OK, Json(serde_json::to_value(links).unwrap()))
}

pub async fn create_link(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
    Json(req): Json<CreateBetaLinkRequest>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "book not found" })));
    }

    if req.reader_name.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "reader_name is required" })));
    }

    let id = Uuid::new_v4().to_string();
    let token = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // Resolve pinned_commit: "HEAD" → actual commit hash
    let pinned_commit = if let Some(ref pc) = req.pinned_commit {
        if pc.eq_ignore_ascii_case("HEAD") {
            state.books.get_head_oid(&book_id).await.ok()
        } else {
            Some(pc.clone())
        }
    } else {
        None
    };

    // Resolve optional username to user_id
    let (resolved_user_id, resolved_username) = if let Some(ref username) = req.username {
        let trimmed = username.trim();
        if trimmed.is_empty() {
            (None, None)
        } else {
            match sqlx::query_as::<_, (String,)>("SELECT id FROM users WHERE username = ?")
                .bind(trimmed)
                .fetch_optional(&state.db)
                .await
            {
                Ok(Some((uid,))) => (Some(uid), Some(trimmed.to_string())),
                _ => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "user not found" }))),
            }
        }
    } else {
        (None, None)
    };

    sqlx::query(
        "INSERT INTO beta_reader_links (id, book_id, token, reader_name, max_chapter_index, pinned_commit, user_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&book_id)
    .bind(&token)
    .bind(req.reader_name.trim())
    .bind(req.max_chapter_index)
    .bind(&pinned_commit)
    .bind(&resolved_user_id)
    .bind(&now)
    .execute(&state.db)
    .await
    .ok();

    let link = BetaReaderLink {
        id,
        book_id,
        token,
        reader_name: req.reader_name.trim().to_string(),
        max_chapter_index: req.max_chapter_index,
        active: true,
        created_at: now,
        pinned_commit,
        user_id: resolved_user_id,
        username: resolved_username,
    };

    (StatusCode::CREATED, Json(serde_json::to_value(link).unwrap()))
}

pub async fn update_link(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, link_id)): Path<(String, String)>,
    Json(req): Json<UpdateBetaLinkRequest>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "book not found" })));
    }

    if let Some(name) = &req.reader_name {
        sqlx::query("UPDATE beta_reader_links SET reader_name = ? WHERE id = ? AND book_id = ?")
            .bind(name.trim())
            .bind(&link_id)
            .bind(&book_id)
            .execute(&state.db)
            .await
            .ok();
    }
    if let Some(max_ch) = &req.max_chapter_index {
        sqlx::query("UPDATE beta_reader_links SET max_chapter_index = ? WHERE id = ? AND book_id = ?")
            .bind(max_ch)
            .bind(&link_id)
            .bind(&book_id)
            .execute(&state.db)
            .await
            .ok();
    }
    if let Some(active) = req.active {
        sqlx::query("UPDATE beta_reader_links SET active = ? WHERE id = ? AND book_id = ?")
            .bind(active as i64)
            .bind(&link_id)
            .bind(&book_id)
            .execute(&state.db)
            .await
            .ok();
    }
    if let Some(ref pinned) = req.pinned_commit {
        let resolved = if let Some(pc) = pinned {
            if pc.eq_ignore_ascii_case("HEAD") {
                state.books.get_head_oid(&book_id).await.ok()
            } else {
                Some(pc.clone())
            }
        } else {
            None
        };
        sqlx::query("UPDATE beta_reader_links SET pinned_commit = ? WHERE id = ? AND book_id = ?")
            .bind(&resolved)
            .bind(&link_id)
            .bind(&book_id)
            .execute(&state.db)
            .await
            .ok();
    }
    if let Some(ref username_opt) = req.username {
        let resolved_user_id = if let Some(username) = username_opt {
            let trimmed = username.trim();
            if trimmed.is_empty() {
                None
            } else {
                match sqlx::query_as::<_, (String,)>("SELECT id FROM users WHERE username = ?")
                    .bind(trimmed)
                    .fetch_optional(&state.db)
                    .await
                {
                    Ok(Some((uid,))) => Some(uid),
                    _ => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "user not found" }))),
                }
            }
        } else {
            None // Detach user
        };
        sqlx::query("UPDATE beta_reader_links SET user_id = ? WHERE id = ? AND book_id = ?")
            .bind(&resolved_user_id)
            .bind(&link_id)
            .bind(&book_id)
            .execute(&state.db)
            .await
            .ok();
    }

    (StatusCode::OK, Json(json!({ "ok": true })))
}

pub async fn delete_link(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, link_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "book not found" })));
    }

    sqlx::query("DELETE FROM beta_reader_links WHERE id = ? AND book_id = ?")
        .bind(&link_id)
        .bind(&book_id)
        .execute(&state.db)
        .await
        .ok();

    (StatusCode::OK, Json(json!({ "ok": true })))
}

// ── Public Reader Endpoints (token-based, no auth) ──────────────────────────

/// Get book info + chapter list for a beta reader.
pub async fn reader_view(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> impl IntoResponse {
    // Look up the link
    let link = sqlx::query_as::<_, (String, String, String, Option<i64>, i64, Option<String>)>(
        "SELECT id, book_id, reader_name, max_chapter_index, active, pinned_commit
         FROM beta_reader_links WHERE token = ?",
    )
    .bind(&token)
    .fetch_optional(&state.db)
    .await;

    let (_link_id, book_id, reader_name, max_chapter_index, active, pinned_commit) = match link {
        Ok(Some(row)) => row,
        _ => return (StatusCode::NOT_FOUND, Json(json!({ "error": "link not found" }))),
    };

    if active == 0 {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "this link has been deactivated" })));
    }

    // Get book data from git (pinned or live)
    let (book_data, chapters) = if let Some(ref commit) = pinned_commit {
        let book_data = match state.books.get_book_at_commit(&book_id, commit).await {
            Ok(data) => data,
            Err(_) => return (StatusCode::NOT_FOUND, Json(json!({ "error": "book not found" }))),
        };
        let chapters = match state.books.list_chapters_at_commit(&book_id, commit).await {
            Ok(chs) => chs,
            Err(_) => Vec::new(),
        };
        (book_data, chapters)
    } else {
        let book_data = match state.books.get_book(&book_id).await {
            Ok(data) => data,
            Err(_) => return (StatusCode::NOT_FOUND, Json(json!({ "error": "book not found" }))),
        };
        let chapters = match state.books.list_chapters(&book_id).await {
            Ok(chs) => chs,
            Err(_) => Vec::new(),
        };
        (book_data, chapters)
    };

    // Filter by max_chapter_index if set
    let mut summaries: Vec<BetaChapterSummary> = chapters
        .into_iter()
        .filter(|ch| {
            if let Some(max) = max_chapter_index {
                ch.sort_order <= max
            } else {
                true
            }
        })
        .map(|ch| BetaChapterSummary {
            id: ch.id,
            title: ch.title,
            sort_order: ch.sort_order,
        })
        .collect();
    summaries.sort_by_key(|s| s.sort_order);

    let view = BetaReaderView {
        book_title: book_data.title,
        book_description: book_data.description,
        reader_name,
        chapters: summaries,
        font_settings: book_data.font_settings,
    };

    (StatusCode::OK, Json(serde_json::to_value(view).unwrap()))
}

/// Get a specific chapter for a beta reader.
pub async fn reader_chapter(
    State(state): State<AppState>,
    Path((token, chapter_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let link = sqlx::query_as::<_, (String, String, Option<i64>, i64, Option<String>)>(
        "SELECT id, book_id, max_chapter_index, active, pinned_commit
         FROM beta_reader_links WHERE token = ?",
    )
    .bind(&token)
    .fetch_optional(&state.db)
    .await;

    let (_link_id, book_id, max_chapter_index, active, pinned_commit) = match link {
        Ok(Some(row)) => row,
        _ => return (StatusCode::NOT_FOUND, Json(json!({ "error": "link not found" }))),
    };

    if active == 0 {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "this link has been deactivated" })));
    }

    let ch_result = if let Some(ref commit) = pinned_commit {
        state.books.get_chapter_at_commit(&book_id, &chapter_id, commit).await
    } else {
        state.books.get_chapter(&book_id, &chapter_id).await
    };

    match ch_result {
        Ok(ch) => {
            // Check chapter is within allowed range
            if let Some(max) = max_chapter_index {
                if ch.sort_order > max {
                    return (StatusCode::FORBIDDEN, Json(json!({ "error": "chapter not accessible" })));
                }
            }

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
            (StatusCode::OK, Json(serde_json::to_value(chapter).unwrap()))
        }
        Err(_) => (StatusCode::NOT_FOUND, Json(json!({ "error": "chapter not found" }))),
    }
}

/// Submit feedback as a beta reader.
pub async fn reader_create_feedback(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Json(req): Json<CreateBetaFeedbackRequest>,
) -> impl IntoResponse {
    let link = sqlx::query_as::<_, (String, String, String, i64)>(
        "SELECT id, book_id, reader_name, active
         FROM beta_reader_links WHERE token = ?",
    )
    .bind(&token)
    .fetch_optional(&state.db)
    .await;

    let (link_id, book_id, reader_name, active) = match link {
        Ok(Some(row)) => row,
        _ => return (StatusCode::NOT_FOUND, Json(json!({ "error": "link not found" }))),
    };

    if active == 0 {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "this link has been deactivated" })));
    }

    if req.comment.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "comment is required" })));
    }

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    sqlx::query(
        "INSERT INTO beta_reader_feedback (id, link_id, chapter_id, selected_text, context_block, comment, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&link_id)
    .bind(&req.chapter_id)
    .bind(&req.selected_text)
    .bind(&req.context_block)
    .bind(req.comment.trim())
    .bind(&now)
    .execute(&state.db)
    .await
    .ok();

    // Broadcast new feedback
    state.broadcaster.broadcast(&book_id, &WsMessage::NewFeedback(BetaFeedback {
        id: id.clone(),
        link_id: link_id.clone(),
        chapter_id: req.chapter_id.clone(),
        selected_text: req.selected_text.clone(),
        context_block: req.context_block.clone(),
        comment: req.comment.trim().to_string(),
        reader_name,
        resolved: false,
        created_at: now,
        replies: Vec::new(),
    }));

    (StatusCode::CREATED, Json(json!({ "ok": true, "id": id })))
}

/// Get feedback for a beta reader's link.
pub async fn reader_list_feedback(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> impl IntoResponse {
    let link = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT id, reader_name, active FROM beta_reader_links WHERE token = ?",
    )
    .bind(&token)
    .fetch_optional(&state.db)
    .await;

    let (link_id, reader_name, active) = match link {
        Ok(Some(row)) => row,
        _ => return (StatusCode::NOT_FOUND, Json(json!({ "error": "link not found" }))),
    };

    if active == 0 {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "this link has been deactivated" })));
    }

    let feedback = fetch_feedback_for_link(&state, &link_id, &reader_name).await;
    (StatusCode::OK, Json(serde_json::to_value(feedback).unwrap()))
}

/// Reply to feedback as a beta reader.
pub async fn reader_reply_to_feedback(
    State(state): State<AppState>,
    Path((token, feedback_id)): Path<(String, String)>,
    Json(req): Json<CreateBetaReplyRequest>,
) -> impl IntoResponse {
    let link = sqlx::query_as::<_, (String, String, String, i64)>(
        "SELECT id, book_id, reader_name, active FROM beta_reader_links WHERE token = ?",
    )
    .bind(&token)
    .fetch_optional(&state.db)
    .await;

    let (link_id, book_id, reader_name, active) = match link {
        Ok(Some(row)) => row,
        _ => return (StatusCode::NOT_FOUND, Json(json!({ "error": "link not found" }))),
    };

    if active == 0 {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "this link has been deactivated" })));
    }

    // Verify the feedback belongs to this link
    let owns = sqlx::query_as::<_, (i64,)>(
        "SELECT COUNT(*) FROM beta_reader_feedback WHERE id = ? AND link_id = ?",
    )
    .bind(&feedback_id)
    .bind(&link_id)
    .fetch_one(&state.db)
    .await
    .map(|r| r.0 > 0)
    .unwrap_or(false);

    if !owns {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "feedback not found" })));
    }

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    sqlx::query(
        "INSERT INTO beta_reader_replies (id, feedback_id, author_type, author_name, content, created_at)
         VALUES (?, ?, 'reader', ?, ?, ?)",
    )
    .bind(&id)
    .bind(&feedback_id)
    .bind(&reader_name)
    .bind(req.content.trim())
    .bind(&now)
    .execute(&state.db)
    .await
    .ok();

    state.broadcaster.broadcast(&book_id, &WsMessage::NewReply {
        feedback_id: feedback_id.clone(),
        reply: BetaFeedbackReply {
            id: id.clone(),
            feedback_id,
            author_type: "reader".to_string(),
            author_name: reader_name,
            content: req.content.trim().to_string(),
            created_at: now,
        },
    });

    (StatusCode::CREATED, Json(json!({ "ok": true, "id": id })))
}

// ── Author Feedback Management (authenticated) ─────────────────────────────

/// Get all feedback for a book (across all beta readers).
pub async fn list_book_feedback(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "book not found" })));
    }

    let rows = sqlx::query_as::<_, (String, String, String, String, String, String, i64, String, String)>(
        "SELECT f.id, f.link_id, f.chapter_id, f.selected_text, f.context_block, f.comment, f.resolved, f.created_at, l.reader_name
         FROM beta_reader_feedback f
         JOIN beta_reader_links l ON f.link_id = l.id
         WHERE l.book_id = ?
         ORDER BY f.created_at DESC",
    )
    .bind(&book_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let mut feedback: Vec<BetaFeedback> = Vec::new();
    for (id, link_id, chapter_id, selected_text, context_block, comment, resolved, created_at, reader_name) in rows {
        let replies = fetch_replies(&state, &id).await;
        feedback.push(BetaFeedback {
            id,
            link_id,
            chapter_id,
            selected_text,
            context_block,
            comment,
            reader_name,
            resolved: resolved != 0,
            created_at,
            replies,
        });
    }

    (StatusCode::OK, Json(serde_json::to_value(feedback).unwrap()))
}

/// Resolve/unresolve feedback.
pub async fn resolve_feedback(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, feedback_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "book not found" })));
    }

    // Toggle resolved status
    sqlx::query(
        "UPDATE beta_reader_feedback SET resolved = 1 - resolved
         WHERE id = ? AND link_id IN (SELECT id FROM beta_reader_links WHERE book_id = ?)",
    )
    .bind(&feedback_id)
    .bind(&book_id)
    .execute(&state.db)
    .await
    .ok();

    // Get the new resolved state
    let resolved = sqlx::query_as::<_, (i64,)>(
        "SELECT resolved FROM beta_reader_feedback WHERE id = ?",
    )
    .bind(&feedback_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .map(|r| r.0 != 0)
    .unwrap_or(false);

    state.broadcaster.broadcast(&book_id, &WsMessage::FeedbackResolved {
        feedback_id: feedback_id.clone(),
        resolved,
    });

    (StatusCode::OK, Json(json!({ "ok": true })))
}

/// Delete feedback.
pub async fn delete_feedback(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, feedback_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "book not found" })));
    }

    sqlx::query(
        "DELETE FROM beta_reader_feedback
         WHERE id = ? AND link_id IN (SELECT id FROM beta_reader_links WHERE book_id = ?)",
    )
    .bind(&feedback_id)
    .bind(&book_id)
    .execute(&state.db)
    .await
    .ok();

    state.broadcaster.broadcast(&book_id, &WsMessage::FeedbackDeleted {
        feedback_id: feedback_id.clone(),
    });

    (StatusCode::OK, Json(json!({ "ok": true })))
}

/// Reply to feedback as the book author.
pub async fn author_reply_to_feedback(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, feedback_id)): Path<(String, String)>,
    Json(req): Json<CreateBetaReplyRequest>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "book not found" })));
    }

    // Get author username
    let username = sqlx::query_as::<_, (String,)>("SELECT username FROM users WHERE id = ?")
        .bind(&user_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .map(|r| r.0)
        .unwrap_or_else(|| "Author".to_string());

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    sqlx::query(
        "INSERT INTO beta_reader_replies (id, feedback_id, author_type, author_name, content, created_at)
         VALUES (?, ?, 'owner', ?, ?, ?)",
    )
    .bind(&id)
    .bind(&feedback_id)
    .bind(&username)
    .bind(req.content.trim())
    .bind(&now)
    .execute(&state.db)
    .await
    .ok();

    state.broadcaster.broadcast(&book_id, &WsMessage::NewReply {
        feedback_id: feedback_id.clone(),
        reply: BetaFeedbackReply {
            id: id.clone(),
            feedback_id,
            author_type: "owner".to_string(),
            author_name: username,
            content: req.content.trim().to_string(),
            created_at: now,
        },
    });

    (StatusCode::CREATED, Json(json!({ "ok": true, "id": id })))
}

// ── Shared Books & Claim ────────────────────────────────────────────────────

/// List books shared with the current user via beta reader links.
pub async fn list_shared_books(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
) -> impl IntoResponse {
    let rows = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT l.token, l.reader_name, b.id, b.title, u.username
         FROM beta_reader_links l
         JOIN books b ON l.book_id = b.id
         JOIN users u ON b.user_id = u.id
         WHERE l.user_id = ? AND l.active = 1
         ORDER BY l.created_at DESC",
    )
    .bind(&user_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let mut shared: Vec<SharedBook> = Vec::new();
    for (token, reader_name, book_id, book_title, author_username) in rows {
        let description = state
            .books
            .get_book(&book_id)
            .await
            .map(|b| b.description)
            .unwrap_or_default();
        shared.push(SharedBook {
            book_title,
            book_description: description,
            token,
            reader_name,
            author_username,
        });
    }

    (StatusCode::OK, Json(serde_json::to_value(shared).unwrap()))
}

/// Claim a beta reader link for the current user (auto-attach).
/// Uses Session directly instead of AuthSession so it's a no-op (not 401)
/// when the user isn't logged in.
pub async fn claim_link(
    State(state): State<AppState>,
    session: Session,
    Path(token): Path<String>,
) -> impl IntoResponse {
    let user_id: Option<String> = session.get("user_id").await.ok().flatten();

    if let Some(user_id) = user_id {
        // Only claim if the link exists, is active, and has no user attached
        sqlx::query(
            "UPDATE beta_reader_links SET user_id = ?
             WHERE token = ? AND active = 1 AND user_id IS NULL",
        )
        .bind(&user_id)
        .bind(&token)
        .execute(&state.db)
        .await
        .ok();
    }

    (StatusCode::OK, Json(json!({ "ok": true })))
}

// ── Helpers ─────────────────────────────────────────────────────────────────

async fn fetch_replies(state: &AppState, feedback_id: &str) -> Vec<BetaFeedbackReply> {
    let rows = sqlx::query_as::<_, (String, String, String, String, String, String)>(
        "SELECT id, feedback_id, author_type, author_name, content, created_at
         FROM beta_reader_replies WHERE feedback_id = ? ORDER BY created_at ASC",
    )
    .bind(feedback_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    rows.into_iter()
        .map(|(id, feedback_id, author_type, author_name, content, created_at)| {
            BetaFeedbackReply {
                id,
                feedback_id,
                author_type,
                author_name,
                content,
                created_at,
            }
        })
        .collect()
}

async fn fetch_feedback_for_link(
    state: &AppState,
    link_id: &str,
    reader_name: &str,
) -> Vec<BetaFeedback> {
    let rows = sqlx::query_as::<_, (String, String, String, String, String, String, i64, String)>(
        "SELECT id, link_id, chapter_id, selected_text, context_block, comment, resolved, created_at
         FROM beta_reader_feedback WHERE link_id = ? ORDER BY created_at DESC",
    )
    .bind(link_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let mut feedback = Vec::new();
    for (id, link_id, chapter_id, selected_text, context_block, comment, resolved, created_at) in rows {
        let replies = fetch_replies(state, &id).await;
        feedback.push(BetaFeedback {
            id,
            link_id,
            chapter_id,
            selected_text,
            context_block,
            comment,
            reader_name: reader_name.to_string(),
            resolved: resolved != 0,
            created_at,
            replies,
        });
    }
    feedback
}
