use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use plotweb_common::*;
use serde_json::json;
use uuid::Uuid;

use crate::auth::AuthSession;
use crate::AppState;

pub async fn list(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
) -> impl IntoResponse {
    let rows = sqlx::query_as::<_, (String, String, String)>(
        "SELECT id, title, created_at FROM books WHERE user_id = ? ORDER BY created_at DESC",
    )
    .bind(&user_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let mut books: Vec<Book> = Vec::new();
    for (id, title, created_at) in rows {
        // Read extra data from git
        match state.books.get_book(&id).await {
            Ok(data) => {
                let chapter_count = data.chapter_order.len() as i64;
                books.push(Book {
                    id,
                    title: data.title,
                    description: data.description,
                    created_at: data.created_at,
                    updated_at: data.updated_at,
                    chapter_count: Some(chapter_count),
                    font_settings: data.font_settings,
                });
            }
            Err(_) => {
                // Git repo missing — show basic info from SQLite
                books.push(Book {
                    id,
                    title,
                    description: String::new(),
                    created_at: created_at.clone(),
                    updated_at: created_at,
                    chapter_count: Some(0),
                    font_settings: None,
                });
            }
        }
    }

    Json(serde_json::to_value(books).unwrap())
}

pub async fn create(
    State(state): State<AppState>,
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
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // Insert ownership row in SQLite
    sqlx::query("INSERT INTO books (id, user_id, title, created_at) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(&user_id)
        .bind(&req.title)
        .bind(&now)
        .execute(&state.db)
        .await
        .ok();

    // Create git repo
    if let Err(e) = state
        .books
        .create_book(&id, &req.title, &req.description, &now)
        .await
    {
        eprintln!("Failed to create book git repo: {}", e);
    }

    let book = Book {
        id,
        title: req.title,
        description: req.description,
        created_at: now.clone(),
        updated_at: now,
        chapter_count: Some(0),
        font_settings: None,
    };
    (StatusCode::CREATED, Json(serde_json::to_value(book).unwrap()))
}

pub async fn get(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Verify ownership
    let row = sqlx::query_as::<_, (String, String, String)>(
        "SELECT id, title, created_at FROM books WHERE id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&user_id)
    .fetch_optional(&state.db)
    .await;

    match row {
        Ok(Some((_id, _title, _created_at))) => {
            match state.books.get_book(&id).await {
                Ok(data) => {
                    let chapter_count = data.chapter_order.len() as i64;
                    let book = Book {
                        id,
                        title: data.title,
                        description: data.description,
                        created_at: data.created_at,
                        updated_at: data.updated_at,
                        chapter_count: Some(chapter_count),
                        font_settings: data.font_settings,
                    };
                    (StatusCode::OK, Json(serde_json::to_value(book).unwrap()))
                }
                Err(_) => (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": "book not found" })),
                ),
            }
        }
        _ => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        ),
    }
}

pub async fn update(
    State(state): State<AppState>,
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
    .fetch_one(&state.db)
    .await
    .unwrap_or((0,));

    if exists.0 == 0 {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    // Update title in SQLite if changed
    if let Some(title) = &req.title {
        sqlx::query("UPDATE books SET title = ? WHERE id = ?")
            .bind(title)
            .bind(&id)
            .execute(&state.db)
            .await
            .ok();
    }

    // Update git repo
    if let Err(e) = state.books.update_book(&id, &req).await {
        eprintln!("Failed to update book in git: {}", e);
    }

    (StatusCode::OK, Json(json!({ "ok": true })))
}

pub async fn delete(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Delete from SQLite
    sqlx::query("DELETE FROM books WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&user_id)
        .execute(&state.db)
        .await
        .ok();

    // Delete git repo
    if let Err(e) = state.books.delete_book(&id).await {
        eprintln!("Failed to delete book git repo: {}", e);
    }

    (StatusCode::OK, Json(json!({ "ok": true })))
}
