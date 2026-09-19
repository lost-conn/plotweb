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

/// A note as git holds it, with its link index derived from the body git holds.
///
/// The one adaptation, so a facet cannot reach the list and miss the single-note read.
/// Edges are derived rather than stored (see `plotweb_common::note_links`), and this is
/// the non-cut-over path — a cut-over read overlays the canonical structure's copy on
/// top, because git's body lags it by the mirror's debounce.
fn git_note(book_id: &str, n: plotweb_git::note::NoteData, index: &LinkIndex) -> Note {
    Note {
        links: extract_note_links_in(&n.content, index),
        id: n.id,
        book_id: book_id.to_string(),
        title: n.title,
        content: n.content,
        color: n.color,
        created_at: n.created_at,
        updated_at: n.updated_at,
        span: n.span,
        relative: n.relative,
        is_entity: n.is_entity,
        event_parent: n.event_parent,
        pinned: n.pinned,
    }
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
                                // Facets come from the canonical structure for the same
                                // reason titles do: it is the source of truth for a
                                // cut-over book, and it is the copy the timeline will be
                                // drawn from.
                                span: structure.note_spans.get(id).cloned(),
                                relative: structure.note_relatives.get(id).cloned(),
                                is_entity: structure.note_entities.contains(id),
                                event_parent: structure.note_event_parents.get(id).cloned(),
                                pinned: structure.note_pinned.contains(id),
                                links: structure.note_links.get(id).cloned().unwrap_or_default(),
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
                None => {
                    let index = crate::structure::read_link_index(&state.books, &book_id).await;
                    (
                    notes
                        .into_iter()
                        .map(|n| git_note(&book_id, n, &index))
                        .collect(),
                    NoteTree {
                        root_order: tree.root_order,
                        children: tree.children,
                        collapsed: tree.collapsed,
                    },
                )
                }
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
                super::CutoverRead::Git => n.content.clone(),
                super::CutoverRead::Canonical(content) => content,
            };
            let index = crate::structure::read_link_index(&state.books, &book_id).await;
            let mut note = git_note(&book_id, n, &index);
            note.content = content;
            // Facets are structure, so for a cut-over book they come from the canonical
            // document — git's copy of them lags by the mirror's debounce exactly as its
            // titles do.
            if let Some(structure) = super::cutover_structure(&state, &book_id).await {
                let id = &note.id;
                note.span = structure.note_spans.get(id).cloned();
                note.relative = structure.note_relatives.get(id).cloned();
                note.is_entity = structure.note_entities.contains(id);
                note.event_parent = structure.note_event_parents.get(id).cloned();
                note.pinned = structure.note_pinned.contains(id);
                note.links = structure.note_links.get(id).cloned().unwrap_or_default();
            }
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
            // A new note is lore — no span, no entity mark, no event parent, and an
            // empty body, so no edges either. Facets only ever arrive by a later edit.
            let note = git_note(&book_id, n, &LinkIndex::new());
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

    // See `chapters::update`: for a cut-over book sync is the only writer of a body,
    // and only where the server can confirm the canonical document is one it can
    // actually read — otherwise nothing carries the edit at all. Title and colour are
    // structure, which REST still carries for every book.
    let doc_id = format!("note:{note_id}");
    let cut_over = state.cutover.is_cut_over(&book_id);
    let sync_owns_body = cut_over && super::canonical_is_authoritative(&state, &book_id, &doc_id);
    let degraded = cut_over && !sync_owns_body && req.content.is_some();

    let carries_content = req.content.is_some();
    let content = if sync_owns_body { None } else { req.content.clone() };

    // Facets are structure, like the title: REST carries them for every book, cut over
    // or not, and they go to git even when sync owns the body.
    let facets = plotweb_git::note::NoteFacetPatch {
        span: req.span.clone(),
        relative: req.relative.clone(),
        is_entity: req.is_entity,
        event_parent: req.event_parent.clone(),
        pinned: req.pinned,
    };
    let wrote_to_git =
        content.is_some() || req.title.is_some() || req.color.is_some() || !facets.is_empty();

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
            facets.clone(),
        )
        .await
    {
        eprintln!("Failed to update note: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to save note" })),
        );
    }

    // The canonical copy could not carry it: clear the claim, so the next write does
    // not stand down for a writer that cannot deliver.
    if degraded {
        let crdt_dir = state.crdt_dir.clone();
        let doc = doc_id.clone();
        if let Ok(Err(e)) =
            tokio::task::spawn_blocking(move || crate::sync::disown_canonical(&crdt_dir, &doc))
                .await
        {
            eprintln!("[cutover] {doc_id}: could not clear a stale sync claim: {e}");
        }
    }

    let applied_to_canonical = match content.as_deref() {
        Some(content) => {
            super::apply_cutover_body(
                &state,
                &book_id,
                &doc_id,
                "note",
                content,
                plotweb_crdt::BodyKind::Note,
            )
            .await
        }
        None => false,
    };
    // A note's title, colour and facets live in the book structure — and so, since the
    // notes revamp, does the link index derived from its body. So an autosave of the
    // body alone no longer skips the book read: the timeline is drawn from the structure
    // document, and stale `$ref` edges there would put a character in the wrong scene.
    //
    // The body is passed through rather than re-read, because when sync owns it git has
    // not seen this text yet and would yield the *previous* index.
    match req.content.as_deref() {
        Some(content) => {
            super::apply_cutover_structure_with_note_body(&state, &book_id, &note_id, content)
                .await
        }
        None if req.title.is_some() || req.color.is_some() || !facets.is_empty() => {
            super::apply_cutover_structure(&state, &book_id, &[]).await
        }
        None => {}
    }

    let mut receipt = SaveReceipt {
        git: wrote_to_git,
        canonical: applied_to_canonical,
        deferred_to_sync: sync_owns_body,
        warning: None,
    };
    receipt.warning = super::save_warning(degraded, carries_content, receipt.is_durable());
    (StatusCode::OK, Json(serde_json::to_value(receipt).unwrap()))
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
