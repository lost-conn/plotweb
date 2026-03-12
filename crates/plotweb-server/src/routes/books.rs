use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use plotweb_common::*;
use serde_json::json;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::auth::AuthSession;

pub async fn list(
    State(pool): State<SqlitePool>,
    AuthSession(user_id): AuthSession,
) -> impl IntoResponse {
    let rows = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT b.id, b.title, b.description, b.created_at, b.updated_at \
         FROM books b WHERE b.user_id = ? ORDER BY b.updated_at DESC",
    )
    .bind(&user_id)
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    // Get chapter counts
    let mut books: Vec<Book> = Vec::new();
    for (id, title, description, created_at, updated_at) in rows {
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM chapters WHERE book_id = ?")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .unwrap_or((0,));

        books.push(Book {
            id,
            title,
            description,
            created_at,
            updated_at,
            chapter_count: Some(count.0),
        });
    }

    Json(serde_json::to_value(books).unwrap())
}

pub async fn create(
    State(pool): State<SqlitePool>,
    AuthSession(user_id): AuthSession,
    Json(req): Json<CreateBookRequest>,
) -> impl IntoResponse {
    if req.title.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "title is required" })),
        );
    }

    let id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO books (id, user_id, title, description) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(&user_id)
        .bind(&req.title)
        .bind(&req.description)
        .execute(&pool)
        .await
        .ok();

    let book = Book {
        id,
        title: req.title,
        description: req.description,
        created_at: String::new(),
        updated_at: String::new(),
        chapter_count: Some(0),
    };
    (StatusCode::CREATED, Json(serde_json::to_value(book).unwrap()))
}

pub async fn get(
    State(pool): State<SqlitePool>,
    AuthSession(user_id): AuthSession,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let row = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT id, title, description, created_at, updated_at FROM books WHERE id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&user_id)
    .fetch_optional(&pool)
    .await;

    match row {
        Ok(Some((id, title, description, created_at, updated_at))) => {
            let count: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM chapters WHERE book_id = ?")
                    .bind(&id)
                    .fetch_one(&pool)
                    .await
                    .unwrap_or((0,));

            let book = Book {
                id,
                title,
                description,
                created_at,
                updated_at,
                chapter_count: Some(count.0),
            };
            (StatusCode::OK, Json(serde_json::to_value(book).unwrap()))
        }
        _ => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        ),
    }
}

pub async fn update(
    State(pool): State<SqlitePool>,
    AuthSession(user_id): AuthSession,
    Path(id): Path<String>,
    Json(req): Json<UpdateBookRequest>,
) -> impl IntoResponse {
    // Verify ownership
    let exists = sqlx::query_as::<_, (i64,)>(
        "SELECT COUNT(*) FROM books WHERE id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&user_id)
    .fetch_one(&pool)
    .await
    .unwrap_or((0,));

    if exists.0 == 0 {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    if let Some(title) = &req.title {
        sqlx::query("UPDATE books SET title = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(title)
            .bind(&id)
            .execute(&pool)
            .await
            .ok();
    }
    if let Some(desc) = &req.description {
        sqlx::query("UPDATE books SET description = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(desc)
            .bind(&id)
            .execute(&pool)
            .await
            .ok();
    }

    (StatusCode::OK, Json(json!({ "ok": true })))
}

pub async fn delete(
    State(pool): State<SqlitePool>,
    AuthSession(user_id): AuthSession,
    Path(id): Path<String>,
) -> impl IntoResponse {
    sqlx::query("DELETE FROM books WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&user_id)
        .execute(&pool)
        .await
        .ok();

    (StatusCode::OK, Json(json!({ "ok": true })))
}
