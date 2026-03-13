use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use plotweb_common::*;
use serde_json::json;
use uuid::Uuid;

use crate::auth::AuthSession;
use crate::AppState;

/// Verify the book belongs to the user. Returns true if valid.
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
        Err(_) => (StatusCode::OK, Json(serde_json::to_value(Vec::<Chapter>::new()).unwrap())),
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
            let chapter = Chapter {
                id: ch.id,
                book_id,
                title: ch.title,
                content: ch.content,
                sort_order: ch.sort_order,
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

    if let Err(e) = state.books.update_chapter(&book_id, &chapter_id, &req).await {
        eprintln!("Failed to update chapter: {}", e);
    }

    (StatusCode::OK, Json(json!({ "ok": true })))
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
    }

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
    }

    (StatusCode::OK, Json(json!({ "ok": true })))
}
