use std::collections::HashSet;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use plotweb_export::{ExportChapter, ExportFormat, ExportInput, export};
use serde::Deserialize;
use serde_json::json;

use crate::AppState;
use crate::auth::AuthSession;
use crate::routes::verify_book_ownership;

#[derive(Deserialize)]
pub struct ExportParams {
    /// One of: `md`, `docx`, `epub`, `pdf`. Defaults to `md` when absent.
    #[serde(default)]
    format: Option<String>,
    /// Comma-separated chapter ids to include. Absent/empty = whole book.
    #[serde(default)]
    chapters: Option<String>,
}

/// GET /api/books/{book_id}/export?format=md&chapters=id1,id2
///
/// Builds the manuscript in the requested format and returns it as a file
/// download. Reuses ownership verification and the git-backed chapter list
/// (which is already in book order).
pub async fn export_book(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
    Query(params): Query<ExportParams>,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        )
            .into_response();
    }

    let format = match params
        .format
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(s) => match ExportFormat::parse(s) {
            Some(f) => f,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "unsupported export format" })),
                )
                    .into_response();
            }
        },
        None => ExportFormat::Markdown,
    };

    let book = match state.books.get_book(&book_id).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "book not found" })),
            )
                .into_response();
        }
    };

    let all = match state.books.list_chapters(&book_id).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("export: failed to list chapters: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "failed to load chapters" })),
            )
                .into_response();
        }
    };

    // Optional subset filter. Selection is by membership only — output stays in
    // book order, never the order the ids were listed in the query.
    let selected: Option<HashSet<String>> = params
        .chapters
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .filter(|set: &HashSet<String>| !set.is_empty());

    let chapters: Vec<ExportChapter> = all
        .into_iter()
        .filter(|ch| selected.as_ref().is_none_or(|set| set.contains(&ch.id)))
        .map(|ch| ExportChapter {
            title: ch.title,
            content: ch.content,
        })
        .collect();

    if chapters.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "no chapters to export" })),
        )
            .into_response();
    }

    let filename = format!("{}.{}", slugify(&book.title), format.extension());
    let input = ExportInput {
        title: book.title,
        description: book.description,
        chapters,
    };

    // Rendering is CPU-bound; keep it off the async reactor.
    let bytes = match tokio::task::spawn_blocking(move || export(&input, format)).await {
        Ok(Ok(b)) => b,
        Ok(Err(e)) => {
            let code = match e {
                plotweb_export::ExportError::Unsupported(_) => StatusCode::NOT_IMPLEMENTED,
                plotweb_export::ExportError::Render(_) => StatusCode::INTERNAL_SERVER_ERROR,
            };
            return (code, Json(json!({ "error": e.to_string() }))).into_response();
        }
        Err(e) => {
            eprintln!("export: render task failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "export failed" })),
            )
                .into_response();
        }
    };

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, format.mime().to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        bytes,
    )
        .into_response()
}

/// Filesystem-safe ASCII slug for the download filename.
fn slugify(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut prev_dash = false;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "manuscript".to_string()
    } else {
        trimmed.to_string()
    }
}
