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

    match state.books.list_notes(&book_id).await {
        Ok((notes, tree)) => {
            // Cut over: titles, colours and the tree come from the canonical document;
            // bodies and timestamps stay git's. Same overlay as the chapter list, and
            // for the same reason.
            let (notes, tree) = match super::cutover_structure(&state, &book_id).await {
                Some(structure) => {
                    let listed = structure
                        .note_titles
                        .iter()
                        .map(|(id, title)| {
                            let git = notes.iter().find(|n| &n.id == id);
                            Note {
                                id: id.clone(),
                                book_id: book_id.clone(),
                                title: title.clone(),
                                content: git.map(|n| n.content.clone()).unwrap_or_default(),
                                color: structure.note_colors.get(id).cloned(),
                                created_at: git.map(|n| n.created_at.clone()).unwrap_or_default(),
                                updated_at: git.map(|n| n.updated_at.clone()).unwrap_or_default(),
                            }
                        })
                        .collect();
                    let tree = NoteTree {
                        root_order: structure.root_order.clone(),
                        children: structure.children.clone().into_iter().collect(),
                        collapsed: structure.collapsed.iter().cloned().collect(),
                    };
                    (listed, tree)
                }
                None => (
                    notes
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
                        .collect(),
                    NoteTree {
                        root_order: tree.root_order,
                        children: tree.children,
                        collapsed: tree.collapsed,
                    },
                ),
            };
            let resp = NotesResponse { notes, tree };
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
            // Cut over: the body comes from the canonical document, with git as the
            // fallback when there is no canonical copy (see routes::cutover_body).
            let content = match super::cutover_body(
                &state,
                &book_id,
                &format!("note:{}", n.id),
                &n.content,
                plotweb_crdt::BodyKind::Note,
            ) {
                super::CutoverRead::Git => n.content,
                super::CutoverRead::Canonical(content) => content,
            };
            let note = Note {
                id: n.id,
                book_id,
                title: n.title,
                content,
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
            super::apply_cutover_structure(&state, &book_id, &[]).await;
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

    // See `chapters::update`: the declaration only counts for a cut-over book. Title
    // and colour are structure and are never covered by it.
    let sync_owns_body = req.sync_owned && state.cutover.is_cut_over(&book_id);
    let content = if sync_owns_body { None } else { req.content.clone() };

    // For color, if it's present in the request we pass Some(value), otherwise None (don't update)
    let color = req.color.as_ref().map(|c| Some(c.as_str()));

    if let Err(e) = state
        .books
        .update_note(
            &book_id,
            &note_id,
            req.title.as_deref(),
            content.as_deref(),
            color,
        )
        .await
    {
        eprintln!("Failed to update note: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to save note" })),
        );
    }

    if let Some(content) = content.as_deref() {
        super::apply_cutover_body(
            &state,
            &book_id,
            &format!("note:{note_id}"),
            "note",
            content,
            plotweb_crdt::BodyKind::Note,
        )
        .await;
    }
    // A note's title and colour live in the book structure, its body does not — so an
    // autosave of the body alone skips the book read.
    if req.title.is_some() || req.color.is_some() {
        super::apply_cutover_structure(&state, &book_id, &[]).await;
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
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to delete note" })),
        );
    }
    // As with chapters: removal from the tree is the deletion (§D7), and stated rather
    // than inferred from git's silence.
    super::apply_cutover_structure(&state, &book_id, &[note_id.clone()]).await;

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
        Ok(()) => {
            super::apply_cutover_structure(&state, &book_id, &[]).await;
            (StatusCode::OK, Json(json!({ "ok": true })))
        }
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
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to save note tree" })),
        );
    }
    super::apply_cutover_structure(&state, &book_id, &[]).await;

    (StatusCode::OK, Json(json!({ "ok": true })))
}
