use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use plotweb_common::*;
use serde_json::json;
use uuid::Uuid;

use tower_sessions::Session;

use crate::auth::AuthSession;
use crate::rhype::{quote, Fields};
use crate::routes::{delete_link_cascade, verify_book_ownership};
use crate::ws::WsMessage;
use crate::AppState;

/// Rewrite a stored cover URL (`/api/books/{id}/images/{file}`) to the
/// token-scoped path (`/api/beta/{token}/images/{file}`) so beta readers and
/// non-owner viewers can fetch it without the owner-only ACL rejecting them.
fn rewrite_cover_for_beta(cover: Option<String>, token: &str) -> Option<String> {
    let url = cover?;
    if let Some(rest) = url.strip_prefix("/api/books/") {
        if let Some(idx) = rest.find("/images/") {
            let filename = &rest[idx + "/images/".len()..];
            if !filename.is_empty() && !filename.contains('/') {
                return Some(format!("/api/beta/{}/images/{}", token, filename));
            }
        }
    }
    Some(url)
}

/// Resolve an optional username to its user UUID. Returns `Ok(None)` for an
/// absent/blank username, `Ok(Some(uuid))` when found, `Err(())` when a
/// non-blank username doesn't match a user.
async fn resolve_username(state: &AppState, username: Option<&str>) -> Result<Option<String>, ()> {
    let Some(name) = username else { return Ok(None) };
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match state
        .rhype
        .find_one(format!("User.filter(.username == {}).limit(1)", quote(trimmed)))
        .await
    {
        Ok(Some(u)) => Ok(u.string("uuid").map(Some).unwrap_or(None)),
        _ => Err(()),
    }
}

/// Look up the author's email (and book title) for a book, for notifications.
async fn author_email_and_title(state: &AppState, book_id: &str) -> Option<(String, String)> {
    let book = state
        .rhype
        .find_one(format!("Book.filter(.uuid == {}).limit(1)", quote(book_id)))
        .await
        .ok()
        .flatten()?;
    let title = book.string("title").unwrap_or_default();
    let owner_id = book.string("user_id")?;
    let user = state
        .rhype
        .find_one(format!("User.filter(.uuid == {}).limit(1)", quote(&owner_id)))
        .await
        .ok()
        .flatten()?;
    let email = user.string("email")?;
    Some((email, title))
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

    let mut rows = state
        .rhype
        .find(format!("BetaLink.filter(.book_id == {})", quote(&book_id)))
        .await
        .unwrap_or_default();
    rows.sort_by(|a, b| b.string("created_at").cmp(&a.string("created_at")));

    let mut links: Vec<BetaReaderLink> = Vec::new();
    for row in rows {
        // LEFT JOIN users → username (only if a user is attached).
        let link_user_id = row.string("user_id");
        let username = match &link_user_id {
            Some(uid) => state
                .rhype
                .find_one(format!("User.filter(.uuid == {}).limit(1)", quote(uid)))
                .await
                .ok()
                .flatten()
                .and_then(|u| u.string("username")),
            None => None,
        };
        links.push(BetaReaderLink {
            id: row.string("uuid").unwrap_or_default(),
            book_id: row.string("book_id").unwrap_or_default(),
            token: row.string("token").unwrap_or_default(),
            reader_name: row.string("reader_name").unwrap_or_default(),
            max_chapter_index: row.i64("max_chapter_index"),
            active: row.bool("active").unwrap_or(false),
            created_at: row.string("created_at").unwrap_or_default(),
            pinned_commit: row.string("pinned_commit"),
            user_id: link_user_id,
            username,
        });
    }

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
    let resolved_user_id = match resolve_username(&state, req.username.as_deref()).await {
        Ok(uid) => uid,
        Err(()) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "user not found" }))),
    };
    let resolved_username = if resolved_user_id.is_some() {
        req.username.as_ref().map(|u| u.trim().to_string())
    } else {
        None
    };

    let fields = Fields::new()
        .str("uuid", &id)
        .str("book_id", &book_id)
        .str("token", &token)
        .str("reader_name", req.reader_name.trim())
        .opt_int("max_chapter_index", req.max_chapter_index)
        .bool("active", true)
        .opt_str("pinned_commit", pinned_commit.as_deref())
        .opt_str("user_id", resolved_user_id.as_deref())
        .str("created_at", &now)
        .render();

    if let Err(e) = state.rhype.create(format!("BetaLink.create({fields})")).await {
        eprintln!("Failed to create beta link: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to create link" })),
        );
    }

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

    let mut fields = Fields::new();
    let mut any = false;

    if let Some(name) = &req.reader_name {
        fields = fields.str("reader_name", name.trim());
        any = true;
    }
    if let Some(max_ch) = &req.max_chapter_index {
        fields = match max_ch {
            Some(v) => fields.int("max_chapter_index", *v),
            None => fields.null("max_chapter_index"),
        };
        any = true;
    }
    if let Some(active) = req.active {
        fields = fields.bool("active", active);
        any = true;
    }
    if let Some(ref pinned) = req.pinned_commit {
        let resolved = match pinned {
            Some(pc) if pc.eq_ignore_ascii_case("HEAD") => state.books.get_head_oid(&book_id).await.ok(),
            Some(pc) => Some(pc.clone()),
            None => None,
        };
        fields = match resolved {
            Some(v) => fields.str("pinned_commit", &v),
            None => fields.null("pinned_commit"),
        };
        any = true;
    }
    if let Some(ref username_opt) = req.username {
        let resolved_user_id = match resolve_username(&state, username_opt.as_deref()).await {
            Ok(uid) => uid,
            Err(()) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "user not found" }))),
        };
        fields = match resolved_user_id {
            Some(uid) => fields.str("user_id", &uid),
            None => fields.null("user_id"),
        };
        any = true;
    }

    if any {
        let _ = state
            .rhype
            .exec(format!(
                "BetaLink.filter(.uuid == {} && .book_id == {}).update({})",
                quote(&link_id),
                quote(&book_id),
                fields.render()
            ))
            .await;
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

    // Only delete a link that actually belongs to this book (the old query
    // scoped the DELETE by id AND book_id).
    let belongs = state
        .rhype
        .exists(format!(
            "BetaLink.filter(.uuid == {} && .book_id == {}).limit(1)",
            quote(&link_id),
            quote(&book_id)
        ))
        .await
        .unwrap_or(false);
    if belongs {
        delete_link_cascade(&state, &link_id).await;
    }

    (StatusCode::OK, Json(json!({ "ok": true })))
}

// ── Public Reader Endpoints (token-based, no auth) ──────────────────────────

/// Fetch a beta link by token. Returns the row only if it exists.
async fn link_by_token(state: &AppState, token: &str) -> Option<crate::rhype::RhypeObject> {
    state
        .rhype
        .find_one(format!("BetaLink.filter(.token == {}).limit(1)", quote(token)))
        .await
        .ok()
        .flatten()
}

/// Get book info + chapter list for a beta reader.
pub async fn reader_view(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> impl IntoResponse {
    let link = match link_by_token(&state, &token).await {
        Some(l) => l,
        None => return (StatusCode::NOT_FOUND, Json(json!({ "error": "link not found" }))),
    };

    if !link.bool("active").unwrap_or(false) {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "this link has been deactivated" })));
    }

    let book_id = link.string("book_id").unwrap_or_default();
    let reader_name = link.string("reader_name").unwrap_or_default();
    let max_chapter_index = link.i64("max_chapter_index");
    let pinned_commit = link.string("pinned_commit");

    // Get book data from git (pinned or live)
    let (book_data, chapters) = if let Some(ref commit) = pinned_commit {
        let book_data = match state.books.get_book_at_commit(&book_id, commit).await {
            Ok(data) => data,
            Err(_) => return (StatusCode::NOT_FOUND, Json(json!({ "error": "book not found" }))),
        };
        let chapters = state
            .books
            .list_chapters_at_commit(&book_id, commit)
            .await
            .unwrap_or_default();
        (book_data, chapters)
    } else {
        let book_data = match state.books.get_book(&book_id).await {
            Ok(data) => data,
            Err(_) => return (StatusCode::NOT_FOUND, Json(json!({ "error": "book not found" }))),
        };
        let chapters = state.books.list_chapters(&book_id).await.unwrap_or_default();
        (book_data, chapters)
    };

    let mut summaries: Vec<BetaChapterSummary> = chapters
        .into_iter()
        .filter(|ch| max_chapter_index.map(|max| ch.sort_order <= max).unwrap_or(true))
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
        cover_image: rewrite_cover_for_beta(book_data.cover_image, &token),
    };

    (StatusCode::OK, Json(serde_json::to_value(view).unwrap()))
}

/// Get a specific chapter for a beta reader.
pub async fn reader_chapter(
    State(state): State<AppState>,
    Path((token, chapter_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let link = match link_by_token(&state, &token).await {
        Some(l) => l,
        None => return (StatusCode::NOT_FOUND, Json(json!({ "error": "link not found" }))),
    };

    if !link.bool("active").unwrap_or(false) {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "this link has been deactivated" })));
    }

    let book_id = link.string("book_id").unwrap_or_default();
    let max_chapter_index = link.i64("max_chapter_index");
    let pinned_commit = link.string("pinned_commit");

    let ch_result = if let Some(ref commit) = pinned_commit {
        state.books.get_chapter_at_commit(&book_id, &chapter_id, commit).await
    } else {
        state.books.get_chapter(&book_id, &chapter_id).await
    };

    match ch_result {
        Ok(ch) => {
            if let Some(max) = max_chapter_index {
                if ch.sort_order > max {
                    return (StatusCode::FORBIDDEN, Json(json!({ "error": "chapter not accessible" })));
                }
            }

            // Rewrite image URLs so beta readers can access them via their token
            let content = ch.content.replace(
                &format!("/api/books/{}/images/", book_id),
                &format!("/api/beta/{}/images/", token),
            );
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
        Err(_) => (StatusCode::NOT_FOUND, Json(json!({ "error": "chapter not found" }))),
    }
}

/// Submit feedback as a beta reader.
pub async fn reader_create_feedback(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Json(req): Json<CreateBetaFeedbackRequest>,
) -> impl IntoResponse {
    let link = match link_by_token(&state, &token).await {
        Some(l) => l,
        None => return (StatusCode::NOT_FOUND, Json(json!({ "error": "link not found" }))),
    };

    if !link.bool("active").unwrap_or(false) {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "this link has been deactivated" })));
    }

    let link_id = link.string("uuid").unwrap_or_default();
    let book_id = link.string("book_id").unwrap_or_default();
    let reader_name = link.string("reader_name").unwrap_or_default();

    if req.comment.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "comment is required" })));
    }

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let fields = Fields::new()
        .str("uuid", &id)
        .str("link_id", &link_id)
        .str("chapter_id", &req.chapter_id)
        .str("selected_text", &req.selected_text)
        .str("context_block", &req.context_block)
        .str("comment", req.comment.trim())
        .bool("resolved", false)
        .str("created_at", &now)
        .render();
    let _ = state.rhype.create(format!("BetaFeedback.create({fields})")).await;

    let comment = req.comment.trim().to_string();

    state.broadcaster.broadcast(&book_id, &WsMessage::NewFeedback(BetaFeedback {
        id: id.clone(),
        link_id: link_id.clone(),
        chapter_id: req.chapter_id.clone(),
        selected_text: req.selected_text.clone(),
        context_block: req.context_block.clone(),
        comment: comment.clone(),
        reader_name: reader_name.clone(),
        resolved: false,
        created_at: now,
        replies: Vec::new(),
    }));

    // Email notification to the book author
    if let Some(ref email_service) = state.email {
        let email_service = email_service.clone();
        let state2 = state.clone();
        let book_id = book_id.clone();
        let chapter_id = req.chapter_id.clone();
        tokio::spawn(async move {
            if let Some((email, book_title)) = author_email_and_title(&state2, &book_id).await {
                let chapter_title = state2
                    .books
                    .get_chapter(&book_id, &chapter_id)
                    .await
                    .map(|ch| ch.title)
                    .unwrap_or_default();
                email_service
                    .notify_new_feedback(&email, &book_title, &chapter_title, &reader_name, &comment, &book_id)
                    .await;
            }
        });
    }

    (StatusCode::CREATED, Json(json!({ "ok": true, "id": id })))
}

/// Get feedback for a beta reader's link.
pub async fn reader_list_feedback(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> impl IntoResponse {
    let link = match link_by_token(&state, &token).await {
        Some(l) => l,
        None => return (StatusCode::NOT_FOUND, Json(json!({ "error": "link not found" }))),
    };

    if !link.bool("active").unwrap_or(false) {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "this link has been deactivated" })));
    }

    let link_id = link.string("uuid").unwrap_or_default();
    let reader_name = link.string("reader_name").unwrap_or_default();

    let feedback = fetch_feedback_for_link(&state, &link_id, &reader_name).await;
    (StatusCode::OK, Json(serde_json::to_value(feedback).unwrap()))
}

/// Reply to feedback as a beta reader.
pub async fn reader_reply_to_feedback(
    State(state): State<AppState>,
    Path((token, feedback_id)): Path<(String, String)>,
    Json(req): Json<CreateBetaReplyRequest>,
) -> impl IntoResponse {
    let link = match link_by_token(&state, &token).await {
        Some(l) => l,
        None => return (StatusCode::NOT_FOUND, Json(json!({ "error": "link not found" }))),
    };

    if !link.bool("active").unwrap_or(false) {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "this link has been deactivated" })));
    }

    let link_id = link.string("uuid").unwrap_or_default();
    let book_id = link.string("book_id").unwrap_or_default();
    let reader_name = link.string("reader_name").unwrap_or_default();

    // Verify the feedback belongs to this link
    let owns = state
        .rhype
        .exists(format!(
            "BetaFeedback.filter(.uuid == {} && .link_id == {}).limit(1)",
            quote(&feedback_id),
            quote(&link_id)
        ))
        .await
        .unwrap_or(false);
    if !owns {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "feedback not found" })));
    }

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let fields = Fields::new()
        .str("uuid", &id)
        .str("feedback_id", &feedback_id)
        .str("author_type", "reader")
        .str("author_name", &reader_name)
        .str("content", req.content.trim())
        .str("created_at", &now)
        .render();
    let _ = state.rhype.create(format!("BetaReply.create({fields})")).await;

    let reply_content = req.content.trim().to_string();

    state.broadcaster.broadcast(&book_id, &WsMessage::NewReply {
        feedback_id: feedback_id.clone(),
        reply: BetaFeedbackReply {
            id: id.clone(),
            feedback_id,
            author_type: "reader".to_string(),
            author_name: reader_name.clone(),
            content: reply_content.clone(),
            created_at: now,
        },
    });

    // Email notification to the book author
    if let Some(ref email_service) = state.email {
        let email_service = email_service.clone();
        let state2 = state.clone();
        let book_id = book_id.clone();
        tokio::spawn(async move {
            if let Some((email, book_title)) = author_email_and_title(&state2, &book_id).await {
                email_service
                    .notify_reader_reply(&email, &book_title, &reader_name, &reply_content, &book_id)
                    .await;
            }
        });
    }

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

    // No JOIN: gather feedback per link of the book, then sort newest-first.
    let links = state
        .rhype
        .find(format!("BetaLink.filter(.book_id == {})", quote(&book_id)))
        .await
        .unwrap_or_default();

    let mut feedback: Vec<BetaFeedback> = Vec::new();
    for link in links {
        let link_id = link.string("uuid").unwrap_or_default();
        let reader_name = link.string("reader_name").unwrap_or_default();
        feedback.extend(fetch_feedback_for_link(&state, &link_id, &reader_name).await);
    }
    feedback.sort_by(|a, b| b.created_at.cmp(&a.created_at));

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

    // Load the feedback and confirm its link belongs to this book.
    let fb = state
        .rhype
        .find_one(format!("BetaFeedback.filter(.uuid == {}).limit(1)", quote(&feedback_id)))
        .await
        .ok()
        .flatten();
    let Some(fb) = fb else {
        return (StatusCode::OK, Json(json!({ "ok": true })));
    };
    let link_id = fb.string("link_id").unwrap_or_default();
    let in_book = state
        .rhype
        .exists(format!(
            "BetaLink.filter(.uuid == {} && .book_id == {}).limit(1)",
            quote(&link_id),
            quote(&book_id)
        ))
        .await
        .unwrap_or(false);
    if !in_book {
        return (StatusCode::OK, Json(json!({ "ok": true })));
    }

    let resolved = !fb.bool("resolved").unwrap_or(false);
    let _ = state
        .rhype
        .exec(format!(
            "BetaFeedback.filter(.uuid == {}).update({})",
            quote(&feedback_id),
            Fields::new().bool("resolved", resolved).render()
        ))
        .await;

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

    // Confirm the feedback's link belongs to this book, then delete it + replies.
    let fb = state
        .rhype
        .find_one(format!("BetaFeedback.filter(.uuid == {}).limit(1)", quote(&feedback_id)))
        .await
        .ok()
        .flatten();
    if let Some(fb) = fb {
        let link_id = fb.string("link_id").unwrap_or_default();
        let in_book = state
            .rhype
            .exists(format!(
                "BetaLink.filter(.uuid == {} && .book_id == {}).limit(1)",
                quote(&link_id),
                quote(&book_id)
            ))
            .await
            .unwrap_or(false);
        if in_book {
            let _ = state
                .rhype
                .exec(format!(
                    "BetaReply.filter(.feedback_id == {}).delete()",
                    quote(&feedback_id)
                ))
                .await;
            let _ = state
                .rhype
                .exec(format!("BetaFeedback.filter(.uuid == {}).delete()", quote(&feedback_id)))
                .await;
        }
    }

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
    let username = state
        .rhype
        .find_one(format!("User.filter(.uuid == {}).limit(1)", quote(&user_id)))
        .await
        .ok()
        .flatten()
        .and_then(|u| u.string("username"))
        .unwrap_or_else(|| "Author".to_string());

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let fields = Fields::new()
        .str("uuid", &id)
        .str("feedback_id", &feedback_id)
        .str("author_type", "owner")
        .str("author_name", &username)
        .str("content", req.content.trim())
        .str("created_at", &now)
        .render();
    let _ = state.rhype.create(format!("BetaReply.create({fields})")).await;

    let reply_content = req.content.trim().to_string();

    state.broadcaster.broadcast(&book_id, &WsMessage::NewReply {
        feedback_id: feedback_id.clone(),
        reply: BetaFeedbackReply {
            id: id.clone(),
            feedback_id: feedback_id.clone(),
            author_type: "owner".to_string(),
            author_name: username.clone(),
            content: reply_content.clone(),
            created_at: now,
        },
    });

    // Email notification to the beta reader (if they have an account)
    if let Some(ref email_service) = state.email {
        let email_service = email_service.clone();
        let state2 = state.clone();
        let book_id = book_id.clone();
        tokio::spawn(async move {
            // feedback → link_id
            let Some(fb) = state2
                .rhype
                .find_one(format!("BetaFeedback.filter(.uuid == {}).limit(1)", quote(&feedback_id)))
                .await
                .ok()
                .flatten()
            else {
                return;
            };
            let Some(link_id) = fb.string("link_id") else { return };

            // link → reader user_id + token
            let Some(link) = state2
                .rhype
                .find_one(format!("BetaLink.filter(.uuid == {}).limit(1)", quote(&link_id)))
                .await
                .ok()
                .flatten()
            else {
                return;
            };
            let (Some(reader_user_id), Some(token)) = (link.string("user_id"), link.string("token"))
            else {
                return;
            };

            // reader user → email
            let Some(reader) = state2
                .rhype
                .find_one(format!("User.filter(.uuid == {}).limit(1)", quote(&reader_user_id)))
                .await
                .ok()
                .flatten()
            else {
                return;
            };
            let Some(email) = reader.string("email") else { return };

            let book_title = state2
                .rhype
                .find_one(format!("Book.filter(.uuid == {}).limit(1)", quote(&book_id)))
                .await
                .ok()
                .flatten()
                .and_then(|b| b.string("title"))
                .unwrap_or_default();

            email_service
                .notify_author_reply(&email, &book_title, &username, &reply_content, &token)
                .await;
        });
    }

    (StatusCode::CREATED, Json(json!({ "ok": true, "id": id })))
}

// ── Shared Books & Claim ────────────────────────────────────────────────────

/// List books shared with the current user via beta reader links.
pub async fn list_shared_books(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
) -> impl IntoResponse {
    let mut rows = state
        .rhype
        .find(format!(
            "BetaLink.filter(.user_id == {} && .active == true)",
            quote(&user_id)
        ))
        .await
        .unwrap_or_default();
    rows.sort_by(|a, b| b.string("created_at").cmp(&a.string("created_at")));

    let mut shared: Vec<SharedBook> = Vec::new();
    for link in rows {
        let token = link.string("token").unwrap_or_default();
        let reader_name = link.string("reader_name").unwrap_or_default();
        let book_id = link.string("book_id").unwrap_or_default();

        // book → title + author user_id
        let Some(book) = state
            .rhype
            .find_one(format!("Book.filter(.uuid == {}).limit(1)", quote(&book_id)))
            .await
            .ok()
            .flatten()
        else {
            continue;
        };
        let book_title = book.string("title").unwrap_or_default();
        let author_username = match book.string("user_id") {
            Some(owner_id) => state
                .rhype
                .find_one(format!("User.filter(.uuid == {}).limit(1)", quote(&owner_id)))
                .await
                .ok()
                .flatten()
                .and_then(|u| u.string("username"))
                .unwrap_or_default(),
            None => String::new(),
        };

        let (description, cover_image) = state
            .books
            .get_book(&book_id)
            .await
            .map(|b| (b.description, b.cover_image))
            .unwrap_or_default();
        let cover_image = rewrite_cover_for_beta(cover_image, &token);

        shared.push(SharedBook {
            book_title,
            book_description: description,
            token,
            reader_name,
            author_username,
            cover_image,
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
        // Only claim if the link exists, is active, and has no user attached.
        if let Some(link) = link_by_token(&state, &token).await {
            let active = link.bool("active").unwrap_or(false);
            let unattached = link.string("user_id").is_none();
            if active && unattached {
                let _ = state
                    .rhype
                    .exec(format!(
                        "BetaLink.filter(.token == {}).update({})",
                        quote(&token),
                        Fields::new().str("user_id", &user_id).render()
                    ))
                    .await;
            }
        }
    }

    (StatusCode::OK, Json(json!({ "ok": true })))
}

// ── Helpers ─────────────────────────────────────────────────────────────────

async fn fetch_replies(state: &AppState, feedback_id: &str) -> Vec<BetaFeedbackReply> {
    let mut rows = state
        .rhype
        .find(format!("BetaReply.filter(.feedback_id == {})", quote(feedback_id)))
        .await
        .unwrap_or_default();
    rows.sort_by(|a, b| a.string("created_at").cmp(&b.string("created_at")));

    rows.into_iter()
        .map(|r| BetaFeedbackReply {
            id: r.string("uuid").unwrap_or_default(),
            feedback_id: r.string("feedback_id").unwrap_or_default(),
            author_type: r.string("author_type").unwrap_or_default(),
            author_name: r.string("author_name").unwrap_or_default(),
            content: r.string("content").unwrap_or_default(),
            created_at: r.string("created_at").unwrap_or_default(),
        })
        .collect()
}

async fn fetch_feedback_for_link(
    state: &AppState,
    link_id: &str,
    reader_name: &str,
) -> Vec<BetaFeedback> {
    let mut rows = state
        .rhype
        .find(format!("BetaFeedback.filter(.link_id == {})", quote(link_id)))
        .await
        .unwrap_or_default();
    rows.sort_by(|a, b| b.string("created_at").cmp(&a.string("created_at")));

    let mut feedback = Vec::new();
    for row in rows {
        let id = row.string("uuid").unwrap_or_default();
        let replies = fetch_replies(state, &id).await;
        feedback.push(BetaFeedback {
            id,
            link_id: row.string("link_id").unwrap_or_default(),
            chapter_id: row.string("chapter_id").unwrap_or_default(),
            selected_text: row.string("selected_text").unwrap_or_default(),
            context_block: row.string("context_block").unwrap_or_default(),
            comment: row.string("comment").unwrap_or_default(),
            reader_name: reader_name.to_string(),
            resolved: row.bool("resolved").unwrap_or(false),
            created_at: row.string("created_at").unwrap_or_default(),
            replies,
        });
    }
    feedback
}
