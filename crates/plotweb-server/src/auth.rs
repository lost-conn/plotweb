use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use password_hash::SaltString;
use rand_core::{OsRng, RngCore};
use serde_json::json;
use sha2::{Digest, Sha256};
use tower_sessions::Session;

const USER_ID_KEY: &str = "user_id";

pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(password.as_bytes(), &salt)?;
    Ok(hash.to_string())
}

/// Generate a high-entropy password-reset token: 32 random bytes (256 bits) from
/// the OS CSPRNG, hex-encoded to a URL-safe 64-char string. This raw token is
/// only ever emailed — never stored (see [`hash_token`]).
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Deterministic SHA-256 (hex) of a reset token, for storage and lookup. The DB
/// holds only this hash, so a store leak can't be turned into a valid token.
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub async fn set_session_user(session: &Session, user_id: &str) {
    session
        .insert(USER_ID_KEY, user_id.to_string())
        .await
        .ok();
}

pub async fn clear_session(session: &Session) {
    session.flush().await.ok();
}

/// Extractor that requires an authenticated user, returning their user_id.
pub struct AuthSession(pub String);

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let body = json!({ "error": "unauthorized" });
        (StatusCode::UNAUTHORIZED, axum::Json(body)).into_response()
    }
}

pub struct AuthError;

impl<S: Send + Sync> FromRequestParts<S> for AuthSession {
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let session = parts.extensions.get::<Session>().ok_or(AuthError)?;
        let user_id: String = session.get(USER_ID_KEY).await.ok().flatten().ok_or(AuthError)?;
        Ok(AuthSession(user_id))
    }
}
