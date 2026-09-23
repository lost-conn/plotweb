//! Personal access tokens: management over the session, `whoami` over bearer
//! auth, and the scope/ownership rules card B's MCP endpoint will lean on.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use plotweb_server::token_auth::TokenAuth;
use serde_json::{json, Value};

/// Create a token over the session and return `(raw_token, info_json)`.
async fn mint(app: &mut TestApp, label: &str, book_ids: Value) -> (String, Value) {
    let r = app
        .post("/api/tokens", &json!({ "label": label, "book_ids": book_ids }))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "create token: {}", r.json);
    let token = r.json["token"].as_str().expect("raw token").to_string();
    (token, r.json["info"].clone())
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_returns_raw_token_once_and_list_is_metadata_only() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;

    let (token, info) = mint(&mut app, "  Claude desktop  ", Value::Null).await;
    assert!(token.starts_with("pw_"), "token format: {token}");
    assert!(token.len() >= 3 + 43, "at least 32 bytes of entropy: {token}");
    assert_eq!(info["label"], "Claude desktop", "label is trimmed");
    assert_eq!(info["prefix"], token[3..11].to_string());
    assert_eq!(info["book_ids"], Value::Null);
    assert_eq!(info["last_used_at"], Value::Null);

    let list = app.get("/api/tokens").await;
    assert_eq!(list.status, StatusCode::OK);
    let arr = list.json.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], info["id"]);
    assert_eq!(arr[0]["label"], "Claude desktop");

    // Neither the raw token nor its hash appears anywhere in the listing.
    let serialized = list.json.to_string();
    assert!(!serialized.contains(&token), "raw token leaked: {serialized}");
    let hash = plotweb_server::auth::hash_token(&token);
    assert!(!serialized.contains(&hash), "hash leaked: {serialized}");
    assert!(!serialized.contains("token_hash"), "hash field leaked: {serialized}");
    let keys: Vec<&String> = arr[0].as_object().unwrap().keys().collect();
    assert_eq!(
        keys.len(),
        6,
        "only id/label/prefix/book_ids/created_at/last_used_at: {keys:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn whoami_accepts_a_fresh_token_and_rejects_everything_else() {
    let mut app = TestApp::new().await;
    let uid = app.register("bella", "hunter2hunter2").await;
    let book = app.create_book("Scoped").await;
    let (token, info) = mint(&mut app, "agent", json!([book])).await;

    let r = app
        .bearer("GET", "/api/tokens/whoami", Some(&bearer(&token)), None)
        .await;
    assert_eq!(r.status, StatusCode::OK, "whoami: {}", r.json);
    assert_eq!(r.json["user_id"], uid);
    assert_eq!(r.json["username"], "bella");
    assert_eq!(r.json["token_label"], "agent");
    assert_eq!(r.json["book_ids"], json!([book]));

    // Using the token stamps last_used_at.
    let list = app.get("/api/tokens").await;
    assert!(
        list.json[0]["last_used_at"].is_string(),
        "last_used_at set after use: {}",
        list.json
    );
    assert_eq!(list.json[0]["id"], info["id"]);

    // Missing, malformed and unknown tokens are all a 401.
    let bogus = format!("pw_{}", "A".repeat(43));
    for header in [
        None,
        Some("".to_string()),
        Some(token.clone()),                  // no scheme
        Some(format!("Basic {token}")),       // wrong scheme
        Some("Bearer not-a-pw-token".into()), // wrong marker
        Some("Bearer pw_".into()),            // marker only
        Some(bearer(&bogus)),                 // well-formed, unknown
    ] {
        let r = app
            .bearer("GET", "/api/tokens/whoami", header.as_deref(), None)
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "header {header:?}");
        assert!(r.json["error"].is_string(), "JSON error body: {}", r.json);
    }

    // The session alone does not satisfy a bearer route.
    let r = app.get("/api/tokens/whoami").await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // Revoked → 401.
    let id = info["id"].as_str().unwrap();
    let del = app.delete(&format!("/api/tokens/{id}")).await;
    assert_eq!(del.status, StatusCode::OK, "revoke: {}", del.json);
    let r = app
        .bearer("GET", "/api/tokens/whoami", Some(&bearer(&token)), None)
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(app.get("/api/tokens").await.json, json!([]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn management_endpoints_and_session_routes_refuse_bearer_tokens() {
    let mut app = TestApp::new().await;
    app.register("carl", "hunter2hunter2").await;
    let (token, info) = mint(&mut app, "agent", Value::Null).await;
    let auth = bearer(&token);
    let id = info["id"].as_str().unwrap();

    let r = app.bearer("GET", "/api/tokens", Some(&auth), None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "list with bearer");
    let r = app
        .bearer(
            "POST",
            "/api/tokens",
            Some(&auth),
            Some(&json!({ "label": "escalate", "book_ids": null })),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "create with bearer");
    let r = app
        .bearer("DELETE", &format!("/api/tokens/{id}"), Some(&auth), None)
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "revoke with bearer");

    // An existing session route does not accept a token either.
    let r = app.bearer("GET", "/api/books", Some(&auth), None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "GET /api/books with bearer");
    let r = app.bearer("GET", "/api/auth/me", Some(&auth), None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "GET /api/auth/me with bearer");

    // Still intact: nothing above minted or revoked anything.
    let list = app.get("/api/tokens").await;
    assert_eq!(list.json.as_array().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn users_cannot_see_or_revoke_each_others_tokens() {
    let mut app = TestApp::new().await;
    app.register("dora", "hunter2hunter2").await;
    let (a_token, a_info) = mint(&mut app, "dora's agent", Value::Null).await;
    let a_id = a_info["id"].as_str().unwrap().to_string();

    app.logout_local();
    app.register("eli", "hunter2hunter2").await;
    assert_eq!(app.get("/api/tokens").await.json, json!([]), "B sees none of A's");

    let r = app.delete(&format!("/api/tokens/{a_id}")).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "B revoking A's token");
    let r = app.delete("/api/tokens/does-not-exist").await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "unknown id looks the same");

    // A's token still works.
    let r = app
        .bearer("GET", "/api/tokens/whoami", Some(&bearer(&a_token)), None)
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json["username"], "dora");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_validates_label_and_book_ownership() {
    let mut app = TestApp::new().await;
    app.register("fay", "hunter2hunter2").await;
    let other_book = app.create_book("Fay's").await;

    app.logout_local();
    app.register("gus", "hunter2hunter2").await;
    let own = app.create_book("Gus's").await;

    let cases = [
        json!({ "label": "", "book_ids": null }),
        json!({ "label": "   ", "book_ids": null }),
        json!({ "label": "x".repeat(65), "book_ids": null }),
        json!({ "label": "ok", "book_ids": [other_book] }),
        json!({ "label": "ok", "book_ids": [own, other_book] }),
        json!({ "label": "ok", "book_ids": ["no-such-book"] }),
        json!({ "label": "ok", "book_ids": [] }),
    ];
    for body in cases {
        let r = app.post("/api/tokens", &body).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "body {body}: {}", r.json);
    }
    assert_eq!(app.get("/api/tokens").await.json, json!([]), "nothing created");

    // 64 characters is fine, and a duplicate id collapses.
    let (_, info) = mint(&mut app, &"y".repeat(64), json!([own, own])).await;
    assert_eq!(info["book_ids"], json!([own]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn can_access_book_combines_ownership_and_scope() {
    let mut app = TestApp::new().await;
    app.register("hana", "hunter2hunter2").await;
    let foreign = app.create_book("Not yours").await;

    app.logout_local();
    app.register("ivan", "hunter2hunter2").await;
    let b1 = app.create_book("One").await;
    let b2 = app.create_book("Two").await;
    let (all_token, _) = mint(&mut app, "all", Value::Null).await;
    let (scoped_token, _) = mint(&mut app, "scoped", json!([b1])).await;

    let state = app.state().clone();
    let all = TokenAuth::authenticate(&state, Some(&bearer(&all_token)))
        .await
        .expect("all-books token authenticates");
    let scoped = TokenAuth::authenticate(&state, Some(&bearer(&scoped_token)))
        .await
        .expect("scoped token authenticates");

    // All-books token: every book the account owns, and nothing else.
    assert!(all.can_access_book(&state, &b1).await);
    assert!(all.can_access_book(&state, &b2).await);
    assert!(
        !all.can_access_book(&state, &foreign).await,
        "another user's book is out of reach even with an all-books token"
    );
    assert!(!all.can_access_book(&state, "no-such-book").await);

    // Scoped token: in scope yes, out of scope no, foreign no.
    assert!(scoped.can_access_book(&state, &b1).await);
    assert!(!scoped.can_access_book(&state, &b2).await, "owned but out of scope");
    assert!(!scoped.can_access_book(&state, &foreign).await);

    // A book deleted after the token was scoped to it is no longer reachable.
    let r = app.delete(&format!("/api/books/{b1}")).await;
    assert!(r.status.is_success(), "delete book: {}", r.json);
    assert!(!scoped.can_access_book(&state, &b1).await);
}
