//! Personal access tokens — the credentials an account's AI agent uses.
//!
//! Management (`GET`/`POST /api/tokens`, `DELETE /api/tokens/{id}`) is
//! **session-only**: a token can never mint, list or revoke tokens, so a leaked
//! one cannot entrench itself. `GET /api/tokens/whoami` is the one bearer route
//! here, for checking an agent's setup. See [`crate::token_auth`] for the token
//! format and the bearer extractor.

use std::collections::HashSet;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use plotweb_common::{
    ApiTokenInfo, CreateApiTokenRequest, CreateApiTokenResponse, TokenWhoAmI,
    MAX_API_TOKEN_LABEL_LEN,
};
use serde_json::json;
use uuid::Uuid;

use crate::auth::{hash_token, AuthSession};
use crate::rhype::{quote, Fields};
use crate::token_auth::{display_prefix, generate_api_token, row_to_info, TokenAuth};
use crate::AppState;

fn bad_request(msg: &str) -> axum::response::Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))).into_response()
}

/// `GET /api/tokens` — the caller's tokens, newest first. Metadata only.
pub async fn list(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
) -> impl IntoResponse {
    let rows = match state
        .rhype
        .find(format!("ApiToken.filter(.user_id == {})", quote(&user_id)))
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("list api tokens failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "failed to list tokens" })),
            )
                .into_response();
        }
    };
    let mut tokens: Vec<ApiTokenInfo> = rows.iter().map(row_to_info).collect();
    // Timestamps are fixed-width, so a string sort is chronological.
    tokens.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    (StatusCode::OK, Json(tokens)).into_response()
}

/// `POST /api/tokens` — mint a token. The raw value is in this response and
/// nowhere else, ever.
pub async fn create(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Json(req): Json<CreateApiTokenRequest>,
) -> impl IntoResponse {
    let label = req.label.trim().to_string();
    if label.is_empty() {
        return bad_request("a label is required");
    }
    if label.chars().count() > MAX_API_TOKEN_LABEL_LEN {
        return bad_request("label is too long (64 characters at most)");
    }

    let book_ids = match req.book_ids {
        None => None,
        Some(ids) => {
            let mut seen = HashSet::new();
            let ids: Vec<String> = ids
                .into_iter()
                .map(|id| id.trim().to_string())
                .filter(|id| seen.insert(id.clone()))
                .collect();
            if ids.is_empty() {
                return bad_request("choose at least one book, or allow all books");
            }
            for id in &ids {
                if !crate::routes::verify_book_ownership(&state, id, &user_id).await {
                    // Same answer for "doesn't exist" and "someone else's".
                    return bad_request("unknown book");
                }
            }
            Some(ids)
        }
    };

    let token = generate_api_token();
    let id = Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let prefix = display_prefix(&token);
    let scope_json = book_ids
        .as_ref()
        .map(|ids| serde_json::to_string(ids).unwrap_or_else(|_| "[]".into()));

    let fields = Fields::new()
        .str("uuid", &id)
        .str("user_id", &user_id)
        .str("label", &label)
        .str("token_hash", &hash_token(&token))
        .str("prefix", &prefix)
        .opt_str("book_ids_json", scope_json.as_deref())
        .str("created_at", &created_at)
        .render();

    if let Err(e) = state.rhype.create(format!("ApiToken.create({fields})")).await {
        // `e` describes the query failure; the query holds the hash, never the
        // raw token, and the engine error does not echo field values anyway.
        eprintln!("api token create failed: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to create token" })),
        )
            .into_response();
    }

    let info = ApiTokenInfo {
        id,
        label,
        prefix,
        book_ids,
        created_at,
        last_used_at: None,
    };
    (StatusCode::CREATED, Json(CreateApiTokenResponse { token, info })).into_response()
}

/// `DELETE /api/tokens/{id}` — revoke. 404 for a token that is not the caller's,
/// whether or not it exists.
pub async fn revoke(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let filter = format!(
        "ApiToken.filter(.uuid == {} && .user_id == {})",
        quote(&id),
        quote(&user_id)
    );
    let owned = state
        .rhype
        .exists(format!("{filter}.limit(1)"))
        .await
        .unwrap_or(false);
    if !owned {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "token not found" })),
        );
    }
    if let Err(e) = state.rhype.exec(format!("{filter}.delete()")).await {
        eprintln!("api token revoke failed: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to revoke token" })),
        );
    }
    (StatusCode::OK, Json(json!({ "ok": true })))
}

/// `GET /api/tokens/whoami` — bearer auth. Who this token acts as, and its scope.
pub async fn whoami(State(state): State<AppState>, auth: TokenAuth) -> impl IntoResponse {
    let username = state
        .rhype
        .find_one(format!(
            "User.filter(.uuid == {}).limit(1)",
            quote(&auth.user_id)
        ))
        .await
        .ok()
        .flatten()
        .and_then(|u| u.string("username"))
        .unwrap_or_default();
    Json(TokenWhoAmI {
        user_id: auth.user_id,
        username,
        token_label: auth.label,
        book_ids: auth.book_scope,
    })
}
