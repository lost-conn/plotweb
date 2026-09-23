//! Personal access tokens: generation, and the `TokenAuth` bearer extractor.
//!
//! A token is `pw_` followed by 32 bytes from the OS CSPRNG, base64url-encoded
//! without padding (43 characters). The raw token is returned to its owner once,
//! at creation, and never stored or logged: the `ApiToken` row holds only its
//! SHA-256 ([`crate::auth::hash_token`]), so a request is authenticated by hashing
//! the presented token and looking the hash up.
//!
//! [`TokenAuth`] is deliberately a *separate* extractor from
//! [`crate::auth::AuthSession`]. Session routes never look at `Authorization`, so a
//! token cannot reach anything that was not written to accept one — and routes
//! that accept one must still ask [`TokenAuth::can_access_book`] per book, which
//! combines the account's ownership with the token's own scope.

use axum::extract::FromRequestParts;
use axum::http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use plotweb_common::{ApiTokenInfo, API_TOKEN_MARKER};
use rand_core::{OsRng, RngCore};
use serde_json::json;

use crate::auth::hash_token;
use crate::rhype::{quote, Fields, RhypeObject};
use crate::AppState;

/// How many characters after the marker are kept as the display prefix.
pub const PREFIX_LEN: usize = 8;

/// `last_used_at` is rewritten at most this often per token, so an agent making
/// many calls a minute does not turn every read into a metadata write.
pub const LAST_USED_THROTTLE_SECS: i64 = 60;

/// Longest header value we bother hashing. A real token is 46 characters; this
/// only bounds the work an absurd header can cause.
const MAX_TOKEN_LEN: usize = 256;

const TIME_FMT: &str = "%Y-%m-%d %H:%M:%S";

/// A fresh raw token: `pw_` + 32 CSPRNG bytes, base64url without padding.
pub fn generate_api_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    format!(
        "{API_TOKEN_MARKER}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

/// The non-secret display prefix of a raw token: the first [`PREFIX_LEN`]
/// characters after the marker. 48 bits of a 256-bit token, which identifies it
/// to its owner without meaningfully helping anyone guess the rest.
pub fn display_prefix(raw: &str) -> String {
    raw.strip_prefix(API_TOKEN_MARKER)
        .unwrap_or(raw)
        .chars()
        .take(PREFIX_LEN)
        .collect()
}

/// Decode a row's `book_ids_json`: absent (or unreadable) = all books.
///
/// Unreadable failing *open* would be wrong for a scope, so an unparseable value
/// is treated as an empty list — a token that reaches nothing — rather than as
/// "all books".
pub fn parse_scope(row: &RhypeObject) -> Option<Vec<String>> {
    let raw = row.str("book_ids_json")?;
    Some(serde_json::from_str::<Vec<String>>(raw).unwrap_or_default())
}

/// The owner-facing view of an `ApiToken` row. Never includes the hash.
pub fn row_to_info(row: &RhypeObject) -> ApiTokenInfo {
    ApiTokenInfo {
        id: row.string("uuid").unwrap_or_default(),
        label: row.string("label").unwrap_or_default(),
        prefix: row.string("prefix").unwrap_or_default(),
        book_ids: parse_scope(row),
        created_at: row.string("created_at").unwrap_or_default(),
        last_used_at: row.string("last_used_at"),
    }
}

/// Whether a token last used at `last_used` (the stored string, if any) is due a
/// fresh `last_used_at` write at `now`. Pure so the throttle is unit-testable.
pub fn should_touch(last_used: Option<&str>, now: chrono::NaiveDateTime) -> bool {
    let Some(last) = last_used else {
        return true;
    };
    match chrono::NaiveDateTime::parse_from_str(last, TIME_FMT) {
        Ok(then) => (now - then).num_seconds() >= LAST_USED_THROTTLE_SECS,
        // Unparseable: overwrite it with a good value.
        Err(_) => true,
    }
}

/// An authenticated bearer-token request.
#[derive(Debug, Clone)]
pub struct TokenAuth {
    pub user_id: String,
    pub token_id: String,
    pub label: String,
    /// `None` = every book the account owns; `Some(ids)` = only those.
    pub book_scope: Option<Vec<String>>,
}

/// Rejection for a missing, malformed, unknown or revoked token. One shape for
/// every case, so a caller learns nothing about *why* beyond "not accepted".
#[derive(Debug)]
pub struct TokenAuthError;

impl IntoResponse for TokenAuthError {
    fn into_response(self) -> Response {
        (
            StatusCode::UNAUTHORIZED,
            [(WWW_AUTHENTICATE, "Bearer")],
            axum::Json(json!({ "error": "invalid or missing API token" })),
        )
            .into_response()
    }
}

/// Pull the raw token out of an `Authorization` header value, if it is a
/// well-formed `Bearer pw_…`.
fn bearer_token(header: &str) -> Option<&str> {
    let (scheme, rest) = header.trim().split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = rest.trim();
    if !token.starts_with(API_TOKEN_MARKER)
        || token.len() <= API_TOKEN_MARKER.len()
        || token.len() > MAX_TOKEN_LEN
    {
        return None;
    }
    Some(token)
}

impl TokenAuth {
    /// Authenticate an `Authorization` header value against the token store.
    ///
    /// Public (rather than only reachable through the extractor) so tests and
    /// future transports — the MCP endpoint may not read headers the same way —
    /// share one implementation.
    pub async fn authenticate(
        state: &AppState,
        authorization: Option<&str>,
    ) -> Result<TokenAuth, TokenAuthError> {
        let raw = authorization.and_then(bearer_token).ok_or(TokenAuthError)?;
        let token_hash = hash_token(raw);

        let row = state
            .rhype
            .find_one(format!(
                "ApiToken.filter(.token_hash == {}).limit(1)",
                quote(&token_hash)
            ))
            .await
            .ok()
            .flatten()
            .ok_or(TokenAuthError)?;

        let user_id = row.string("user_id").unwrap_or_default();
        let user_exists = !user_id.is_empty()
            && state
                .rhype
                .exists(format!(
                    "User.filter(.uuid == {}).limit(1)",
                    quote(&user_id)
                ))
                .await
                .unwrap_or(false);
        if !user_exists {
            return Err(TokenAuthError);
        }

        let now = chrono::Utc::now().naive_utc();
        if should_touch(row.str("last_used_at"), now) {
            // Best effort: a failed bookkeeping write must not fail the request.
            let _ = state
                .rhype
                .exec(format!(
                    "ApiToken.filter(.token_hash == {}).update({})",
                    quote(&token_hash),
                    Fields::new()
                        .str("last_used_at", &now.format(TIME_FMT).to_string())
                        .render()
                ))
                .await;
        }

        Ok(TokenAuth {
            user_id,
            token_id: row.string("uuid").unwrap_or_default(),
            label: row.string("label").unwrap_or_default(),
            book_scope: parse_scope(&row),
        })
    }

    /// Whether the token's scope names `book_id` (ignoring ownership).
    pub fn in_scope(&self, book_id: &str) -> bool {
        match &self.book_scope {
            None => true,
            Some(ids) => ids.iter().any(|id| id == book_id),
        }
    }

    /// Whether this token may touch `book_id`: the account must own the book
    /// (the same check every session route uses) **and** the token's scope must
    /// include it. Both, always — a scope is a restriction on the owner's access,
    /// never a grant beyond it.
    pub async fn can_access_book(&self, state: &AppState, book_id: &str) -> bool {
        self.in_scope(book_id)
            && crate::routes::verify_book_ownership(state, book_id, &self.user_id).await
    }
}

impl FromRequestParts<AppState> for TokenAuth {
    type Rejection = TokenAuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok());
        TokenAuth::authenticate(state, header).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> chrono::NaiveDateTime {
        chrono::NaiveDateTime::parse_from_str(s, TIME_FMT).unwrap()
    }

    #[test]
    fn generated_tokens_are_marked_long_and_url_safe() {
        let a = generate_api_token();
        let b = generate_api_token();
        assert_ne!(a, b);
        assert!(a.starts_with("pw_"));
        // 32 bytes -> 43 base64url characters, no padding.
        assert_eq!(a.len(), 3 + 43);
        assert!(a[3..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn prefix_is_the_characters_after_the_marker() {
        assert_eq!(display_prefix("pw_abcdefghijk"), "abcdefgh");
        assert_eq!(display_prefix("pw_abc"), "abc");
    }

    #[test]
    fn bearer_parsing() {
        assert_eq!(bearer_token("Bearer pw_abc"), Some("pw_abc"));
        assert_eq!(bearer_token("bearer   pw_abc  "), Some("pw_abc"));
        assert_eq!(bearer_token("Basic pw_abc"), None);
        assert_eq!(bearer_token("Bearer abc"), None);
        assert_eq!(bearer_token("Bearer pw_"), None);
        assert_eq!(bearer_token("pw_abc"), None);
        assert_eq!(bearer_token(""), None);
        let long = format!("Bearer pw_{}", "a".repeat(400));
        assert_eq!(bearer_token(&long), None);
    }

    #[test]
    fn last_used_is_throttled_to_once_a_minute() {
        let now = at("2026-09-23 12:00:00");
        assert!(should_touch(None, now));
        assert!(!should_touch(Some("2026-09-23 11:59:30"), now));
        assert!(should_touch(Some("2026-09-23 11:59:00"), now));
        assert!(should_touch(Some("2026-09-22 12:00:00"), now));
        assert!(should_touch(Some("garbage"), now));
    }

    #[test]
    fn scope_membership() {
        let all = TokenAuth {
            user_id: "u".into(),
            token_id: "t".into(),
            label: "l".into(),
            book_scope: None,
        };
        assert!(all.in_scope("any"));
        let one = TokenAuth {
            book_scope: Some(vec!["b1".into()]),
            ..all.clone()
        };
        assert!(one.in_scope("b1"));
        assert!(!one.in_scope("b2"));
    }
}
