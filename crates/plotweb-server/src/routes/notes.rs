use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use plotweb_common::*;
use serde_json::json;
use uuid::Uuid;

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

    match state.books.list_notes(&book_id).await {
        Ok((notes, tree)) => {
            let notes: Vec<Note> = notes
                .into_iter()
                .map(|n| Note {
                    id: n.id,
                    book_id: book_id.clone(),
                    title: n.title,
                    content: n.content,
                    color: n.color,
                    created_at: n.created_at,
                    updated_at: n.updated_at,
                })
                .collect();
            let resp = NotesResponse {
                notes,
                tree: NoteTree {
                    root_order: tree.root_order,
                    children: tree.children,
                    collapsed: tree.collapsed,
                },
            };
            (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()))
        }
        Err(_) => {
            let resp = NotesResponse {
                notes: Vec::new(),
                tree: NoteTree {
                    root_order: Vec::new(),
                    children: std::collections::HashMap::new(),
                    collapsed: Vec::new(),
                },
            };
            (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()))
        }
    }
}

pub async fn get(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, note_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    match state.books.get_note(&book_id, &note_id).await {
        Ok(n) => {
            let note = Note {
                id: n.id,
                book_id,
                title: n.title,
                content: n.content,
                color: n.color,
                created_at: n.created_at,
                updated_at: n.updated_at,
            };
            (StatusCode::OK, Json(serde_json::to_value(note).unwrap()))
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "note not found" })),
        ),
    }
}

pub async fn create(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
    Json(req): Json<CreateNoteRequest>,
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
    let now = chrono::Utc::now()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    match state
        .books
        .create_note(
            &book_id,
            &id,
            &req.title,
            req.parent_id.as_deref(),
            req.color.as_deref(),
            &now,
        )
        .await
    {
        Ok(n) => {
            let note = Note {
                id: n.id,
                book_id,
                title: n.title,
                content: n.content,
                color: n.color,
                created_at: n.created_at,
                updated_at: n.updated_at,
            };
            (
                StatusCode::CREATED,
                Json(serde_json::to_value(note).unwrap()),
            )
        }
        Err(e) => {
            eprintln!("Failed to create note: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "failed to create note" })),
            )
        }
    }
}

pub async fn update(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, note_id)): Path<(String, String)>,
    Json(req): Json<UpdateNoteRequest>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    // For color, if it's present in the request we pass Some(value), otherwise None (don't update)
    let color = req.color.as_ref().map(|c| Some(c.as_str()));

    if let Err(e) = state
        .books
        .update_note(
            &book_id,
            &note_id,
            req.title.as_deref(),
            req.content.as_deref(),
            color,
        )
        .await
    {
        eprintln!("Failed to update note: {}", e);
    }

    (StatusCode::OK, Json(json!({ "ok": true })))
}

pub async fn delete(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, note_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    if let Err(e) = state.books.delete_note(&book_id, &note_id).await {
        eprintln!("Failed to delete note: {}", e);
    }

    (StatusCode::OK, Json(json!({ "ok": true })))
}

pub async fn move_note(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
    Json(req): Json<MoveNoteRequest>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    match state
        .books
        .move_note(
            &book_id,
            &req.note_id,
            req.new_parent_id.as_deref(),
            req.index,
        )
        .await
    {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))),
        Err(plotweb_git::error::GitStoreError::CircularReference) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "cannot move note into its own subtree" })),
        ),
        Err(e) => {
            eprintln!("Failed to move note: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "failed to move note" })),
            )
        }
    }
}

pub async fn update_tree(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
    Json(req): Json<UpdateNoteTreeRequest>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    let tree = plotweb_git::note::NotesTreeJson {
        root_order: req.tree.root_order,
        children: req.tree.children,
        collapsed: req.tree.collapsed,
    };

    if let Err(e) = state.books.update_note_tree(&book_id, &tree).await {
        eprintln!("Failed to update note tree: {}", e);
    }

    (StatusCode::OK, Json(json!({ "ok": true })))
}
