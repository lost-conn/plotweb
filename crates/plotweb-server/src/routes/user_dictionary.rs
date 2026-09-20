//! The signed-in user's custom spellcheck dictionary.
//!
//! One row per account (`UserDictionary` in `rhypedb/schema.rhype`), holding the
//! words the author told the editor were not mistakes. The client is the local-first
//! owner of this list — it keeps its own copy in `rinch-storage`, merges the server's
//! copy into it as a union, and pushes the whole set back — so the write endpoint is
//! a **replace**, not an append.
//!
//! Normalisation (trim, drop empties, de-duplicate, sort) happens here rather than
//! on the client so two devices that added the same word in different order still
//! converge on identical bytes.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use plotweb_common::{
    MAX_USER_DICTIONARY_WORD_LEN, MAX_USER_DICTIONARY_WORDS, UpdateUserDictionaryRequest,
    UserDictionary,
};
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthSession;
use crate::rhype::{Fields, quote};

/// The largest `PUT` body accepted, applied as a `DefaultBodyLimit` on the route in
/// `lib.rs`. 10,000 words at the 64-byte cap plus JSON overhead is comfortably under
/// this; anything larger is not a dictionary.
pub const MAX_DICTIONARY_BODY: usize = 1024 * 1024;

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Trim, drop empties and over-long entries, de-duplicate (case-**sensitively**, so
/// `Elowen` and `elowen` are distinct entries the way the speller treats them), and
/// sort.
fn normalize(words: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = words
        .into_iter()
        .map(|w| w.trim().to_string())
        .filter(|w| !w.is_empty() && w.chars().count() <= MAX_USER_DICTIONARY_WORD_LEN)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The account's row, if it has one.
async fn read_row(state: &AppState, user_id: &str) -> Option<(Vec<String>, String)> {
    let row = state
        .rhype
        .find_one(format!(
            "UserDictionary.filter(.user_id == {}).limit(1)",
            quote(user_id)
        ))
        .await
        .ok()
        .flatten()?;
    let words = row
        .str("words_json")
        .and_then(|j| serde_json::from_str::<Vec<String>>(j).ok())
        .unwrap_or_default();
    Some((words, row.string("updated_at").unwrap_or_default()))
}

/// GET /api/me/dictionary
///
/// An account with no row answers `200` with an empty list, not `404`: "you have
/// never added a word" is the normal state, not a missing resource.
pub async fn get(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
) -> impl IntoResponse {
    let (words, updated_at) = read_row(&state, &user_id).await.unwrap_or_default();
    (
        StatusCode::OK,
        Json(serde_json::to_value(UserDictionary { words, updated_at }).unwrap()),
    )
}

/// PUT /api/me/dictionary
///
/// Replaces the account's word list with the normalised `words`, and answers with
/// what was stored — so the client can adopt the server's normalisation rather than
/// re-deriving it.
pub async fn update(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    Json(req): Json<UpdateUserDictionaryRequest>,
) -> impl IntoResponse {
    let words = normalize(req.words);
    if words.len() > MAX_USER_DICTIONARY_WORDS {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({
                "error": format!(
                    "custom dictionary is limited to {MAX_USER_DICTIONARY_WORDS} words"
                )
            })),
        );
    }

    let words_json = serde_json::to_string(&words).unwrap_or_else(|_| "[]".into());
    let updated_at = now();
    let existed = read_row(&state, &user_id).await.is_some();

    let result = if existed {
        state
            .rhype
            .exec(format!(
                "UserDictionary.filter(.user_id == {}).update({})",
                quote(&user_id),
                Fields::new()
                    .str("words_json", &words_json)
                    .str("updated_at", &updated_at)
                    .render()
            ))
            .await
            .map(|_| ())
    } else {
        state
            .rhype
            .create(format!(
                "UserDictionary.create({})",
                Fields::new()
                    .str("uuid", &Uuid::new_v4().to_string())
                    .str("user_id", &user_id)
                    .str("words_json", &words_json)
                    .str("updated_at", &updated_at)
                    .render()
            ))
            .await
            .map(|_| ())
    };

    if let Err(e) = result {
        eprintln!("Failed to save user dictionary: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to save dictionary" })),
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::to_value(UserDictionary { words, updated_at }).unwrap()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trims_dedupes_and_sorts() {
        let got = normalize(vec![
            "  Elowen ".into(),
            "".into(),
            "aeth".into(),
            "Elowen".into(),
            "   ".into(),
            "Aeth".into(),
        ]);
        assert_eq!(got, vec!["Aeth", "Elowen", "aeth"]);
    }

    #[test]
    fn normalize_drops_absurdly_long_entries() {
        let long = "x".repeat(MAX_USER_DICTIONARY_WORD_LEN + 1);
        let got = normalize(vec![long, "ok".into()]);
        assert_eq!(got, vec!["ok"]);
    }
}
