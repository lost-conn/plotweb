use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use plotweb_common::*;
use serde_json::json;
use uuid::Uuid;

use crate::auth::AuthSession;
use crate::rhype::{quote, Fields};
use crate::routes::{delete_book_beta_metadata, verify_book_ownership};
use crate::AppState;

pub async fn list(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
) -> impl IntoResponse {
    let mut rows = state
        .rhype
        .find(format!("Book.filter(.user_id == {})", quote(&user_id)))
        .await
        .unwrap_or_default();

    // The DSL has no ORDER BY; sort newest-first here. created_at is
    // "%Y-%m-%d %H:%M:%S", so lexical order is chronological.
    rows.sort_by(|a, b| b.string("created_at").cmp(&a.string("created_at")));

    let mut books: Vec<Book> = Vec::new();
    for row in rows {
        let id = row.string("uuid").unwrap_or_default();
        let created_at = row.string("created_at").unwrap_or_default();
        // Read extra data from git
        match state.books.get_book(&id).await {
            Ok(data) => {
                let chapter_count = data.chapter_order.len() as i64;
                let word_count = state.books.book_word_count(&id).await;
                books.push(Book {
                    id,
                    title: data.title,
                    description: data.description,
                    created_at: data.created_at,
                    updated_at: data.updated_at,
                    chapter_count: Some(chapter_count),
                    word_count: Some(word_count),
                    font_settings: data.font_settings,
                    cover_image: data.cover_image,
                });
            }
            Err(_) => {
                // Git repo missing — show basic info from the metadata row
                books.push(Book {
                    id,
                    title: row.string("title").unwrap_or_default(),
                    description: String::new(),
                    created_at: created_at.clone(),
                    updated_at: created_at,
                    chapter_count: Some(0),
                    word_count: Some(0),
                    font_settings: None,
                    cover_image: None,
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

    // Insert ownership row in rhypedb
    let fields = Fields::new()
        .str("uuid", &id)
        .str("user_id", &user_id)
        .str("title", &req.title)
        .str("created_at", &now)
        .render();
    if let Err(e) = state.rhype.create(format!("Book.create({fields})")).await {
        eprintln!("Failed to create book metadata: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to create book" })),
        );
    }

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
        word_count: Some(0),
        font_settings: None,
        cover_image: None,
    };
    (StatusCode::CREATED, Json(serde_json::to_value(book).unwrap()))
}

pub async fn get(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    match state.books.get_book(&id).await {
        Ok(data) => {
            let chapter_count = data.chapter_order.len() as i64;
            let word_count = state.books.book_word_count(&id).await;
            let book = Book {
                id,
                title: data.title,
                description: data.description,
                created_at: data.created_at,
                updated_at: data.updated_at,
                chapter_count: Some(chapter_count),
                word_count: Some(word_count),
                font_settings: data.font_settings,
                cover_image: data.cover_image,
            };
            (StatusCode::OK, Json(serde_json::to_value(book).unwrap()))
        }
        Err(_) => (
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
    if !verify_book_ownership(&state, &id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    // Update title in the metadata row if changed
    if let Some(title) = &req.title {
        let _ = state
            .rhype
            .exec(format!(
                "Book.filter(.uuid == {}).update({})",
                quote(&id),
                Fields::new().str("title", title).render()
            ))
            .await;
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
    if !verify_book_ownership(&state, &id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    // Cascade: beta links/feedback/replies, then the book row (was ON DELETE
    // CASCADE in SQLite).
    delete_book_beta_metadata(&state, &id).await;
    let _ = state
        .rhype
        .exec(format!("Book.filter(.uuid == {}).delete()", quote(&id)))
        .await;

    // Delete git repo
    if let Err(e) = state.books.delete_book(&id).await {
        eprintln!("Failed to delete book git repo: {}", e);
    }

    (StatusCode::OK, Json(json!({ "ok": true })))
}
