use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use axum_extra::extract::Multipart;
use plotweb_common::*;
use serde_json::json;
use uuid::Uuid;

use crate::auth::AuthSession;
use crate::AppState;

const MAX_IMAGE_SIZE: usize = 10 * 1024 * 1024; // 10MB

fn allowed_extension(ext: &str) -> bool {
    matches!(ext, "jpg" | "jpeg" | "png" | "gif" | "webp")
}

fn content_type_for_ext(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

fn images_dir(state: &AppState, book_id: &str) -> std::path::PathBuf {
    state.books.base_dir().join(book_id).join("images")
}

fn validate_filename(filename: &str) -> bool {
    !filename.contains('/') && !filename.contains('\\') && !filename.contains("..")
}

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

/// POST /api/books/{book_id}/images
pub async fn upload(
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

    // Read file from multipart
    let (filename, data) = {
        let mut result = None;
        while let Ok(Some(field)) = multipart.next_field().await {
            if field.name() == Some("file") {
                let fname = field
                    .file_name()
                    .unwrap_or("image.png")
                    .to_string();
                match field.bytes().await {
                    Ok(bytes) => {
                        result = Some((fname, bytes.to_vec()));
                        break;
                    }
                    Err(e) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "error": format!("failed to read file: {}", e) })),
                        );
                    }
                }
            }
        }
        match result {
            Some(r) => r,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "no file uploaded" })),
                );
            }
        }
    };

    // Validate size
    if data.len() > MAX_IMAGE_SIZE {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "image too large (max 10MB)" })),
        );
    }

    // Validate extension
    let ext = filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_lowercase();
    if !allowed_extension(&ext) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "unsupported image format (use jpg, png, gif, or webp)" })),
        );
    }

    // Generate UUID filename
    let new_filename = format!("{}.{}", Uuid::new_v4(), ext);
    let dir = images_dir(&state, &book_id);

    // Write to disk
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("Failed to create images dir: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to save image" })),
        );
    }

    let path = dir.join(&new_filename);
    if let Err(e) = std::fs::write(&path, &data) {
        eprintln!("Failed to write image: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to save image" })),
        );
    }

    let url = format!("/api/books/{}/images/{}", book_id, new_filename);
    let resp = ImageUploadResponse {
        url,
        filename: new_filename,
    };
    (StatusCode::CREATED, Json(serde_json::to_value(resp).unwrap()))
}

/// GET /api/books/{book_id}/images/{filename}
pub async fn serve(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, filename)): Path<(String, String)>,
) -> impl IntoResponse {
    if !validate_filename(&filename) {
        return StatusCode::BAD_REQUEST.into_response();
    }

    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return StatusCode::NOT_FOUND.into_response();
    }

    serve_image_file(&state, &book_id, &filename).await
}

/// GET /api/beta/{token}/images/{filename}
pub async fn serve_beta(
    State(state): State<AppState>,
    Path((token, filename)): Path<(String, String)>,
) -> impl IntoResponse {
    if !validate_filename(&filename) {
        return StatusCode::BAD_REQUEST.into_response();
    }

    // Look up book_id from token
    let row = sqlx::query_as::<_, (String, i64)>(
        "SELECT book_id, active FROM beta_reader_links WHERE token = ?",
    )
    .bind(&token)
    .fetch_optional(&state.db)
    .await;

    let (book_id, active) = match row {
        Ok(Some(r)) => r,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };

    if active == 0 {
        return StatusCode::FORBIDDEN.into_response();
    }

    serve_image_file(&state, &book_id, &filename).await
}

async fn serve_image_file(state: &AppState, book_id: &str, filename: &str) -> axum::response::Response {
    let path = images_dir(state, book_id).join(filename);

    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    let ext = filename
        .rsplit('.')
        .next()
        .unwrap_or("");
    let content_type = content_type_for_ext(ext);

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        Body::from(data),
    )
        .into_response()
}

/// DELETE /api/books/{book_id}/images/{filename}
pub async fn delete(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path((book_id, filename)): Path<(String, String)>,
) -> impl IntoResponse {
    if !validate_filename(&filename) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid filename" })),
        );
    }

    if !verify_book_ownership(&state, &book_id, &user_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "book not found" })),
        );
    }

    let path = images_dir(&state, &book_id).join(&filename);
    if path.exists() {
        std::fs::remove_file(&path).ok();
    }

    (StatusCode::OK, Json(json!({ "ok": true })))
}
