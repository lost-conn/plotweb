use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use plotweb_common::*;
use serde_json::json;
use tower_sessions::{Expiry, Session};
use uuid::Uuid;

use crate::auth::{self, AuthSession};
use crate::rhype::{quote, Fields};
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
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let fields = Fields::new()
        .str("uuid", &id)
        .str("username", &req.username)
        .str("email", &req.email)
        .str("password_hash", &password_hash)
        .str("created_at", &now)
        .render();

    match state.rhype.create(format!("User.create({fields})")).await {
        Ok(_) => {
            auth::set_session_user(&session, &id).await;
            let user = User {
                id,
                username: req.username,
                email: req.email,
                created_at: now,
            };
            (StatusCode::CREATED, Json(serde_json::to_value(user).unwrap()))
        }
        Err(e) => {
            if e.to_string().to_lowercase().contains("unique") {
                (
                    StatusCode::CONFLICT,
                    Json(json!({ "error": "username or email already taken" })),
                )
            } else {
                eprintln!("register failed: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "registration failed" })),
                )
            }
        }
    }
}

pub async fn login(
    State(state): State<AppState>,
    session: Session,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    let found = state
        .rhype
        .find_one(format!(
            "User.filter(.username == {}).limit(1)",
            quote(&req.username)
        ))
        .await;

    match found {
        Ok(Some(o)) => {
            let password_hash = o.string("password_hash").unwrap_or_default();
            if !auth::verify_password(&req.password, &password_hash) {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "invalid credentials" })),
                );
            }
            if !req.remember_me {
                session.set_expiry(Some(Expiry::OnSessionEnd));
            }
            let id = o.string("uuid").unwrap_or_default();
            auth::set_session_user(&session, &id).await;
            let user = User {
                id,
                username: o.string("username").unwrap_or_default(),
                email: o.string("email").unwrap_or_default(),
                created_at: o.string("created_at").unwrap_or_default(),
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
    let found = state
        .rhype
        .find_one(format!("User.filter(.uuid == {}).limit(1)", quote(&user_id)))
        .await;

    match found {
        Ok(Some(o)) => {
            let user = User {
                id: o.string("uuid").unwrap_or_default(),
                username: o.string("username").unwrap_or_default(),
                email: o.string("email").unwrap_or_default(),
                created_at: o.string("created_at").unwrap_or_default(),
            };
            (StatusCode::OK, Json(serde_json::to_value(user).unwrap()))
        }
        _ => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "user not found" })),
        ),
    }
}
