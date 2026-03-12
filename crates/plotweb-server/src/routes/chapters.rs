use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use plotweb_common::*;
use serde_json::json;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::auth::AuthSession;

/// Verify the book belongs to the user. Returns true if valid.
async fn verify_book_ownership(pool: &SqlitePool, book_id: &str, user_id: &str) -> bool {
    sqlx::query_as::<_, (i64,)>(
        "SELECT COUNT(*) FROM books WHERE id = ? AND user_id = ?",
    )
    .bind(book_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map(|r| r.0 > 0)
    .unwrap_or(false)
}

pub async fn list(
    State(pool): State<SqlitePool>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
) -> impl IntoResponse {
    if !verify_book_ownership(&pool, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    let rows = sqlx::query_as::<_, (String, String, String, String, i64, String, String)>(
        "SELECT id, book_id, title, content, sort_order, created_at, updated_at \
         FROM chapters WHERE book_id = ? ORDER BY sort_order ASC",
    )
    .bind(&book_id)
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    let chapters: Vec<Chapter> = rows
        .into_iter()
        .map(|(id, book_id, title, content, sort_order, created_at, updated_at)| Chapter {
            id,
            book_id,
            title,
            content,
            sort_order,
            created_at,
            updated_at,
        })
        .collect();

    (StatusCode::OK, Json(serde_json::to_value(chapters).unwrap()))
}

pub async fn get(
    State(pool): State<SqlitePool>,
    AuthSession(user_id): AuthSession,
    Path((book_id, chapter_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if !verify_book_ownership(&pool, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    let row = sqlx::query_as::<_, (String, String, String, String, i64, String, String)>(
        "SELECT id, book_id, title, content, sort_order, created_at, updated_at \
         FROM chapters WHERE id = ? AND book_id = ?",
    )
    .bind(&chapter_id)
    .bind(&book_id)
    .fetch_optional(&pool)
    .await;

    match row {
        Ok(Some((id, book_id, title, content, sort_order, created_at, updated_at))) => {
            let chapter = Chapter {
                id,
                book_id,
                title,
                content,
                sort_order,
                created_at,
                updated_at,
            };
            (StatusCode::OK, Json(serde_json::to_value(chapter).unwrap()))
        }
        _ => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "chapter not found" })),
        ),
    }
}

pub async fn create(
    State(pool): State<SqlitePool>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
    Json(req): Json<CreateChapterRequest>,
) -> impl IntoResponse {
    if !verify_book_ownership(&pool, &book_id, &user_id).await {
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

    // Get next sort_order
    let max_order: (i64,) = sqlx::query_as(
        "SELECT COALESCE(MAX(sort_order), -1) FROM chapters WHERE book_id = ?",
    )
    .bind(&book_id)
    .fetch_one(&pool)
    .await
    .unwrap_or((0,));

    let id = Uuid::new_v4().to_string();
    let sort_order = max_order.0 + 1;

    sqlx::query(
        "INSERT INTO chapters (id, book_id, title, sort_order) VALUES (?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&book_id)
    .bind(&req.title)
    .bind(sort_order)
    .execute(&pool)
    .await
    .ok();

    // Update book's updated_at
    sqlx::query("UPDATE books SET updated_at = datetime('now') WHERE id = ?")
        .bind(&book_id)
        .execute(&pool)
        .await
        .ok();

    let chapter = Chapter {
        id,
        book_id,
        title: req.title,
        content: String::new(),
        sort_order,
        created_at: String::new(),
        updated_at: String::new(),
    };
    (StatusCode::CREATED, Json(serde_json::to_value(chapter).unwrap()))
}

pub async fn update(
    State(pool): State<SqlitePool>,
    AuthSession(user_id): AuthSession,
    Path((book_id, chapter_id)): Path<(String, String)>,
    Json(req): Json<UpdateChapterRequest>,
) -> impl IntoResponse {
    if !verify_book_ownership(&pool, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    if let Some(title) = &req.title {
        sqlx::query(
            "UPDATE chapters SET title = ?, updated_at = datetime('now') WHERE id = ? AND book_id = ?",
        )
        .bind(title)
        .bind(&chapter_id)
        .bind(&book_id)
        .execute(&pool)
        .await
        .ok();
    }
    if let Some(content) = &req.content {
        sqlx::query(
            "UPDATE chapters SET content = ?, updated_at = datetime('now') WHERE id = ? AND book_id = ?",
        )
        .bind(content)
        .bind(&chapter_id)
        .bind(&book_id)
        .execute(&pool)
        .await
        .ok();
    }

    // Update book's updated_at
    sqlx::query("UPDATE books SET updated_at = datetime('now') WHERE id = ?")
        .bind(&book_id)
        .execute(&pool)
        .await
        .ok();

    (StatusCode::OK, Json(json!({ "ok": true })))
}

pub async fn delete(
    State(pool): State<SqlitePool>,
    AuthSession(user_id): AuthSession,
    Path((book_id, chapter_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if !verify_book_ownership(&pool, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    sqlx::query("DELETE FROM chapters WHERE id = ? AND book_id = ?")
        .bind(&chapter_id)
        .bind(&book_id)
        .execute(&pool)
        .await
        .ok();

    (StatusCode::OK, Json(json!({ "ok": true })))
}

pub async fn reorder(
    State(pool): State<SqlitePool>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
    Json(req): Json<ReorderChaptersRequest>,
) -> impl IntoResponse {
    if !verify_book_ownership(&pool, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    for (i, chapter_id) in req.chapter_ids.iter().enumerate() {
        sqlx::query(
            "UPDATE chapters SET sort_order = ?, updated_at = datetime('now') WHERE id = ? AND book_id = ?",
        )
        .bind(i as i64)
        .bind(chapter_id)
        .bind(&book_id)
        .execute(&pool)
        .await
        .ok();
    }

    (StatusCode::OK, Json(json!({ "ok": true })))
}
