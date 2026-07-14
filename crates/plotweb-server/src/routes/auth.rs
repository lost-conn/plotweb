use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use plotweb_common::*;
use serde_json::json;
use tower_sessions::{Expiry, Session};
use uuid::Uuid;

use std::sync::LazyLock;

use crate::auth::{self, AuthSession};
use crate::rhype::{quote, Fields};
use crate::AppState;

/// A valid argon2 PHC hash of a fixed throwaway password, computed once. Used to
/// run an equivalent verify in the user-not-found branch of login so response
/// timing doesn't reveal whether a username exists.
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    auth::hash_password("dummy-password-for-timing").unwrap_or_default()
});

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
        _ => {
            // Run argon2 against a dummy hash so a missing user takes the same
            // time as a wrong password (no timing oracle on username existence).
            let _ = auth::verify_password(&req.password, &DUMMY_HASH);
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "invalid credentials" })),
            )
        }
    }
}

pub async fn logout(session: Session) -> impl IntoResponse {
    auth::clear_session(&session).await;
    (StatusCode::OK, Json(json!({ "ok": true })))
}

/// Request a password-reset link. Always responds `200 { ok: true }` with an
/// identical body whether or not the email maps to an account, so it can't be
/// used to probe which addresses are registered (same principle as the
/// timing-equalised login above). If a user is found we issue a single-use,
/// 1-hour token — storing only its hash — and email the raw token.
pub async fn forgot_password(
    State(state): State<AppState>,
    Json(req): Json<ForgotPasswordRequest>,
) -> impl IntoResponse {
    let email = req.email.trim();
    let mut body = json!({ "ok": true });

    if !email.is_empty() {
        if let Ok(Some(user)) = state
            .rhype
            .find_one(format!("User.filter(.email == {}).limit(1)", quote(email)))
            .await
        {
            let user_id = user.string("uuid").unwrap_or_default();
            let token = auth::generate_token();
            let token_hash = auth::hash_token(&token);
            let now = chrono::Utc::now();
            let created_at = now.format("%Y-%m-%d %H:%M:%S").to_string();
            let expires_at = (now + chrono::Duration::hours(1))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string();

            // One active token per user: clear any earlier ones first.
            let _ = state
                .rhype
                .exec(format!(
                    "PasswordReset.filter(.user_id == {}).delete()",
                    quote(&user_id)
                ))
                .await;

            let id = Uuid::new_v4().to_string();
            let fields = Fields::new()
                .str("uuid", &id)
                .str("user_id", &user_id)
                .str("token_hash", &token_hash)
                .str("expires_at", &expires_at)
                .str("created_at", &created_at)
                .render();

            match state
                .rhype
                .create(format!("PasswordReset.create({fields})"))
                .await
            {
                Ok(_) => {
                    if let Some(mailer) = &state.email {
                        let to = user.string("email").unwrap_or_default();
                        mailer.send_password_reset(&to, &token).await;
                    }
                    // Dev convenience: with no mailer configured in a debug
                    // build, hand the token back so the flow is usable locally.
                    // Never happens in a release build.
                    if state.email.is_none() && cfg!(debug_assertions) {
                        body = json!({ "ok": true, "reset_token": token });
                    }
                }
                Err(e) => eprintln!("password reset token create failed: {e}"),
            }
        }
    }

    (StatusCode::OK, Json(body))
}

/// Redeem a reset token for a new password. Looks the token up by its hash,
/// rejects it if missing/expired, then updates the user's `password_hash` and
/// deletes every outstanding token for that user (single-use). All failure
/// modes return the same generic 400 so a valid-but-expired token isn't
/// distinguishable from a bogus one.
pub async fn reset_password(
    State(state): State<AppState>,
    Json(req): Json<ResetPasswordRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if req.new_password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "password is required" })),
        );
    }

    let invalid = || {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid or expired token" })),
        )
    };

    let token_hash = auth::hash_token(req.token.trim());
    let reset = match state
        .rhype
        .find_one(format!(
            "PasswordReset.filter(.token_hash == {}).limit(1)",
            quote(&token_hash)
        ))
        .await
    {
        Ok(Some(o)) => o,
        _ => return invalid(),
    };

    // Fixed-width timestamps compare lexicographically in chronological order.
    let expires_at = reset.string("expires_at").unwrap_or_default();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if expires_at.is_empty() || now >= expires_at {
        let _ = state
            .rhype
            .exec(format!(
                "PasswordReset.filter(.token_hash == {}).delete()",
                quote(&token_hash)
            ))
            .await;
        return invalid();
    }

    let user_id = reset.string("user_id").unwrap_or_default();
    let password_hash = match auth::hash_password(&req.new_password) {
        Ok(h) => h,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "failed to hash password" })),
            )
        }
    };

    if let Err(e) = state
        .rhype
        .exec(format!(
            "User.filter(.uuid == {}).update({})",
            quote(&user_id),
            Fields::new().str("password_hash", &password_hash).render()
        ))
        .await
    {
        eprintln!("password reset update failed: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to reset password" })),
        );
    }

    // Single-use: drop every outstanding token for this user.
    let _ = state
        .rhype
        .exec(format!(
            "PasswordReset.filter(.user_id == {}).delete()",
            quote(&user_id)
        ))
        .await;

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
