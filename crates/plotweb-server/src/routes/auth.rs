use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use plotweb_common::*;
use serde_json::json;
use tower_sessions::{Expiry, Session};
use uuid::Uuid;

use crate::auth::{self, AuthSession};
use crate::AppState;

pub async fn register(
    State(state): State<AppState>,
    session: Session,
    Json(req): Json<RegisterRequest>,
) -> impl IntoResponse {
    if req.username.trim().is_empty() || req.password.is_empty() || req.email.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "all fields are required" })),
        );
    }

    let password_hash = match auth::hash_password(&req.password) {
        Ok(h) => h,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "failed to hash password" })),
            )
        }
    };

    let id = Uuid::new_v4().to_string();
    let result = sqlx::query(
        "INSERT INTO users (id, username, email, password_hash) VALUES (?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&req.username)
    .bind(&req.email)
    .bind(&password_hash)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => {
            auth::set_session_user(&session, &id).await;
            let user = User {
                id,
                username: req.username,
                email: req.email,
                created_at: String::new(),
            };
            (StatusCode::CREATED, Json(serde_json::to_value(user).unwrap()))
        }
        Err(e) => {
            let msg = if e.to_string().contains("UNIQUE") {
                "username or email already taken"
            } else {
                "registration failed"
            };
            (
                StatusCode::CONFLICT,
                Json(json!({ "error": msg })),
            )
        }
    }
}

pub async fn login(
    State(state): State<AppState>,
    session: Session,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    let row = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT id, username, email, password_hash, created_at FROM users WHERE username = ?",
    )
    .bind(&req.username)
    .fetch_optional(&state.db)
    .await;

    match row {
        Ok(Some((id, username, email, password_hash, created_at))) => {
            if !auth::verify_password(&req.password, &password_hash) {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "invalid credentials" })),
                );
            }
            if !req.remember_me {
                session.set_expiry(Some(Expiry::OnSessionEnd));
            }
            auth::set_session_user(&session, &id).await;
            let user = User {
                id,
                username,
                email,
                created_at,
            };
            (StatusCode::OK, Json(serde_json::to_value(user).unwrap()))
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid credentials" })),
        ),
    }
}

pub async fn logout(session: Session) -> impl IntoResponse {
    auth::clear_session(&session).await;
    (StatusCode::OK, Json(json!({ "ok": true })))
}

pub async fn me(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
) -> impl IntoResponse {
    let row = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT id, username, email, created_at FROM users WHERE id = ?",
    )
    .bind(&user_id)
    .fetch_optional(&state.db)
    .await;

    match row {
        Ok(Some((id, username, email, created_at))) => {
            let user = User {
                id,
                username,
                email,
                created_at,
            };
            (StatusCode::OK, Json(serde_json::to_value(user).unwrap()))
        }
        _ => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "user not found" })),
        ),
    }
}
