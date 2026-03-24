use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use plotweb_common::*;
use serde::Deserialize;
use serde_json::json;

use crate::auth::AuthSession;
use crate::AppState;

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

#[derive(Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

fn default_limit() -> usize {
    50
}

pub async fn list(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    match state.books.list_commits(&book_id, query.limit, query.offset).await {
        Ok(commits) => (StatusCode::OK, Json(serde_json::to_value(commits).unwrap())),
        Err(e) => {
            eprintln!("Failed to list history: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "failed to list history" })),
            )
        }
    }
}

pub async fn list_chapters(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, commit)): Path<(String, String)>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    match state.books.list_chapters_at_commit(&book_id, &commit).await {
        Ok(chapters) => {
            let chapters: Vec<Chapter> = chapters
                .into_iter()
                .map(|ch| Chapter {
                    id: ch.id,
                    book_id: book_id.clone(),
                    title: ch.title,
                    content: ch.content,
                    sort_order: ch.sort_order,
                    created_at: ch.created_at,
                    updated_at: ch.updated_at,
                })
                .collect();
            (StatusCode::OK, Json(serde_json::to_value(chapters).unwrap()))
        }
        Err(e) => {
            eprintln!("Failed to list chapters at commit: {}", e);
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "commit not found" })),
            )
        }
    }
}

pub async fn get_chapter(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, commit, chapter_id)): Path<(String, String, String)>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    match state
        .books
        .get_chapter_at_commit(&book_id, &chapter_id, &commit)
        .await
    {
        Ok(ch) => {
            let chapter = Chapter {
                id: ch.id,
                book_id,
                title: ch.title,
                content: ch.content,
                sort_order: ch.sort_order,
                created_at: ch.created_at,
                updated_at: ch.updated_at,
            };
            (StatusCode::OK, Json(serde_json::to_value(chapter).unwrap()))
        }
        Err(e) => {
            eprintln!("Failed to get chapter at commit: {}", e);
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "chapter not found at commit" })),
            )
        }
    }
}

pub async fn restore(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, commit)): Path<(String, String)>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    match state.books.restore_to_commit(&book_id, &commit).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))),
        Err(e) => {
            eprintln!("Failed to restore book: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "failed to restore" })),
            )
        }
    }
}

pub async fn diff(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, commit)): Path<(String, String)>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    match state.books.diff_commit(&book_id, &commit).await {
        Ok(diff) => (StatusCode::OK, Json(serde_json::to_value(diff).unwrap())),
        Err(e) => {
            eprintln!("Failed to compute diff: {}", e);
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "commit not found" })),
            )
        }
    }
}
