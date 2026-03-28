use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use axum_extra::extract::Multipart;
use plotweb_common::*;
use serde_json::json;

use crate::auth::AuthSession;
use crate::AppState;

/// Verify the book belongs to the user.
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

/// Read a single file field from the multipart upload.
async fn read_file_field(multipart: &mut Multipart) -> Result<(String, Vec<u8>), String> {
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("file") {
            let filename = field
                .file_name()
                .unwrap_or("unknown.txt")
                .to_string();
            let data = field
                .bytes()
                .await
                .map_err(|e| format!("failed to read file: {}", e))?;
            return Ok((filename, data.to_vec()));
        }
    }
    Err("no file uploaded".to_string())
}

/// Parse an uploaded file, returning the detected chapters and filename.
fn parse_upload(
    filename: &str,
    data: &[u8],
) -> Result<Vec<plotweb_import::DetectedChapter>, (StatusCode, serde_json::Value)> {
    let format = plotweb_import::ImportFormat::from_filename(filename).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            json!({ "error": "Unsupported file format. Please upload a .md, .txt, or .docx file." }),
        )
    })?;

    plotweb_import::parse_manuscript(data, format).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            json!({ "error": format!("Failed to parse file: {}", e) }),
        )
    })
}

/// POST /api/books/{book_id}/import/preview
///
/// Accepts a multipart file upload, parses the manuscript, and returns
/// a preview of the detected chapters without creating anything.
pub async fn preview(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    let (filename, data) = match read_file_field(&mut multipart).await {
        Ok(v) => v,
        Err(msg) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))),
    };

    let chapters = match parse_upload(&filename, &data) {
        Ok(c) => c,
        Err((status, val)) => return (status, Json(val)),
    };

    let preview_chapters: Vec<ImportPreviewChapter> = chapters
        .iter()
        .map(|ch| {
            let word_count = ch.content.split_whitespace().count();
            let preview = if ch.content.len() > 200 {
                format!("{}...", &ch.content[..200])
            } else {
                ch.content.clone()
            };
            ImportPreviewChapter {
                title: ch.title.clone(),
                content_preview: preview,
                word_count,
            }
        })
        .collect();

    let resp = ImportPreviewResponse {
        chapters: preview_chapters,
        filename,
    };
    (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()))
}

/// POST /api/books/{book_id}/import/confirm
///
/// Accepts a multipart file upload, re-parses the manuscript, and creates
/// all detected chapters in the book.
pub async fn confirm(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(book_id): Path<String>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    let (filename, data) = match read_file_field(&mut multipart).await {
        Ok(v) => v,
        Err(msg) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))),
    };

    let detected = match parse_upload(&filename, &data) {
        Ok(c) => c,
        Err((status, val)) => return (status, Json(val)),
    };

    if detected.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "no chapters detected" })),
        );
    }

    let import_chapters: Vec<ImportChapter> = detected
        .into_iter()
        .map(|ch| ImportChapter {
            title: ch.title,
            content: ch.content,
        })
        .collect();

    match state
        .books
        .import_chapters(&book_id, &import_chapters)
        .await
    {
        Ok(chapters) => {
            let chapters: Vec<Chapter> = chapters
                .into_iter()
                .map(|ch| Chapter {
                    id: ch.id,
                    book_id: book_id.clone(),
                    title: ch.title,
                    content: ch.content,
                    sort_order: ch.sort_order,
                    word_count: ch.word_count,
                    created_at: ch.created_at,
                    updated_at: ch.updated_at,
                })
                .collect();
            (
                StatusCode::CREATED,
                Json(serde_json::to_value(chapters).unwrap()),
            )
        }
        Err(e) => {
            eprintln!("Failed to import chapters: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "failed to import chapters" })),
            )
        }
    }
}
